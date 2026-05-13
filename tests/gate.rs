//! Integration tests for the four protocol computations.
//!
//! Every score is followed by a verify in `assert_gated()` to confirm
//! the receipt round-trips.
//!
//! Categories:
//! - Boundary conditions (max_bytes / max_items at and over the 16 MiB cap)
//! - Malformed descriptors (missing field, invalid effect, invalid hash)
//! - Composability (exact, larger/smaller, type mismatch, structured
//!   subset / missing field, list compatible / incompatible items)
//! - Determinism (same input → same output_hash)

use lyra_ref::{score, verify, Receipt, VerifyOutcome};

// ---- helpers -----------------------------------------------------------

/// Score and re-verify in one step. Asserts the gate property: a
/// receipt produced by `score` is accepted by `verify` against the
/// same (computation, input). Returns the receipt's `output_hash` for
/// downstream comparisons (e.g. determinism tests).
fn assert_gated(computation: &str, input: &str) -> String {
    let receipt = score(computation, input)
        .unwrap_or_else(|e| panic!("score {computation} failed: {e}\ninput={input}"));
    let outcome = verify(computation, input, &receipt)
        .unwrap_or_else(|e| panic!("verify {computation} failed: {e}"));
    match outcome {
        VerifyOutcome::Ok { output_hash, .. } => output_hash,
        VerifyOutcome::ContentMismatch {
            expected,
            actual_in_receipt,
        } => panic!("content mismatch on fresh receipt: expected={expected} actual={actual_in_receipt}"),
    }
}

/// Assert that score rejects an input.
fn assert_rejected(computation: &str, input: &str) {
    let res = score(computation, input);
    assert!(
        res.is_err(),
        "expected {computation} to reject input but it succeeded.\ninput={input}",
    );
}

// Build a minimal valid Lyra interface descriptor JSON. Inline so each
// test is self-contained (the gate is JSON-shaped today; once Phase 2
// turns it into typed Rust structs, this helper becomes a struct ctor).
fn descriptor(
    name: &str,
    content_hash: &str,
    input_shape: &str,
    output_shape: &str,
    effects: &str,
    references: &str,
) -> String {
    // Canonical alphabetical key order: content_hash, effects, input_shape, name, output_shape, references, version.
    format!(
        r#"{{"content_hash":"{ch}","effects":{eff},"input_shape":{is},"name":"{n}","output_shape":{os},"references":{refs},"version":"1.0.0"}}"#,
        n = name,
        ch = content_hash,
        is = input_shape,
        os = output_shape,
        eff = effects,
        refs = references,
    )
}

