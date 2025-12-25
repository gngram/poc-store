use anyhow::{Context, Result};
use reqwest::{
    header::{ETAG, LAST_MODIFIED},
    Client,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{sync::Mutex, time::sleep};

const POLICY_JSON: &str = "policies.json";

/// Config format:
/// {
///   "policy-name": {
///     "url": "...",
///     "vms": ["vm1","vm2"],
///     "poll_interval_secs": 30,
///     "head_ref": "etag:abc"   // added by program
///   }
/// }
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PolicyConfig {
    url: String,
    vms: Vec<String>,
    poll_interval_secs: u64,

    /// Stored state used by the program (ETag, Last-Modified or hash)
    #[serde(default)]
    head_ref: Option<String>,
}

/// Map from policy-name → configuration
type PolicyMap = HashMap<String, PolicyConfig>;

#[derive(Clone)]
struct PolicyPoller {
    client: Client,
    policies: Arc<Mutex<PolicyMap>>,
    json_path: String,
    output_dir: PathBuf,
}

impl PolicyPoller {
    /// Load from JSON and build a poller.
    fn from_file(path: &str, output_dir: impl Into<PathBuf>) -> Result<Self> {
        let map: PolicyMap = if Path::new(path).exists() {
            let raw = fs::read_to_string(path)
                .with_context(|| format!("Failed to read {}", path))?;
            serde_json::from_str(&raw)
                .with_context(|| "Failed to parse policies JSON")?
        } else {
            HashMap::new()
        };

        Ok(Self {
            client: Client::new(),
            policies: Arc::new(Mutex::new(map)),
            json_path: path.to_string(),
            output_dir: output_dir.into(),
        })
    }

    /// Save the current policies map to disk.
    async fn save(&self) -> Result<()> {
        // Take a snapshot under the lock, then drop the lock
        let snapshot = {
            let guard = self.policies.lock().await;
            guard.clone()
        };

        let json = serde_json::to_string_pretty(&snapshot)
            .context("Failed to serialize policies to JSON")?;
        fs::write(&self.json_path, json)
            .with_context(|| format!("Failed to write {}", self.json_path))?;
        Ok(())
    }

    /// Spawn a polling task per policy and wait forever.
    async fn run(self) -> Result<()> {
        let snapshot = {
            let guard = self.policies.lock().await;
            guard.keys().cloned().collect::<Vec<_>>()
        };

        let mut handles = Vec::new();
        for policy_name in snapshot {
            let poller = self.clone();
            let handle = tokio::spawn(async move {
                if let Err(err) = poller.poll_policy_loop(policy_name.clone()).await {
                    eprintln!("Error in poller task for {}: {:#}", policy_name, err);
                }
            });
            handles.push(handle);
        }

        for h in handles {
            let _ = h.await;
        }

        Ok(())
    }

    /// Infinite loop for one policy.
    async fn poll_policy_loop(&self, policy_name: String) -> Result<()> {
        loop {
            let interval_secs = {
                let guard = self.policies.lock().await;
                guard
                    .get(&policy_name)
                    .map(|cfg| cfg.poll_interval_secs)
                    .unwrap_or(60)
            };

            if let Err(err) = self.poll_once(&policy_name).await {
                eprintln!("Error polling {}: {:#}", policy_name, err);
            }

            sleep(Duration::from_secs(interval_secs)).await;
        }
    }

    /// One poll iteration: check HEAD, download on change, update metadata.
    async fn poll_once(&self, policy_name: &str) -> Result<()> {
        let (url, current_head) = {
            let guard = self.policies.lock().await;
            let cfg = match guard.get(policy_name) {
                Some(cfg) => cfg,
                None => {
                    eprintln!("Policy {} not found; skipping.", policy_name);
                    return Ok(());
                }
            };
            (cfg.url.clone(), cfg.head_ref.clone())
        };

        let head_resp = self.client.head(&url).send().await?;
        if !head_resp.status().is_success() {
            eprintln!(
                "HEAD {} ({}) returned non-success status: {}",
                policy_name,
                url,
                head_resp.status()
            );
            return Ok(());
        }

        let headers = head_resp.headers();
        let mut new_head: Option<String> = None;

        if let Some(etag_val) = headers.get(ETAG) {
            if let Ok(s) = etag_val.to_str() {
                new_head = Some(format!("etag:{s}"));
            }
        }
        if new_head.is_none() {
            if let Some(lm_val) = headers.get(LAST_MODIFIED) {
                if let Ok(s) = lm_val.to_str() {
                    new_head = Some(format!("last-modified:{s}"));
                }
            }
        }

        let need_hash = new_head.is_none();

        if !need_hash && new_head == current_head {
            println!("No change → {policy_name}");
            return Ok(());
        }

        println!("Change detected for {policy_name}, downloading…");

        let get_resp = self.client.get(&url).send().await?;
        if !get_resp.status().is_success() {
            eprintln!(
                "GET {} ({}) returned non-success status: {}",
                policy_name,
                url,
                get_resp.status()
            );
            return Ok(());
        }

        let body = get_resp.bytes().await?;

        let final_head = if need_hash {
            let mut hasher = Sha256::new();
            hasher.update(&body);
            Some(format!("sha256:{:x}", hasher.finalize()))
        } else {
            new_head
        };
        // Get the file name from url
        let policy_file = url
            .split('/')
            .last()
            .unwrap_or("unknown_policy.bin");

        // Decide destination per policy: ./pulled-policies/<policy_name>/policy-file
        let dest = self.output_dir.join(policy_name).join(policy_file);

        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {:?}", parent))?;
        }
        fs::write(&dest, &body)
            .with_context(|| format!("Failed to write file {:?}", dest))?;

        println!("Saved {policy_name} to {:?}", dest);

        // Update head_ref in memory
        {
            let mut guard = self.policies.lock().await;
            if let Some(cfg) = guard.get_mut(policy_name) {
                cfg.head_ref = final_head;
            } else {
                eprintln!(
                    "Policy {} disappeared from map while updating; not saving head_ref.",
                    policy_name
                );
                return Ok(());
            }
        }

        // Persist JSON to disk (no lock held here)
        if let Err(e) = self.save().await {
            eprintln!("Failed to save {}: {:#}", self.json_path, e);
        }

        // Call your dummy "apply" function
        self.on_policy_updated(policy_name).await;

        Ok(())
    }

    /// Dummy function: print policy name + target VMs.
    async fn on_policy_updated(&self, policy_name: &str) {
        let vms = {
            let guard = self.policies.lock().await;
            guard
                .get(policy_name)
                .map(|cfg| cfg.vms.clone())
                .unwrap_or_default()
        };

        println!(
            "🚀 POLICY UPDATED → {}\n   Target VMs: {:?}\n",
            policy_name, vms
        );
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Files will be saved under ./pulled-policies/<policy>.bin
    let poller = PolicyPoller::from_file(POLICY_JSON, "./pulled-policies")?;
    poller.run().await
}
