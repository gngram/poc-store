use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use flate2::read::GzDecoder;
use reqwest::{Client, StatusCode};
use tar::Archive;
use tokio::time::sleep;

/// Callback is called when a *new* bundle is downloaded.
/// It receives the archive path and should return Result.
pub type BundleCallback =
    Arc<dyn Fn(&Path) -> Result<(), Box<dyn std::error::Error + Send + Sync>> + Send + Sync>;

/// Load token from file if it exists and is non-empty.
fn load_token_from_file(path: &Path) -> Option<String> {
    if !path.exists() {
        return None;
    }
    match fs::read_to_string(path) {
        Ok(s) => {
            let trimmed = s.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        }
        Err(e) => {
            eprintln!("Failed to read token file {}: {e}", path.display());
            None
        }
    }
}

fn write_etag_to_file(path: &Path, etag: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?; // ensure directory exists
    }
    fs::write(path, etag.trim())?;   // overwrite file
    Ok(())
}

fn load_etag_from_file(path: &Path) -> Option<String> {
    if !path.exists() {
        return None;
    }

    match fs::read_to_string(path) {
        Ok(s) => {
            let trimmed = s.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        }
        Err(e) => {
            eprintln!("Failed to read ETag file {}: {e}", path.display());
            None
        }
    }
}


/// Start a background poller that periodically checks a bundle URL,
/// downloads the bundle when changed, writes it to `archive_dir/bundle.tar.gz`,
/// and then invokes `on_new_bundle` callback.
///
/// - `token_file` is optional; if present and readable, its contents
///   are used as a Bearer token for auth.
/// - Returns immediately; the poller runs in a tokio task.
pub fn start_bundle_poller(
    client: Client,
    bundle_url: String,
    archive_dir: PathBuf,
    poll_interval: Duration,
    token_file: Option<PathBuf>,
    on_new_bundle: BundleCallback,
) {
    tokio::spawn(async move {
        let etag_file = archive_dir.join("etag.txt");
        let mut last_etag: Option<String> = load_etag_from_file(&etag_file);
        // Ensure archive dir exists
        if let Err(e) = fs::create_dir_all(&archive_dir) {
            eprintln!(
                "Failed to create archive directory {}: {e}",
                archive_dir.display()
            );
            return;
        }

        loop {
            sleep(poll_interval).await;

            // Build request
            let mut req = client.get(&bundle_url);

            // Auth token (if file exists and is non-empty)
            if let Some(token_path) = token_file.as_ref() {
                if let Some(token) = load_token_from_file(token_path) {
                    // Adjust this if your auth scheme is different
                    req = req.bearer_auth(token);
                }
            }

            // Conditional request if we have an ETag
            if let Some(etag) = last_etag.as_deref() {
                req = req.header(reqwest::header::IF_NONE_MATCH, etag);
            }

            let resp = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("Bundle poll HTTP error: {e:?}");
                    continue;
                }
            };

            // Not modified → nothing to do
            if resp.status() == StatusCode::NOT_MODIFIED {
                continue;
            }

            if !resp.status().is_success() {
                eprintln!("Bundle poll non-success status: {}", resp.status());
                continue;
            }

            // New ETag (if any)
            let new_etag = resp
                .headers()
                .get(reqwest::header::ETAG)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());

            let body_bytes = match resp.bytes().await {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("Failed to read bundle body: {e:?}");
                    continue;
                }
            };

            // Write bundle archive to file
            let archive_path = archive_dir.join("bundle.tar.gz");
            if let Err(e) = fs::write(&archive_path, &body_bytes) {
                eprintln!(
                    "Failed to write bundle archive to {}: {e}",
                    archive_path.display()
                );
                continue;
            }

            // Update ETag state + persist to file
            if let Some(ref et) = new_etag {
                last_etag = Some(et.clone());
                if let Err(e) = write_etag_to_file(&etag_file, et) {
                    eprintln!("Failed to update ETag file {}: {e}", etag_file.display());
                }
            }

            // Invoke callback
            if let Err(e) = on_new_bundle(&archive_path) {
                eprintln!("on_new_bundle callback error: {e:?}");
            }
        }
    });
}

/// Callback that extracts the *entire* archive into `extract_dir`
/// preserving all directory structure exactly as stored in the tar.gz.
/// After extraction, you can perform any custom logic you want.
pub fn make_extract_callback(extract_dir: PathBuf) -> BundleCallback {
    Arc::new(move |archive_path: &Path| {
        // Ensure extraction directory exists
        fs::create_dir_all(&extract_dir)?;

        // Read archive into memory
        let data = fs::read(archive_path)?;
        let gz = GzDecoder::new(Cursor::new(data));
        let mut archive = Archive::new(gz);

        // Extract the entire archive into extract_dir
        archive.unpack(&extract_dir)?;

        // Custom post-extraction logic
        println!(
            "Extracted full bundle from {} into {}",
            archive_path.display(),
            extract_dir.display()
        );

        Ok(())
    })
}