fn compose_input(producer_out: &str, consumer_in: &str) -> String {
    // `compose_interfaces` now routes through the typed builder (audit #2:
    // single source of truth). The builder requires full descriptors, so
    // we wrap each shape in a complete skeleton. The producer's
    // input_shape and the consumer's output_shape are uniform u8 stubs —
    // they don't affect the composition check, which only inspects
    // producer.output_shape and consumer.input_shape.
    let producer = format!(
        r#"{{"content_hash":"{c}","effects":["none"],"input_shape":{{"type":"u8","max_bytes":1}},"name":"prod","output_shape":{out},"references":[],"version":"1.0.0"}}"#,
        c = "a".repeat(64),
        out = producer_out,
    );
    let consumer = format!(
        r#"{{"content_hash":"{c}","effects":["none"],"input_shape":{inp},"name":"cons","output_shape":{{"type":"u8","max_bytes":1}},"references":[],"version":"1.0.0"}}"#,
        c = "b".repeat(64),
        inp = consumer_in,
    );
    format!(r#"{{"producer":{producer},"consumer":{consumer}}}"#)
}

// blake3("COMPATIBLE") and blake3("INCOMPATIBLE:...") pre-images. The
// `compose_interfaces` computation hashes a literal status string, so
// the receipt's output_hash distinguishes the two outcomes.
// BLAKE3-256 of (LYRA_RUNTIME_IDENT || 0x00 || "compose_interfaces" || 0x00 || b"COMPATIBLE").
// Folding the runtime ident makes the seal substrate-bound; this constant
// must change whenever LYRA_RUNTIME_IDENT does.
const COMPATIBLE_HASH: &str = "9175ef568d458732eec490b34786bc01d7651f4518218260c0e73738e7a05649";

fn assert_compatible(producer_out: &str, consumer_in: &str) {
    let h = assert_gated("compose_interfaces", &compose_input(producer_out, consumer_in));
    assert_eq!(
        h, COMPATIBLE_HASH,
        "expected COMPATIBLE for producer={producer_out} consumer={consumer_in}, got {h}",
    );
}

fn assert_incompatible(producer_out: &str, consumer_in: &str) {
    let h = assert_gated("compose_interfaces", &compose_input(producer_out, consumer_in));
    assert_ne!(
        h, COMPATIBLE_HASH,
        "expected INCOMPATIBLE for producer={producer_out} consumer={consumer_in}, got COMPATIBLE",
    );
}

// ---- boundary tests ----------------------------------------------------

#[test]
fn boundary_max_bytes_at_limit() {
    let desc = descriptor(
        "limit-test",
        &"a".repeat(64),
        r#"{"type":"string","max_bytes":16777216}"#,
        r#"{"type":"u8","max_bytes":1}"#,
        r#"["none"]"#,
        r#"[]"#,
    );
    assert_gated("skill_interface_hash", &desc);
}

#[test]
fn boundary_max_bytes_over_limit_rejected() {
    let desc = descriptor(
        "over-limit",
        &"a".repeat(64),
        r#"{"type":"string","max_bytes":16777217}"#,
        r#"{"type":"u8","max_bytes":1}"#,
        r#"["none"]"#,
        r#"[]"#,
    );
    assert_rejected("skill_interface_hash", &desc);
}

#[test]
fn boundary_max_items_at_limit() {
    let desc = descriptor(
        "list-limit",
        &"a".repeat(64),
        r#"{"type":"list","item":{"type":"u8","max_bytes":1},"max_items":16777216}"#,
        r#"{"type":"u8","max_bytes":1}"#,
        r#"["none"]"#,
        r#"[]"#,
    );
    assert_gated("skill_interface_hash", &desc);
}

// ---- malformed descriptor tests ----------------------------------------

#[test]
fn malformed_missing_field_rejected() {
    // Missing input_shape, output_shape, effects, references, content_hash.
    let bad = r#"{"name":"x","version":"1.0.0"}"#;
    assert_rejected("skill_interface_hash", bad);
}

#[test]
fn malformed_invalid_effect_rejected() {
    let desc = descriptor(
        "bad-effect",
        &"a".repeat(64),
        r#"{"type":"u8","max_bytes":1}"#,
        r#"{"type":"u8","max_bytes":1}"#,
        r#"["teleport"]"#,
        r#"[]"#,
    );
    assert_rejected("skill_interface_hash", &desc);
}

#[test]
fn malformed_invalid_content_hash_rejected() {
    let desc = descriptor(
        "bad-hash",
        "not-hex!",
        r#"{"type":"u8","max_bytes":1}"#,
        r#"{"type":"u8","max_bytes":1}"#,
        r#"["none"]"#,
        r#"[]"#,
    );
    assert_rejected("skill_interface_hash", &desc);
}

// ---- composability tests -----------------------------------------------

#[test]
fn compose_exact_match() {
    assert_compatible(
        r#"{"type":"string","max_bytes":256}"#,
        r#"{"type":"string","max_bytes":256}"#,
    );
}

// Composition direction is Liskov: producer.max_bytes <= consumer.max_bytes.
// A producer that outputs at most 128 bytes safely feeds a consumer
// accepting up to 256. A producer that might output 1024 bytes
// overflows a consumer accepting only 256.

#[test]
fn compose_producer_smaller_capacity_ok() {
    assert_compatible(
        r#"{"type":"string","max_bytes":128}"#,
        r#"{"type":"string","max_bytes":256}"#,
    );
}

#[test]
fn compose_producer_larger_capacity_rejected() {
    assert_incompatible(
        r#"{"type":"string","max_bytes":1024}"#,
        r#"{"type":"string","max_bytes":256}"#,
    );
}

#[test]
fn compose_type_mismatch_rejected() {
    // String → u32 is a type mismatch regardless of capacity. Use
    // legitimate max_bytes (u32::SITE_COUNT = 4) so the typed builder
    // accepts both descriptors and the composition gate is the layer
    // that rejects.
    assert_incompatible(
        r#"{"type":"string","max_bytes":256}"#,
        r#"{"type":"u32","max_bytes":4}"#,
    );
}

#[test]
fn compose_structured_subset() {
    // Producer has both fields; consumer needs only one. Compatible.
    assert_compatible(
        r#"{"type":"structured","fields":[{"name":"x","shape":{"type":"string","max_bytes":256}},{"name":"y","shape":{"type":"u8","max_bytes":1}}]}"#,
        r#"{"type":"structured","fields":[{"name":"x","shape":{"type":"string","max_bytes":256}}]}"#,
    );
}

#[test]
fn compose_structured_missing_field_rejected() {
    // Consumer needs both fields; producer has only one. Incompatible.
    assert_incompatible(
        r#"{"type":"structured","fields":[{"name":"x","shape":{"type":"string","max_bytes":256}}]}"#,
        r#"{"type":"structured","fields":[{"name":"x","shape":{"type":"string","max_bytes":256}},{"name":"y","shape":{"type":"u8","max_bytes":1}}]}"#,
    );
}

#[test]
fn compose_list_compatible() {
    // Liskov direction for list capacity: producer.max_items <= consumer.max_items.
    // Producer might emit up to 50 items; consumer can absorb up to 100.
    assert_compatible(
        r#"{"type":"list","item":{"type":"string","max_bytes":256},"max_items":50}"#,
        r#"{"type":"list","item":{"type":"string","max_bytes":256},"max_items":100}"#,
    );
}

#[test]
fn compose_list_incompatible_items_rejected() {
    assert_incompatible(
        r#"{"type":"list","item":{"type":"string","max_bytes":256},"max_items":100}"#,
        r#"{"type":"list","item":{"type":"u32","max_bytes":4},"max_items":50}"#,
    );
}

// ---- determinism tests -------------------------------------------------

#[test]
fn determinism_interface_hash() {
    let desc = descriptor(
        "det-test",
        &"a".repeat(64),
        r#"{"type":"u8","max_bytes":1}"#,
        r#"{"type":"u8","max_bytes":1}"#,
        r#"["none"]"#,
        r#"[]"#,
    );
    let h1 = assert_gated("skill_interface_hash", &desc);
    let h2 = assert_gated("skill_interface_hash", &desc);
    assert_eq!(h1, h2, "interface hash must be deterministic");
}

#[test]
fn determinism_compose() {
    let p = r#"{"type":"string","max_bytes":256}"#;
    let c = r#"{"type":"string","max_bytes":256}"#;
    let h1 = assert_gated("compose_interfaces", &compose_input(p, c));
    let h2 = assert_gated("compose_interfaces", &compose_input(p, c));
    assert_eq!(h1, h2, "compose_interfaces must be deterministic");
}

// ---- gate-property tests (these are the ones that did not exist) -------

#[test]
fn gate_property_every_score_passes_verify() {
    // The whole point of the gate. Pre-2026-05 audit, the Python shim
    // never called verify, masking a receipt-parser bug that broke
    // verify on every JSON-input computation. Pin the property here.
    let desc = descriptor(
        "gate-property",
        &"a".repeat(64),
        r#"{"type":"string","max_bytes":256}"#,
        r#"{"type":"string","max_bytes":256}"#,
        r#"["none"]"#,
        r#"[]"#,
    );
    let receipt = score("skill_interface_hash", &desc).expect("score");

    // The receipt MUST round-trip through verify.
    let outcome = verify("skill_interface_hash", &desc, &receipt).expect("verify");
    assert!(matches!(outcome, VerifyOutcome::Ok { .. }), "expected Ok, got {outcome:?}");
}

#[test]
fn gate_property_tampered_receipt_rejected() {
    let desc = descriptor(
        "tamper-test",
        &"a".repeat(64),
        r#"{"type":"string","max_bytes":256}"#,
        r#"{"type":"string","max_bytes":256}"#,
        r#"["none"]"#,
        r#"[]"#,
    );
    let receipt = score("skill_interface_hash", &desc).expect("score");

    // Forge a different output_hash. Structural validation will
    // pass (the trace is intact), but content re-execution will not
    // match.
    let mut tampered = receipt.clone();
    let mut bytes: Vec<u8> = tampered.output_hash.into_bytes();
    bytes[0] ^= 0x01;
    tampered.output_hash = String::from_utf8(bytes).unwrap();

    let outcome = verify("skill_interface_hash", &desc, &tampered).expect("verify call ok");
    match outcome {
        VerifyOutcome::ContentMismatch { .. } => {} // expected
        VerifyOutcome::Ok { .. } => panic!("tampered receipt accepted"),
    }
}

#[test]
fn gate_property_wrong_input_rejected() {
    let desc1 = descriptor(
        "wrong-input-a",
        &"a".repeat(64),
        r#"{"type":"u8","max_bytes":1}"#,
        r#"{"type":"u8","max_bytes":1}"#,
        r#"["none"]"#,
        r#"[]"#,
    );
    let desc2 = descriptor(
        "wrong-input-b",
        &"b".repeat(64),
        r#"{"type":"u8","max_bytes":1}"#,
        r#"{"type":"u8","max_bytes":1}"#,
        r#"["none"]"#,
        r#"[]"#,
    );
    let receipt = score("skill_interface_hash", &desc1).expect("score");

    // Verify with a different input must error out (input mismatch).
    let res = verify("skill_interface_hash", &desc2, &receipt);
    assert!(res.is_err(), "verify must reject mismatched input");
}

// ---- Merkle manifest -- exercised here because it was not in the Python suite ----

#[test]
fn merkle_manifest_roundtrips() {
    let manifest = r#"[{"path":"skills/web-search","content_hash":"10b5b447"},{"path":"skills/http-client","content_hash":"aabbccdd"}]"#;
    assert_gated("merkle_manifest", manifest);
}

#[test]
fn merkle_manifest_order_independent() {
    // Per the spec the manifest is sorted by path before hashing, so
    // the same set in different order produces the same root.
    let m1 = r#"[{"path":"skills/web-search","content_hash":"10b5b447"},{"path":"skills/http-client","content_hash":"aabbccdd"}]"#;
    let m2 = r#"[{"path":"skills/http-client","content_hash":"aabbccdd"},{"path":"skills/web-search","content_hash":"10b5b447"}]"#;
    let h1 = assert_gated("merkle_manifest", m1);
    let h2 = assert_gated("merkle_manifest", m2);
    assert_eq!(h1, h2, "merkle root must be path-order independent");
}

// ---- skill_reference_resolve ------------------------------------------

#[test]
fn skill_reference_resolve_succeeds_when_present() {
    // S4: references are pinned `name@<64-hex>`. Both name and content_hash
    // must match a manifest entry.
    let http_hash = "aa".repeat(32);
    let input = format!(
        r#"{{"manifest":[{{"content_hash":"{http_hash}","name":"http-client"}}],"skill":{{"content_hash":"10b5b447","effects":["web_read"],"input_shape":{{"type":"string","max_bytes":256}},"output_shape":{{"type":"string","max_bytes":4096}},"references":["http-client@{http_hash}"],"name":"web-search","version":"1.0.0"}}}}"#
    );
    assert_gated("skill_reference_resolve", &input);
}

#[test]
fn skill_reference_resolve_rejects_missing_reference() {
    // Reference to a skill not in the manifest → must error.
    let missing_hash = "ff".repeat(32);
    let input = format!(
        r#"{{"manifest":[],"skill":{{"content_hash":"10b5b447","effects":["web_read"],"input_shape":{{"type":"string","max_bytes":256}},"output_shape":{{"type":"string","max_bytes":4096}},"references":["nonexistent-skill@{missing_hash}"],"name":"web-search","version":"1.0.0"}}}}"#
    );
    assert_rejected("skill_reference_resolve", &input);
}

// Make sure Receipt is exported. Compile-level check.
#[allow(dead_code)]
fn _exports() {
    fn _r(_: &Receipt) {}
}
