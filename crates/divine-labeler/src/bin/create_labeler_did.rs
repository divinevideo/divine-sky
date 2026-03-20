//! One-off tool to create a did:plc for the divine-labeler service.
//!
//! Usage:
//!   cargo run -p divine-labeler --bin create-labeler-did -- \
//!     --signing-key <hex> --pds-endpoint <url> --handle <handle> --plc-directory <url>

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use secp256k1::{PublicKey, Secp256k1, SecretKey};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    let signing_key_hex = get_arg(&args, "--signing-key")?;
    let pds_endpoint = get_arg(&args, "--pds-endpoint")?;
    let handle = get_arg(&args, "--handle")?;
    let plc_directory = get_arg(&args, "--plc-directory")
        .unwrap_or_else(|_| "https://plc.directory".to_string());

    // Derive public key from signing key
    let key_bytes = hex::decode(&signing_key_hex).context("invalid hex signing key")?;
    let secp = Secp256k1::new();
    let secret_key = SecretKey::from_slice(&key_bytes).context("invalid secp256k1 key")?;
    let public_key = PublicKey::from_secret_key(&secp, &secret_key);

    let signing_did_key = pubkey_to_did_key(&public_key);
    let rotation_did_key = signing_did_key.clone(); // Same key for rotation (labeler is service-operated)

    eprintln!("Signing key (did:key): {signing_did_key}");

    // Build PLC operation
    let mut verification_methods = BTreeMap::new();
    verification_methods.insert("atproto".to_string(), signing_did_key);

    let mut services = BTreeMap::new();
    services.insert(
        "atproto_labeler".to_string(),
        serde_json::json!({
            "type": "AtprotoLabeler",
            "endpoint": pds_endpoint
        }),
    );

    let mut operation = serde_json::json!({
        "type": "plc_operation",
        "rotationKeys": [rotation_did_key],
        "verificationMethods": verification_methods,
        "alsoKnownAs": [format!("at://{handle}")],
        "services": services,
        "prev": null,
        "sig": ""
    });

    // Sign the operation
    let sig = sign_plc_operation(&operation, &secret_key)?;
    operation["sig"] = serde_json::Value::String(sig);

    // Derive the DID
    let did = derive_did_plc(&operation);
    eprintln!("Derived DID: {did}");
    eprintln!("PLC operation:");
    eprintln!("{}", serde_json::to_string_pretty(&operation)?);

    // Post to PLC directory
    eprintln!("\nPosting to {plc_directory}...");

    let client = reqwest::blocking::Client::new();

    // First attempt with our derived DID
    let plc_url = format!("{}/{}", plc_directory.trim_end_matches('/'), did);
    eprintln!("POST {plc_url}");
    let resp = client
        .post(&plc_url)
        .json(&operation)
        .send()
        .context("failed to reach PLC directory")?;

    let status = resp.status();
    let body = resp.text().unwrap_or_default();

    if status.is_success() || status.as_u16() == 409 {
        eprintln!("Success! (status: {status})");
        println!("{did}");
    } else if body.contains("does not match DID identifier") {
        // PLC directory computed a different hash — use their DID
        if let Some(correct_did) = body.split("does not match DID identifier: ").nth(1) {
            let correct_did = correct_did.trim().trim_end_matches(|c: char| !c.is_alphanumeric() && c != ':');
            eprintln!("DID hash mismatch — PLC directory says correct DID is: {correct_did}");
            let retry_url = format!("{}/{}", plc_directory.trim_end_matches('/'), correct_did);
            eprintln!("POST {retry_url}");
            let retry_resp = client
                .post(&retry_url)
                .json(&operation)
                .send()
                .context("failed to reach PLC directory on retry")?;
            let retry_status = retry_resp.status();
            let retry_body = retry_resp.text().unwrap_or_default();
            if retry_status.is_success() || retry_status.as_u16() == 409 {
                eprintln!("Success! (status: {retry_status})");
                println!("{correct_did}");
            } else {
                eprintln!("Failed on retry (status: {retry_status}): {retry_body}");
                std::process::exit(1);
            }
        } else {
            eprintln!("Failed (status: {status}): {body}");
            std::process::exit(1);
        }
    } else {
        eprintln!("Failed (status: {status}): {body}");
        std::process::exit(1);
    }

    Ok(())
}

fn get_arg(args: &[String], name: &str) -> Result<String> {
    let pos = args
        .iter()
        .position(|a| a == name)
        .with_context(|| format!("missing required argument: {name}"))?;
    args.get(pos + 1)
        .cloned()
        .with_context(|| format!("missing value for {name}"))
}

fn pubkey_to_did_key(pubkey: &PublicKey) -> String {
    let compressed = pubkey.serialize();
    let mut buf = vec![0xe7u8, 0x01];
    buf.extend_from_slice(&compressed);
    let encoded = bs58::encode(&buf).into_string();
    format!("did:key:z{encoded}")
}

fn sign_plc_operation(
    operation: &serde_json::Value,
    secret_key: &SecretKey,
) -> Result<String> {
    use sha2::{Digest, Sha256};

    let mut op_clone = operation.clone();
    if let Some(obj) = op_clone.as_object_mut() {
        obj.remove("sig");
    }
    let cbor_bytes = serde_ipld_dagcbor::to_vec(&op_clone)
        .context("failed to encode PLC operation as DAG-CBOR")?;
    let hash: [u8; 32] = Sha256::digest(&cbor_bytes).into();
    let msg = secp256k1::Message::from_digest(hash);
    let secp = Secp256k1::new();
    let sig = secp.sign_ecdsa(&msg, secret_key);
    Ok(data_encoding::BASE64URL_NOPAD.encode(&sig.serialize_compact()))
}

fn derive_did_plc(operation: &serde_json::Value) -> String {
    use sha2::{Digest, Sha256};

    let mut op_clone = operation.clone();
    if let Some(obj) = op_clone.as_object_mut() {
        obj.remove("sig");
    }
    let cbor_bytes = serde_ipld_dagcbor::to_vec(&op_clone)
        .expect("valid PLC operations encode to DAG-CBOR");
    let hash = Sha256::digest(&cbor_bytes);
    let encoded = data_encoding::BASE32_NOPAD
        .encode(&hash[..15])
        .to_ascii_lowercase();
    format!("did:plc:{encoded}")
}
