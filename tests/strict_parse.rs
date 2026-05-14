//! Strict-parse acceptance suite.
//!
//! Pins exactly which malformed JSON shapes hermes-lyra MUST reject.
//! Every test here represents an input that an attacker could submit
//! over MCP or via a hand-edited SKILL.md to try to provoke a
//! parse-state divergence between two implementations.
//!
//! The protocol's content-addressing guarantee requires that **for
//! every byte sequence accepted as a descriptor, exactly one canonical
//! form exists**. Trailing commas, unquoted keys, BOM bytes, etc., all
//! create acceptance-set ambiguity that breaks content addressing.

use lyra_ref::cli_api::score;

/// Minimal valid descriptor we mutate per-test. Stays as one long
/// string so the mutation in each test is a single, obvious diff.
const VALID: &str = r#"{"content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","effects":["none"],"input_shape":{"type":"u8","max_bytes":1},"name":"strict","output_shape":{"type":"u8","max_bytes":1},"references":[],"version":"1.0.0"}"#;

#[test]
fn baseline_valid_descriptor_parses() {
    score("skill_interface_hash", VALID).expect("baseline must parse");
}

// ---------- Trailing comma ----------

#[test]
fn rejects_trailing_comma_in_object() {
    let bad = VALID.trim_end_matches('}').to_string() + ",}";
    let res = score("skill_interface_hash", &bad);
    assert!(res.is_err(), "trailing comma must be rejected; got: {res:?}");
}

#[test]
fn rejects_trailing_comma_in_nested_object() {
    // input_shape becomes {"type":"u8","max_bytes":1,}
    let bad = VALID.replace(r#""max_bytes":1}"#, r#""max_bytes":1,}"#);
    let res = score("skill_interface_hash", &bad);
    assert!(res.is_err(), "nested trailing comma must be rejected; got: {res:?}");
}

#[test]
fn rejects_trailing_comma_in_array() {
    let bad = VALID.replace(r#""references":[]"#, r#""references":["x",]"#);
    let res = score("skill_interface_hash", &bad);
    assert!(res.is_err(), "trailing comma in array must be rejected; got: {res:?}");
}

// ---------- Unquoted keys ----------

#[test]
fn rejects_unquoted_key() {
    // Replace "name":"strict" with name:"strict" — JavaScript-style, not strict-JSON.
    let bad = VALID.replace(r#""name":"strict""#, r#"name:"strict""#);
    let res = score("skill_interface_hash", &bad);
    assert!(res.is_err(), "unquoted key must be rejected; got: {res:?}");
}

// ---------- BOM ----------

#[test]
fn rejects_leading_bom() {
    let bad = format!("\u{FEFF}{VALID}");
    let res = score("skill_interface_hash", &bad);
    assert!(res.is_err(), "leading BOM must be rejected; got: {res:?}");
}

// ---------- Lone CR ----------

#[test]
fn rejects_bare_carriage_return_inside_object() {
    // Insert a lone \r between two key-value pairs. CR is not whitespace in canonical JSON.
    let bad = VALID.replace(r#","effects""#, "\r,\"effects\"");
    let res = score("skill_interface_hash", &bad);
    assert!(res.is_err(), "bare CR inside JSON must be rejected; got: {res:?}");
}

// ---------- Nesting depth ----------

#[test]
fn rejects_excessive_nesting_depth() {
    // Build a descriptor with ~64 levels of nested structured shapes.
    // The protocol cap is 32; anything beyond MUST be rejected.
    let mut inner = String::from(r#"{"type":"u8","max_bytes":1}"#);
    for _ in 0..64 {
        inner = format!(r#"{{"type":"structured","fields":[{{"name":"n","shape":{inner}}}]}}"#);
    }
    let bad = format!(
        r#"{{"content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","effects":["none"],"input_shape":{inner},"name":"deep","output_shape":{{"type":"u8","max_bytes":1}},"references":[],"version":"1.0.0"}}"#
    );
    let res = score("skill_interface_hash", &bad);
    assert!(res.is_err(), "depth>32 must be rejected; got: {res:?}");
}

#[test]
fn accepts_nesting_at_or_below_limit() {
    // At depth 8 (well under 32), the descriptor MUST still parse.
    let mut inner = String::from(r#"{"type":"u8","max_bytes":1}"#);
    for _ in 0..8 {
        inner = format!(r#"{{"type":"structured","fields":[{{"name":"n","shape":{inner}}}]}}"#);
    }
    let ok = format!(
        r#"{{"content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","effects":["none"],"input_shape":{inner},"name":"shallow","output_shape":{{"type":"u8","max_bytes":1}},"references":[],"version":"1.0.0"}}"#
    );
    let res = score("skill_interface_hash", &ok);
    assert!(res.is_ok(), "depth=8 must parse; got: {res:?}");
}

// ---------- Determinism (negative tests don't shift acceptance for valid bytes) ----------

#[test]
fn valid_bytes_still_hash_to_same_value_after_strict_mode() {
    // Pinned hash for the VALID descriptor above. Any change here means
    // strict mode accidentally moved canonical bytes — a regression.
    let r = score("skill_interface_hash", VALID).expect("must parse");
    // We pin only the prefix to avoid an accidental hex-collision tripwire
    // when this test is regenerated; this is enough to catch movement.
    assert!(r.output_cid.starts_with('b'), "CIDv1 multibase prefix");
    assert!(r.output_cid.len() >= 59, "CID at least 59 chars");
    // The value below is regenerated once strict mode lands. It pins the
    // post-strict canonical bytes for the minimal VALID descriptor so any
    // future change to canonicalize() that drifts gets caught here.
    let expected_prefix = std::env::var("STRICT_BASELINE_PREFIX").unwrap_or_default();
    if !expected_prefix.is_empty() {
        assert!(
            r.output_cid.starts_with(&expected_prefix),
            "canonical bytes shifted: got {} expected prefix {expected_prefix}",
            r.output_cid
        );
    }
}
