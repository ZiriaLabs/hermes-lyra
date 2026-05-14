//! Schema.org Depth-1 alignment for `jsonld::to_jsonld` / `from_jsonld`.
//!
//! Tests pin both the wire format (what bytes get emitted, what
//! external consumers see) and the parser behavior (what shapes are
//! accepted). The wire format is the contract — any change to
//! `to_jsonld`'s output must change one of these tests.

use lyra_ref::descriptor::{EffectKind, NamedField, Shape, SkillDescriptor};
use lyra_ref::jsonld::{from_jsonld, to_jsonld};
use lyra_ref::schema::LYRA_SKILL_SCHEMA_V1_CID;

fn rich_descriptor() -> SkillDescriptor {
    SkillDescriptor::builder()
        .name("rich-skill")
        .version("1.2.3")
        .content_hash_hex(&"ab".repeat(32))
        .input_shape(Shape::Structured {
            fields: vec![
                NamedField {
                    name: "query".into(),
                    shape: Shape::String { max_bytes: 256 },
                },
                NamedField {
                    name: "limit".into(),
                    shape: Shape::U32 { max_bytes: 4 },
                },
            ],
        })
        .output_shape(Shape::List {
            item: Box::new(Shape::String { max_bytes: 4096 }),
            max_items: 100,
        })
        .effect(EffectKind::WebRead)
        .effect(EffectKind::Llm)
        .reference("bafkr4idtjwypfi4rrrhkzciuvexfxehrv4zgvnylzxgr7lhuyimcfdmene")
        .build()
        .expect("test descriptor must build")
}

// ---------------------------------------------------------------
// Wire format: what bytes do we emit?
// ---------------------------------------------------------------

