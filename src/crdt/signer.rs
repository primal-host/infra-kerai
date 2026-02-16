/// Canonical signable data construction and Ed25519 signature verification for CRDT operations.
use crate::identity;

/// Build the canonical byte representation of an operation for signing.
///
/// Format: `"op_type|node_id|author_seq|payload_json"` as UTF-8 bytes.
/// `node_id` is "null" when absent.
pub fn build_signable(
    op_type: &str,
    node_id: Option<&str>,
    author_seq: i64,
    payload_json: &str,
) -> Vec<u8> {
    let nid = node_id.unwrap_or("null");
    format!("{}|{}|{}|{}", op_type, nid, author_seq, payload_json).into_bytes()
}

/// Verify an Ed25519 signature over the canonical representation of an operation.
pub fn verify_op_signature(
    public_key: &[u8],
    op_type: &str,
    node_id: Option<&str>,
    author_seq: i64,
    payload_json: &str,
    signature: &[u8],
) -> bool {
    let pk_bytes: [u8; 32] = match public_key.try_into() {
        Ok(b) => b,
        Err(_) => return false,
    };
    let verifying_key = match ed25519_dalek::VerifyingKey::from_bytes(&pk_bytes) {
        Ok(k) => k,
        Err(_) => return false,
    };
    let signable = build_signable(op_type, node_id, author_seq, payload_json);
    identity::verify_signature(&verifying_key, &signable, signature)
}
