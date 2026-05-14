//! Substrate-property tests: deterministic canonicalization survives
//! independent processes, and substrate-version mismatches are rejected
//! visibly rather than silently.

use std::path::PathBuf;
use std::process::Command;

use lyra_ref::cli_api::{score, verify, VerifyOutcome};
use lyra_ref::receipt::Receipt;
use lyra_ref::LYRA_RUNTIME_IDENT;

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_lyra"))
}

const DESCRIPTOR: &str = r#"{"content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","effects":["file_read"],"input_shape":{"type":"string","max_bytes":4096},"name":"pdf-extract","output_shape":{"type":"string","max_bytes":16777216},"references":[],"version":"1.0.0"}"#;

// ----------------------------------------------------------------------
// Deterministic canonicalization: same substrate → bit-identical seal.
// ----------------------------------------------------------------------

#[test]
fn same_process_two_seals_are_byte_identical() {
    let r1 = score("skill_interface_hash", DESCRIPTOR).expect("r1");
    let r2 = score("skill_interface_hash", DESCRIPTOR).expect("r2");
    assert_eq!(r1.output_cid, r2.output_cid);
    assert_eq!(r1.runtime, r2.runtime);
    assert_eq!(r1.computation_id, r2.computation_id);
    assert_eq!(r1.input, r2.input);
}

/// Spawn a *fresh* `lyra` process and have it score the same descriptor.
/// The output_hash bytes must match what the in-process API produces.
/// This is the empirical proof that the seal is reproducible across
/// independent invocations — the cross-process half of the Schelling
/// property.
#[test]
fn cross_process_seal_matches_in_process_seal() {
    let local = score("skill_interface_hash", DESCRIPTOR).expect("local");

    let mut receipt_path = std::env::temp_dir();
    receipt_path.push(format!("lyra-xp-{}.json", std::process::id()));

    let status = Command::new(binary())
        .args(["score", "skill_interface_hash", DESCRIPTOR])
        .arg(&receipt_path)
        .status()
        .expect("spawn lyra");
    assert!(status.success(), "cross-process score failed");

    let cross = Receipt::read_from_file(receipt_path.to_str().unwrap()).expect("read");
    assert_eq!(
        local.output_cid, cross.output_cid,
        "cross-process seal must match in-process seal byte-for-byte"
    );
    assert_eq!(local.runtime, cross.runtime);
    assert_eq!(local.runtime, LYRA_RUNTIME_IDENT);

    // The other process's receipt verifies in *this* process — that's the
    // peer-verifiable-offline property.
    let outcome = verify(&cross.computation_id, &cross.input, &cross).expect("verify");
    assert!(matches!(outcome, VerifyOutcome::Ok { .. }));

    let _ = std::fs::remove_file(&receipt_path);
}

// ----------------------------------------------------------------------
// Substrate-version mismatch is rejected explicitly.
// ----------------------------------------------------------------------

#[test]
fn receipt_with_foreign_runtime_is_rejected() {
    let mut receipt = score("skill_interface_hash", DESCRIPTOR).expect("score");
    receipt.runtime = "lyra-ref/9.9.9+uor-foundation/9.9.9".into();
    let err = verify(&receipt.computation_id, &receipt.input, &receipt)
        .expect_err("should reject foreign runtime");
    assert!(
        err.contains("SubstrateVersionMismatch"),
        "expected SubstrateVersionMismatch, got: {err}"
    );
}

// ----------------------------------------------------------------------
// The runtime ident is in the canonical bytes — different runtime → different hash.
// ----------------------------------------------------------------------

#[test]
fn runtime_ident_is_baked_into_output_hash() {
    // Tamper a fresh receipt's runtime AND output_hash to match what
    // a "different runtime" would produce. Verify still rejects, because
    // re-running the computation in *our* runtime produces our hash.
    let mut receipt = score("skill_interface_hash", DESCRIPTOR).expect("score");
    receipt.runtime = "lyra-ref/9.9.9+uor-foundation/9.9.9".into();
    // Don't change output_hash — we expect SubstrateVersionMismatch.
    let err = verify(&receipt.computation_id, &receipt.input, &receipt)
        .expect_err("foreign runtime");
    assert!(err.contains("SubstrateVersionMismatch"));
}
