#![allow(dead_code)]

use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use pgrx::prelude::*;
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

const KEY_DIR: &str = "kerai/keys";
const KEY_FILE: &str = "private.key";

/// Get the key storage directory under PGDATA
fn key_dir() -> PathBuf {
    let pgdata = unsafe {
        let ptr = pgrx::pg_sys::DataDir;
        if ptr.is_null() {
            error!("DataDir is null — cannot determine PGDATA");
        }
        std::ffi::CStr::from_ptr(ptr)
            .to_str()
            .expect("DataDir is not valid UTF-8")
    };
    PathBuf::from(pgdata).join(KEY_DIR)
}

/// Generate a new Ed25519 keypair, save private key to PGDATA, return both keys
pub fn generate_keypair() -> (SigningKey, VerifyingKey) {
    let mut rng = rand::rngs::OsRng;
    let signing_key = SigningKey::generate(&mut rng);
    let verifying_key = signing_key.verifying_key();

    let dir = key_dir();
    fs::create_dir_all(&dir).unwrap_or_else(|e| {
        error!("Failed to create key directory {}: {}", dir.display(), e);
    });

    let key_path = dir.join(KEY_FILE);
    fs::write(&key_path, signing_key.to_bytes()).unwrap_or_else(|e| {
        error!("Failed to write private key to {}: {}", key_path.display(), e);
    });

    // Set file permissions to 0600 (owner read/write only)
    fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600)).unwrap_or_else(|e| {
        error!(
            "Failed to set permissions on {}: {}",
            key_path.display(),
            e
        );
    });

    info!("Generated Ed25519 keypair, saved to {}", key_path.display());
    (signing_key, verifying_key)
}

/// Load the signing key from PGDATA
pub fn load_signing_key() -> Option<SigningKey> {
    let key_path = key_dir().join(KEY_FILE);
    let bytes = fs::read(&key_path).ok()?;
    let key_bytes: [u8; 32] = bytes.try_into().ok()?;
    Some(SigningKey::from_bytes(&key_bytes))
}

/// Compute a SHA-256 fingerprint of the public key, base64-encoded
pub fn fingerprint(verifying_key: &VerifyingKey) -> String {
    let hash = Sha256::digest(verifying_key.as_bytes());
    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, hash)
}

/// Sign data with the signing key
pub fn sign_data(signing_key: &SigningKey, data: &[u8]) -> Vec<u8> {
    signing_key.sign(data).to_bytes().to_vec()
}

/// Verify a signature against data and public key
pub fn verify_signature(verifying_key: &VerifyingKey, data: &[u8], signature: &[u8]) -> bool {
    let sig_bytes: [u8; 64] = match signature.try_into() {
        Ok(b) => b,
        Err(_) => return false,
    };
    let sig = match ed25519_dalek::Signature::from_bytes(&sig_bytes) {
        sig => sig,
    };
    verifying_key.verify(data, &sig).is_ok()
}
