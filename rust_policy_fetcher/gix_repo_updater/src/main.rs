mod repo_updater;

use std::thread;
use std::time::Duration;
use std::collections::HashSet;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use flate2::write::GzEncoder;
use flate2::Compression;
use tar::Builder;
use repo_updater::RepoUpdater;
use tracing::{error, info};
use tracing_subscriber::{fmt, EnvFilter};
use tracing_subscriber::prelude::*;

fn changed_vm_policy_dirs(changeset: &str) -> Vec<String> {
    let mut dirs = HashSet::new();

    for line in changeset.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Expect format like: "M  vm-policies/gui-vm/rules.json"
        // Split once on whitespace to drop the status part.
        let mut parts = line.split_whitespace();

        // First is status ("M", "A", etc.), second is the path
        let _status = parts.next();
        let path = match parts.next() {
            Some(p) => p,
            None => continue,
        };

        // We only care about paths that are within vm-policies/
        const PREFIX: &str = "vm-policies/";
        if !path.starts_with(PREFIX) {
            continue;
        }

        // Take the component immediately after "vm-policies/"
        // e.g. "vm-policies/gui-vm/rules.json" -> "gui-vm"
        let rest = &path[PREFIX.len()..];
        if let Some(first_component) = rest.split('/').next() {
            if !first_component.is_empty() {
                dirs.insert(first_component.to_string());
            }
        }
    }

    // Turn into sorted Vec if you want deterministic order
    let mut result: Vec<String> = dirs.into_iter().collect();
    result.sort();
    result
}

fn create_vm_tar(vm_root: &Path, vm_name: &str, output_dir: &Path) -> anyhow::Result<()> {
    let vm_path = vm_root.join(vm_name);
    if !vm_path.exists() {
        anyhow::bail!("VM directory does not exist: {}", vm_path.display());
    }
    /* Return if vm_root doesn't exists */
    if !vm_root.exists() {
        return Ok(());
    }

    let out_file_path = output_dir.join(format!("{}.tar.gz", vm_name));
    let tar_gz = File::create(&out_file_path)?;
    let enc = GzEncoder::new(tar_gz, Compression::default());
    let mut tar = Builder::new(enc);

    // Iterate all files recursively inside vm-policies/<vmname>
    for entry in walkdir::WalkDir::new(&vm_path) {
        let entry = entry?;
        let path = entry.path();

        if path == vm_path {
            continue; // skip the root folder itself
        }

        let relative_path = path.strip_prefix(&vm_path)?;

        // Add the file to the tar with ONLY the relative path
        tar.append_path_with_name(path, relative_path)?;
    }

    tar.finish()?;
    println!("Created {}", out_file_path.display());
    Ok(())
}

fn ensure_archives_if_head_changed(
    vm_root: &Path,
    output_dir: &Path,
    head_file_path: &Path,
    new_head: &str,
) -> anyhow::Result<()> {
    if !vm_root.exists() {
        return Ok(());
    }

    let old_head = fs::read_to_string(head_file_path)
        .ok()
        .map(|s| s.trim().to_string());

    if let Some(old) = &old_head {
        if old == new_head {
            info!("Policy cache is up-to-date.");
            return Ok(());
        }
    }

    fs::remove_dir_all(output_dir)?;
    fs::create_dir_all(output_dir)?;

    for entry in fs::read_dir(vm_root)? {
        let entry = entry?;
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            let vm_name = entry
                .file_name()
                .into_string()
                .map_err(|os| anyhow::anyhow!("Non-UTF8 VM directory name: {:?}", os))?;

            create_vm_tar(vm_root, &vm_name, output_dir)?;
        }
    }

    let mut head_file = File::create(head_file_path)?;
    head_file.write_all(new_head.as_bytes())?;
    info!("Policy cache updated");

    Ok(())
}

fn spawn_poller_thread() -> thread::JoinHandle<()> {
    thread::spawn(|| {

        let policydir = Path::new("/work/policies/data");
        let vm_root = policydir.join("vm-policies");
        let output_dir = Path::new("/work/policies/.cache"); // where archives will be written
        let head_file_path = output_dir.join("head.txt");

        let mut updater = match RepoUpdater::new(
            "https://github.com/gngram/policy-store.git",
            "test_policy",
            policydir,
        ) {
            Ok(u) => u,
            Err(e) => {
                error!("Failed to initialize RepoUpdater: {}", e);
                return; // bail out of the thread
            }
        };

        let head_str = updater
            .repo_head()
            .map(|h| h.to_string())
            .unwrap_or_else(|| "UNKNOWN".into());

        info!("Current HEAD is: {}", head_str);


        let _ = ensure_archives_if_head_changed(
            &vm_root,
            output_dir,
            &head_file_path,
            &head_str,
        );

        loop {
            info!("\n--- Checking for updates ---");
            match updater.pull() {
                Ok(Some(old_head)) => {
                    let new_head = updater.repo_head().unwrap(); // safe if pull succeeded
                    info!("Update found! Fetched changes from {} to {}", old_head, new_head);

                    match updater.get_change_set(&old_head.to_string(), &new_head.to_string()) {
                        Ok(changes) => {
                            if !changes.is_empty() {
                                info!("Changeset:\n{}", changes);
                                let changed_vms = changed_vm_policy_dirs(&changes);
                                info!("Changed vm-policies subdirs: {:?}", changed_vms);
                                for vm in changed_vms {
                                    let res = create_vm_tar(vm_root.as_path(), &vm, &output_dir);
                                    if let Err(e) = res {
                                        error!("Failed to create tar for {}: {}", vm, e);
                                    }
                                    /* write new head to a file head.txt in output_dir */

                                    let head_file = File::create(&head_file_path);
                                    let res = head_file.expect("REASON").write_all(new_head.to_string().as_bytes());
                                    if let Err(e) = res {
                                        error!("Failed to write head to file: {}", e);
                                    }
                                }
                            } else {
                                info!("Update applied, but no file changes were detected in the diff.");
                            }
                        }
                        Err(e) => error!("Failed to compute change set: {}", e),
                    }
                }
                Ok(None) => info!("Repository is already up-to-date."),
                Err(e) => error!("An error occurred during pull: {}", e),
            }

            thread::sleep(Duration::from_secs(10));
        }
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // --- tracing setup ---
    let fmt_layer = fmt::layer().with_target(false);

    let filter_layer = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer)
        .init();

    // Just spawn the background poller
    let handle = spawn_poller_thread();

    // If you want main to block forever on this:
    handle.join().expect("poller thread panicked");

    Ok(())
}


/*
    fn main() {
        let changes = r#"M  vm-policies
    M  vm-policies/gui-vm
    M  vm-policies/gui-vm/rules.json
    M  vm-policies/ghaf-host
    M  vm-policies/ghaf-host/rules.json
    M  some-other-dir/file.txt
    "#;

        let dirs = changed_vm_policy_dirs(changes);
        println!("Changed vm-policies subdirs: {:?}", dirs);
        // Output: ["ghaf-host", "gui-vm"]
    }
*/