#[test]
fn emits_context_with_both_namespaces() {
    let out = to_jsonld(&rich_descriptor());
    assert!(
        out.contains(r#""@context""#),
        "must emit @context block: {out}"
    );
    assert!(
        out.contains(r#""schema": "https://schema.org/""#),
        "must declare schema: namespace: {out}"
    );
    assert!(
        out.contains(r#""lyra": "https://lyra-protocol.org/ontology/v0.1/""#),
        "must declare lyra: namespace: {out}"
    );
}

#[test]
fn emits_schema_org_software_application_type() {
    let out = to_jsonld(&rich_descriptor());
    assert!(
        out.contains(r#""@type": "schema:SoftwareApplication""#),
        "must render as schema:SoftwareApplication: {out}"
    );
}

#[test]
fn emits_schema_name_and_software_version() {
    let out = to_jsonld(&rich_descriptor());
    assert!(
        out.contains(r#""schema:name": "rich-skill""#),
        "name must be schema:name: {out}"
    );
    assert!(
        out.contains(r#""schema:softwareVersion": "1.2.3""#),
        "version must be schema:softwareVersion: {out}"
    );
}

#[test]
fn emits_lyra_namespaced_specific_fields() {
    let out = to_jsonld(&rich_descriptor());
    // Schema CID, content hash, shapes, effects, references all live
    // under lyra: because schema.org has no native vocabulary for them.
    assert!(out.contains(r#""lyra:schema""#), "must emit lyra:schema: {out}");
    assert!(
        out.contains(r#""lyra:contentHash""#),
        "must emit lyra:contentHash: {out}"
    );
    assert!(
        out.contains(r#""lyra:inputShape""#),
        "must emit lyra:inputShape: {out}"
    );
    assert!(
        out.contains(r#""lyra:outputShape""#),
        "must emit lyra:outputShape: {out}"
    );
    assert!(
        out.contains(r#""lyra:effects""#),
        "must emit lyra:effects: {out}"
    );
    assert!(
        out.contains(r#""lyra:references""#),
        "must emit lyra:references: {out}"
    );
}

#[test]
fn does_not_emit_legacy_bare_keys_at_top_level() {
    // The bare-key form (`"name"`, `"version"`, etc.) was the v0.2
    // wire format for *descriptor top-level fields*. After Depth-1
    // alignment, those move to the namespaced form.
    //
    // IMPORTANT: bare keys like `"name"` ALSO appear inside nested
    // structured-shape field declarations (`{"name": "query", "shape": ...}`).
    // Those are not descriptor top-level fields — they're shape grammar
    // internals — and they're intentionally not aliased. The Depth-1
    // alignment is at the descriptor boundary; below that the typed
    // shape vocabulary remains untouched.
    //
    // To check "no legacy top-level keys" precisely we extract the
    // top-level keys (those that appear right after `{` or after a
    // top-level `,`) and assert none of the legacy names are among
    // them. We do this by partial JSON walk rather than by string
    // search.
    let out = to_jsonld(&rich_descriptor());
    let top_keys = extract_top_level_keys(&out);
    for legacy in &[
        "name",
        "version",
        "content_hash",
        "input_shape",
        "output_shape",
        "effects",
        "references",
        "schema",
        "type",
    ] {
        assert!(
            !top_keys.iter().any(|k| k == legacy),
            "legacy bare key {legacy:?} must not appear at top level; saw keys {top_keys:?}"
        );
    }
}

/// Walk a single JSON object string and return its top-level keys
/// (depth-1 only). Used by the wire-format test to assert nothing
/// legacy is at the descriptor root.
fn extract_top_level_keys(s: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    // skip until first `{`
    while i < bytes.len() && bytes[i] != b'{' {
        i += 1;
    }
    if i >= bytes.len() {
        return keys;
    }
    i += 1;
    let mut depth = 1i32;
    let mut in_str = false;
    let mut esc = false;
    let mut current_key: Option<String> = None;
    let mut buf = String::new();
    let mut reading_key = false;
    while i < bytes.len() {
        let b = bytes[i];
        if in_str {
            if esc {
                esc = false;
                if reading_key { buf.push(b as char); }
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
                if reading_key {
                    current_key = Some(buf.clone());
                    buf.clear();
                    reading_key = false;
                }
            } else if reading_key {
                buf.push(b as char);
            }
        } else {
            match b {
                b'"' => {
                    in_str = true;
                    if depth == 1 && current_key.is_none() {
                        reading_key = true;
                        buf.clear();
                    }
                }
                b'{' | b'[' => depth += 1,
                b'}' | b']' => {
                    depth -= 1;
                    if depth == 0 { break; }
                }
                b':' if depth == 1 => {
                    if let Some(k) = current_key.take() {
                        keys.push(k);
                    }
                }
                b',' if depth == 1 => {
                    current_key = None;
                }
                _ => {}
            }
        }
        i += 1;
    }
    keys
}

#[test]
fn schema_cid_field_carries_v1_default() {
    // Every descriptor built without an explicit schema defaults to v1.
    // That default must surface on the wire as lyra:schema.
    let out = to_jsonld(&rich_descriptor());
    let expected = format!(r#""lyra:schema": "{LYRA_SKILL_SCHEMA_V1_CID}""#);
    assert!(
        out.contains(&expected),
        "must emit lyra:schema with v1 CID {LYRA_SKILL_SCHEMA_V1_CID}: {out}"
    );
}

// ---------------------------------------------------------------
// Output is valid JSON (and therefore valid JSON-LD)
// ---------------------------------------------------------------

#[test]
fn output_is_parseable_via_internal_parser() {
    // We don't bring in serde_json for tests — the existing
    // hand-rolled parser is the canonical "is this JSON" check
    // for this crate. If from_jsonld accepts to_jsonld's output,
    // it's at minimum a well-formed JSON object.
    let out = to_jsonld(&rich_descriptor());
    from_jsonld(&out).expect("to_jsonld output must be parseable by from_jsonld");
}

// ---------------------------------------------------------------
// Round-trip: value identity through serialize→parse
// ---------------------------------------------------------------

#[test]
fn round_trip_preserves_all_fields() {
    let original = rich_descriptor();
    let out = to_jsonld(&original);
    let parsed = from_jsonld(&out).expect("round-trip parse must succeed");

    assert_eq!(parsed.name(), original.name(), "name lost");
    assert_eq!(parsed.version(), original.version(), "version lost");
    assert_eq!(
        parsed.content_hash(),
        original.content_hash(),
        "content_hash lost"
    );
    assert_eq!(
        parsed.input_shape(),
        original.input_shape(),
        "input_shape lost"
    );
    assert_eq!(
        parsed.output_shape(),
        original.output_shape(),
        "output_shape lost"
    );
    assert_eq!(parsed.effects(), original.effects(), "effects lost");
    assert_eq!(
        parsed.references(),
        original.references(),
        "references lost"
    );
    assert_eq!(parsed.schema(), original.schema(), "schema CID lost");
}

#[test]
fn round_trip_idempotent() {
    // to(parse(to(d))) == to(d) — the wire format is a fixed point.
    let original = rich_descriptor();
    let once = to_jsonld(&original);
    let parsed = from_jsonld(&once).expect("first round-trip");
    let twice = to_jsonld(&parsed);
    assert_eq!(
        once, twice,
        "JSON-LD output must be idempotent under round-trip"
    );
}

// ---------------------------------------------------------------
// Parser tolerance: legacy bare-key form still parses
// ---------------------------------------------------------------

#[test]
fn parser_accepts_legacy_bare_key_form() {
    // A descriptor JSON-LD blob written by hand (or by a pre-Depth-1
    // tool) using bare keys must still parse. This is the input-side
    // backward-compat path; output is always the new namespaced form.
    let legacy = r#"{
        "name": "legacy-skill",
        "version": "0.1.0",
        "content_hash": "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20",
        "input_shape":  {"type": "string", "max_bytes": 16},
        "output_shape": {"type": "u8", "max_bytes": 1},
        "effects": ["none"],
        "references": []
    }"#;
    let parsed = from_jsonld(legacy).expect("legacy bare-key form must still parse");
    assert_eq!(parsed.name(), "legacy-skill");
    assert_eq!(parsed.version(), "0.1.0");
    // No explicit schema → builder defaults to v1.
    assert_eq!(parsed.schema(), LYRA_SKILL_SCHEMA_V1_CID);
}

#[test]
fn parser_prefers_namespaced_keys_when_both_present() {
    // If a (malicious or weird) blob includes BOTH `schema:name` and
    // bare `name`, the namespaced form wins — matching what we emit.
    let mixed = r#"{
        "@type": "schema:SoftwareApplication",
        "schema:name": "namespaced-wins",
        "name": "bare-loses",
        "schema:softwareVersion": "1.0.0",
        "version": "9.9.9",
        "lyra:contentHash": "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20",
        "content_hash": "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "lyra:inputShape":  {"type": "string", "max_bytes": 16},
        "lyra:outputShape": {"type": "u8", "max_bytes": 1},
        "lyra:effects": ["none"],
        "lyra:references": []
    }"#;
    let parsed = from_jsonld(mixed).expect("mixed-form must parse");
    assert_eq!(parsed.name(), "namespaced-wins");
    assert_eq!(parsed.version(), "1.0.0");
    assert_eq!(
        parsed.content_hash_hex(),
        "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20"
    );
}

// ---------------------------------------------------------------
// Adversarial / edge cases
// ---------------------------------------------------------------

#[test]
fn wrong_type_does_not_break_parsing() {
    // `@type` is informational on input. A blob claiming to be a
    // schema:Person but carrying valid skill data still parses.
    // This is the JSON-LD-edge-format stance: we don't enforce
    // schema.org typing in the parser; that would be Depth-3.
    let wrong_type = r#"{
        "@type": "schema:Person",
        "schema:name": "weird-skill",
        "schema:softwareVersion": "1.0.0",
        "lyra:contentHash": "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20",
        "lyra:inputShape":  {"type": "string", "max_bytes": 16},
        "lyra:outputShape": {"type": "u8", "max_bytes": 1},
        "lyra:effects": ["none"],
        "lyra:references": []
    }"#;
    let parsed = from_jsonld(wrong_type).expect("wrong @type must not block parsing");
    assert_eq!(parsed.name(), "weird-skill");
}

#[test]
fn missing_schema_name_is_a_typed_error() {
    let bad = r#"{
        "@type": "schema:SoftwareApplication",
        "schema:softwareVersion": "1.0.0",
        "lyra:contentHash": "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20",
        "lyra:inputShape":  {"type": "string", "max_bytes": 16},
        "lyra:outputShape": {"type": "u8", "max_bytes": 1},
        "lyra:effects": ["none"],
        "lyra:references": []
    }"#;
    let err = from_jsonld(bad).expect_err("missing name must error");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("name"),
        "error must mention the missing field: {msg}"
    );
}

#[test]
fn empty_effects_and_references_round_trip() {
    let desc = SkillDescriptor::builder()
        .name("pure")
        .version("1.0.0")
        .content_hash_hex(&"77".repeat(32))
        .input_shape(Shape::U8 { max_bytes: 1 })
        .output_shape(Shape::U8 { max_bytes: 1 })
        .effect(EffectKind::None)
        .build()
        .expect("pure descriptor must build");
    let out = to_jsonld(&desc);
    let parsed = from_jsonld(&out).expect("round-trip");
    assert_eq!(parsed.effects(), desc.effects());
    assert_eq!(parsed.references(), desc.references());
    assert!(out.contains(r#""lyra:effects": ["none"]"#));
    assert!(out.contains(r#""lyra:references": []"#));
}

#[test]
fn structured_input_shape_serializes_recursively() {
    // The nested `fields` array uses bare key names (`name`, `shape`)
    // because they're not top-level descriptor fields — they're shape
    // grammar internals. The Depth-1 alignment is at the descriptor
    // boundary; below that we keep our typed shape vocabulary.
    let out = to_jsonld(&rich_descriptor());
    assert!(
        out.contains(r#""type": "structured""#),
        "structured shape must emit type: structured: {out}"
    );
    assert!(
        out.contains(r#""name": "query""#),
        "field name `query` must round-trip: {out}"
    );
    assert!(
        out.contains(r#""name": "limit""#),
        "field name `limit` must round-trip: {out}"
    );
}

// ---------------------------------------------------------------
// External-consumer property: schema.org IRIs resolve in a vocabulary
// ---------------------------------------------------------------

#[test]
fn schema_org_iri_form_matches_canonical() {
    // We point at https://schema.org/ — the canonical schema.org
    // base. If anyone ever runs the output through a JSON-LD context
    // expander, `schema:name` → `https://schema.org/name`, which is
    // the actual canonical IRI for the name property in schema.org.
    let out = to_jsonld(&rich_descriptor());
    assert!(out.contains(r#""schema": "https://schema.org/""#));
    // This is what a JSON-LD expander would produce after context expansion:
    //   schema:name           -> https://schema.org/name
    //   schema:softwareVersion -> https://schema.org/softwareVersion
    // Both are real schema.org properties (verified against schema.org/v25).
}
