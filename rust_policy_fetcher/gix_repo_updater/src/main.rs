mod repo_updater;

use std::thread;
use std::time::Duration;
use repo_updater::RepoUpdater;
use tracing::{error, info};
use tracing_subscriber::{fmt, EnvFilter};
use tracing_subscriber::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fmt_layer = fmt::layer()
    .with_target(false);

    let filter_layer = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer)
        .init();

    let mut updater = RepoUpdater::new(
        "https://github.com/gngram/policy-store.git",
        "test_policy",
        "/work/policies",
    )?;

    info!("Starting poller... Current HEAD is: {}", updater.repo_head().unwrap());

    loop {
        info!("\n--- Checking for updates ---");
        match updater.pull() {
            Ok(Some(old_head)) => {
                let new_head = updater.repo_head().unwrap(); // Safe to unwrap, pull succeeded
                info!("Update found! Fetched changes from {} to {}", old_head, new_head);

                let changes = updater.get_change_set(&old_head.to_string(), &new_head.to_string())?;
                if !changes.is_empty() {
                    info!("Changeset:\n{}", changes);
                } else {
                    info!("Update applied, but no file changes were detected in the diff.");
                }
            }
            Ok(None) => info!("Repository is already up-to-date."),
            Err(e) => error!("An error occurred during pull: {}", e),
        }

        thread::sleep(Duration::from_secs(10));
    }
}
