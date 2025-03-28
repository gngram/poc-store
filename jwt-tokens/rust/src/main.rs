use std::env;
use std::fs;
use std::path::Path;
use std::process;
use jsonwebtoken::{encode, Header, Algorithm, EncodingKey};
use std::time::{SystemTime, UNIX_EPOCH};
use prost::Message;

mod pb {
    include!(concat!(env!("OUT_DIR"), "/token.rs"));
}

fn generate_token(secret: &str, purpose: &str, src: pb::VmInfo, dst: pb::VmInfo) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let claims = pb::TokenClaims {
        purpose: purpose.to_string(),
        src_vm: Some(src),
        dst_vm: Some(dst),
        iat: now,
        exp: now + 300, // Token expires in 5 minutes
    };

    let mut buf = Vec::new();
    claims.encode(&mut buf).unwrap();

    encode(
        &Header::new(Algorithm::HS256),
        &buf,
        &EncodingKey::from_secret(secret.as_ref()),
    )
    .unwrap()
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <secret_key> <output_file>", args[0]);
        process::exit(1);
    }

    let secret = &args[1];
    let output_file = &args[2];
    let output_path = Path::new(output_file);
    if let Some(parent) = output_path.parent() {
        if !parent.exists() {
            println!("Directory {} does not exist, creating it...", parent.display());
            if let Err(e) = fs::create_dir_all(parent) {
                eprintln!("Failed to create directory: {}", e);
                process::exit(1);
            }
        }
    }

    // Example token generation
    let src_vm = pb::VmInfo {
        id: "vm-1".into(),
        name: "source-vm".into(),
        ip: "192.168.1.10".into(),
    };
    let dst_vm = pb::VmInfo {
        id: "vm-2".into(),
        name: "destination-vm".into(),
        ip: "192.168.1.20".into(),
    };
    let token = generate_token(secret, "data-transfer", src_vm, dst_vm);

    if let Err(e) = fs::write(output_file, &token) {
        eprintln!("Failed to write token to file: {}", e);
        process::exit(1);
    }

    println!("Token successfully written to {}", output_file);
}
