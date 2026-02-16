/// Zero-knowledge proof stubs — attestation-only mode using SHA-256 commitments.
/// Real ZK-STARK/SNARK implementation will replace these stubs in a future iteration.
use pgrx::prelude::*;
use sha2::{Digest, Sha256};

/// Generate a proof for an attestation.
/// Currently produces a SHA-256 commitment over the attestation's underlying data
/// (scope, claim_type, perspective_count, avg_weight). This is an "attestation-only"
/// proof that commits to the claimed values without zero-knowledge properties.
/// Future: Replace with ZK-STARK proof generation.
#[pg_extern]
fn generate_proof(attestation_id: pgrx::Uuid) -> pgrx::JsonB {
    // Fetch attestation data
    let att = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT jsonb_build_object(
            'id', id,
            'scope', scope::text,
            'claim_type', claim_type,
            'perspective_count', perspective_count,
            'avg_weight', avg_weight,
            'compute_cost', compute_cost,
            'uniqueness_score', uniqueness_score,
            'instance_id', instance_id
        ) FROM kerai.attestations WHERE id = '{}'::uuid",
        attestation_id,
    ))
    .unwrap_or(None);

    let att = match att {
        Some(a) => a,
        None => error!("Attestation not found: {}", attestation_id),
    };

    let obj = att.0.as_object().unwrap();

    // Build commitment: SHA-256(scope || claim_type || perspective_count || avg_weight)
    let mut hasher = Sha256::new();
    hasher.update(obj["scope"].as_str().unwrap_or("").as_bytes());
    hasher.update(obj["claim_type"].as_str().unwrap_or("").as_bytes());
    hasher.update(obj["perspective_count"].as_i64().unwrap_or(0).to_le_bytes());
    let avg_w = obj["avg_weight"].as_f64().unwrap_or(0.0);
    hasher.update(avg_w.to_le_bytes());
    let hash = hasher.finalize();

    let proof_hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();

    // Store proof in attestation
    Spi::run(&format!(
        "UPDATE kerai.attestations
         SET proof_type = 'sha256_commitment', proof_data = '\\x{}'::bytea
         WHERE id = '{}'::uuid",
        proof_hex, attestation_id,
    ))
    .unwrap();

    pgrx::JsonB(serde_json::json!({
        "attestation_id": attestation_id.to_string(),
        "proof_type": "sha256_commitment",
        "proof_hex": proof_hex,
        "note": "Attestation-only mode. ZK-STARK proofs will replace this.",
    }))
}

/// Verify a proof for an attestation.
/// Currently re-computes the SHA-256 commitment and compares.
/// Future: Replace with ZK-STARK proof verification.
#[pg_extern]
fn verify_proof(attestation_id: pgrx::Uuid, proof_data: Vec<u8>) -> pgrx::JsonB {
    // Fetch attestation data
    let att = Spi::get_one::<pgrx::JsonB>(&format!(
        "SELECT jsonb_build_object(
            'scope', scope::text,
            'claim_type', claim_type,
            'perspective_count', perspective_count,
            'avg_weight', avg_weight
        ) FROM kerai.attestations WHERE id = '{}'::uuid",
        attestation_id,
    ))
    .unwrap_or(None);

    let att = match att {
        Some(a) => a,
        None => error!("Attestation not found: {}", attestation_id),
    };

    let obj = att.0.as_object().unwrap();

    // Recompute commitment
    let mut hasher = Sha256::new();
    hasher.update(obj["scope"].as_str().unwrap_or("").as_bytes());
    hasher.update(obj["claim_type"].as_str().unwrap_or("").as_bytes());
    hasher.update(obj["perspective_count"].as_i64().unwrap_or(0).to_le_bytes());
    let avg_w = obj["avg_weight"].as_f64().unwrap_or(0.0);
    hasher.update(avg_w.to_le_bytes());
    let expected = hasher.finalize();

    let valid = proof_data.as_slice() == expected.as_slice();

    pgrx::JsonB(serde_json::json!({
        "attestation_id": attestation_id.to_string(),
        "valid": valid,
        "proof_type": "sha256_commitment",
    }))
}
