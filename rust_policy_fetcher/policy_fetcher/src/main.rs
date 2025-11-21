mod fetcher;

use std::path::PathBuf;
use std::time::Duration;

use fetcher::{make_extract_callback, start_bundle_poller};
use reqwest::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new();

    // Remote secure repo (GitHub, internal service, etc.)
    let bundle_url = "https://github.com/gngram/policy-store/archive/refs/heads/main.tar.gz".to_string();
    println!("Policy Repo URL: {}", bundle_url);

    // Where to store the raw archive
    let archive_dir = PathBuf::from("./Downloads");
    println!("Archive download path:{}", archive_dir.display());

    // Where to extract /policy contents
    let policy_dir = PathBuf::from("./Policies");
    println!("Policy extract path:{}", policy_dir.display());

    // Access token file (optional)
    let token_file = Some(PathBuf::from("./token.txt"));
    println!("URL access token:{}", token_file.as_ref().unwrap().display());

    let callback = make_extract_callback(policy_dir);

    // Start background poller; returns immediately
    start_bundle_poller(
        client,
        bundle_url,
        archive_dir,
        Duration::from_secs(10),
        token_file,
        callback,
    );

    // Your app can keep running (HTTP server, VM manager, etc.)
    // Here we just park the main task.
    loop {
        tokio::time::sleep(Duration::from_secs(20)).await;
    }
}

