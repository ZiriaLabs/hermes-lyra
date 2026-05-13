//! Lineage receipt tests: `next_generation` must mint a receipt iff
//! the child descriptor is a **Liskov-substitutable** refinement of the
//! parent.

use lyra_ref::cli_api::{base64_encode, score, verify, VerifyOutcome};
use lyra_ref::receipt::Receipt;
use lyra_ref::{next_generation_check, NextGenerationError, RefinementError};

// ---- helpers -----------------------------------------------------------

/// 64-char content_hash from a one-byte hex.
fn ch(prefix: u8) -> String {
    format!("{:02x}", prefix).repeat(32)
}

/// Format a descriptor in canonical alphabetical key order.
fn descriptor_json(
    name: &str,
    version: &str,
    content_hash: &str,
    input_shape: &str,
    output_shape: &str,
    effects: &[&str],
    references: &[&str],
) -> String {
    let eff = if effects.is_empty() {
        "[]".to_string()
    } else {
        format!("[{}]", effects.iter().map(|e| format!("\"{e}\"")).collect::<Vec<_>>().join(","))
    };
    let refs = if references.is_empty() {
        "[]".to_string()
    } else {
        format!("[{}]", references.iter().map(|r| format!("\"{r}\"")).collect::<Vec<_>>().join(","))
    };
    format!(
        r#"{{"content_hash":"{content_hash}","effects":{eff},"input_shape":{input_shape},"name":"{name}","output_shape":{output_shape},"references":{refs},"version":"{version}"}}"#
    )
}

fn score_root(descriptor_json: &str) -> Receipt {
    score("skill_interface_hash", descriptor_json).expect("score skill_interface_hash")
}

/// Build the `next_generation` input from a parent receipt + child JSON.
fn lineage_input(parent: &Receipt, child_json: &str) -> String {
    let parent_b64 = base64_encode(parent.to_json().as_bytes());
    format!(r#"{{"child_descriptor":{child_json},"parent_receipt":"{parent_b64}"}}"#)
}

/// String-erased path (via score() / CLI dispatcher).
fn score_next_gen(parent: &Receipt, child_json: &str) -> Result<Receipt, String> {
    score("next_generation", &lineage_input(parent, child_json))
}

/// Typed path: panics on success, returns the typed error on failure.
fn typed_next_gen_err(parent: &Receipt, child_json: &str) -> NextGenerationError {
    next_generation_check(&lineage_input(parent, child_json))
        .err()
        .expect("expected NextGenerationError")
}

fn assert_verifies(receipt: &Receipt) {
    let outcome = verify(&receipt.computation_id, &receipt.input, receipt).expect("verify");
    assert!(
        matches!(outcome, VerifyOutcome::Ok { .. }),
        "expected Ok, got {:?}",
        outcome
    );
}

// ---- the canonical "pdf-extract" lineage used across tests --------------

fn pdf_v1() -> String {
    descriptor_json(
        "pdf-extract",
        "1.0.0",
        &ch(0xa1),
        r#"{"type":"string","max_bytes":4096}"#,
        r#"{"type":"string","max_bytes":16777216}"#,
        &["file_read"],
        &[],
    )
}

fn pdf_v1_1() -> String {
    descriptor_json(
        "pdf-extract",
        "1.1.0",
        &ch(0xa2),
        r#"{"type":"string","max_bytes":8192}"#,        // widened input
        r#"{"type":"string","max_bytes":4096}"#,        // narrowed output
        &["file_read"],
        &[],
    )
}

fn pdf_v2() -> String {
    descriptor_json(
        "pdf-extract",
        "2.0.0",
        &ch(0xa3),
        r#"{"type":"string","max_bytes":16384}"#,
        r#"{"type":"string","max_bytes":2048}"#,
        &["file_read"],
        &[],
    )
}

// ---- T1 ----------------------------------------------------------------

#[test]
fn t1_happy_path_three_generations() {
    let r1 = score_root(&pdf_v1());
    assert_verifies(&r1);

    let r2 = score_next_gen(&r1, &pdf_v1_1()).expect("r2");
    assert_verifies(&r2);

    let r3 = score_next_gen(&r2, &pdf_v2()).expect("r3");
    assert_verifies(&r3);
}

// ---- T2 ----------------------------------------------------------------

#[test]
fn t2_tampered_middle_generation_rejected() {
    let r1 = score_root(&pdf_v1());
    let r2 = score_next_gen(&r1, &pdf_v1_1()).expect("r2");

    let mut tampered = r2.clone();
    let mut bytes = tampered.output_hash.into_bytes();
    bytes[0] = if bytes[0] == b'0' { b'1' } else { b'0' };
    tampered.output_hash = String::from_utf8(bytes).unwrap();

    let err = typed_next_gen_err(&tampered, &pdf_v2());
    assert!(
        matches!(err, NextGenerationError::ParentReceiptInvalid(_)),
        "expected ParentReceiptInvalid, got {err}"
    );
}

// ---- T3 ----------------------------------------------------------------

#[test]
fn t3_incompatible_input_narrowing_rejected() {
    let child = descriptor_json(
        "pdf-extract",
        "1.0.1",
        &ch(0xa2),
        r#"{"type":"string","max_bytes":2048}"#,        // narrower input
        r#"{"type":"string","max_bytes":16777216}"#,
        &["file_read"],
        &[],
    );
    let r1 = score_root(&pdf_v1());
    let err = typed_next_gen_err(&r1, &child);
    assert!(
        matches!(err, NextGenerationError::NotARefinement(RefinementError::InputNarrowed)),
        "got {err}"
    );
}

// ---- T4 ----------------------------------------------------------------

#[test]
fn t4_incompatible_output_widening_rejected() {
    let smaller_parent = descriptor_json(
        "pdf-extract",
        "1.0.0",
        &ch(0xa1),
        r#"{"type":"string","max_bytes":4096}"#,
        r#"{"type":"string","max_bytes":1024}"#,
        &["file_read"],
        &[],
    );
    let widened_child = descriptor_json(
        "pdf-extract",
        "1.0.1",
        &ch(0xa2),
        r#"{"type":"string","max_bytes":4096}"#,
        r#"{"type":"string","max_bytes":4096}"#,        // widened output
        &["file_read"],
        &[],
    );
    let r1 = score_root(&smaller_parent);
    let err = typed_next_gen_err(&r1, &widened_child);
    assert!(
        matches!(err, NextGenerationError::NotARefinement(RefinementError::OutputWidened)),
        "got {err}"
    );
}

// ---- T5 ----------------------------------------------------------------

#[test]
fn t5_new_effect_rejected() {
    let child = descriptor_json(
        "pdf-extract",
        "1.0.1",
        &ch(0xa2),
        r#"{"type":"string","max_bytes":4096}"#,
        r#"{"type":"string","max_bytes":16777216}"#,
        &["file_read", "llm"],                     // new effect
        &[],
    );
    let r1 = score_root(&pdf_v1());
    let err = typed_next_gen_err(&r1, &child);
    assert!(
        matches!(err, NextGenerationError::NotARefinement(RefinementError::EffectAdded)),
        "got {err}"
    );
}

// ---- T6 ----------------------------------------------------------------

#[test]
fn t6_version_not_increased_rejected() {
    let child = descriptor_json(
        "pdf-extract",
        "1.0.0",                                        // same version
        &ch(0xa2),
        r#"{"type":"string","max_bytes":4096}"#,
        r#"{"type":"string","max_bytes":16777216}"#,
        &["file_read"],
        &[],
    );
    let r1 = score_root(&pdf_v1());
    let err = typed_next_gen_err(&r1, &child);
    assert!(
        matches!(err, NextGenerationError::NotARefinement(RefinementError::VersionNotIncreased)),
        "got {err}"
    );
}

// ---- T7 ----------------------------------------------------------------

#[test]
fn t7_name_change_rejected() {
    let child = descriptor_json(
        "pdf-extract-v2",                               // different name
        "1.0.1",
        &ch(0xa2),
        r#"{"type":"string","max_bytes":4096}"#,
        r#"{"type":"string","max_bytes":16777216}"#,
        &["file_read"],
        &[],
    );
    let r1 = score_root(&pdf_v1());
    let err = typed_next_gen_err(&r1, &child);
    assert!(
        matches!(err, NextGenerationError::NotARefinement(RefinementError::NameChanged)),
        "got {err}"
    );
}

// ---- T8: Liskov-correct — child input REMOVES a field is OK ------------

#[test]
fn t8_structured_field_dropped_from_input_ok() {
    let parent = descriptor_json(
        "csv-analyze",
        "1.0.0",
        &ch(0xb1),
        // Parent requires {path, encoding}.
        r#"{"type":"structured","fields":[{"name":"encoding","shape":{"type":"string","max_bytes":32}},{"name":"path","shape":{"type":"string","max_bytes":4096}}]}"#,
        r#"{"type":"u32","max_bytes":4}"#,
        &["file_read"],
        &[],
    );
    let child = descriptor_json(
        "csv-analyze",
        "1.1.0",
        &ch(0xb2),
        // Child requires just {path}. Caller's {path, encoding} still satisfies.
        r#"{"type":"structured","fields":[{"name":"path","shape":{"type":"string","max_bytes":4096}}]}"#,
        r#"{"type":"u32","max_bytes":4}"#,
        &["file_read"],
        &[],
    );
    let r1 = score_root(&parent);
    let r2 = score_next_gen(&r1, &child).expect("dropping required input field is a refinement");
    assert_verifies(&r2);
}

// ---- T8b: Liskov-correct — child input ADDS a field is REJECTED --------

#[test]
fn t8b_structured_field_added_to_input_rejected() {
    let parent = descriptor_json(
        "csv-analyze",
        "1.0.0",
        &ch(0xb1),
        r#"{"type":"structured","fields":[{"name":"path","shape":{"type":"string","max_bytes":4096}}]}"#,
        r#"{"type":"u32","max_bytes":4}"#,
        &["file_read"],
        &[],
    );
    let child = descriptor_json(
        "csv-analyze",
        "1.1.0",
        &ch(0xb2),
        // Child requires a NEW field; caller's parent-typed value cannot provide it.
        r#"{"type":"structured","fields":[{"name":"encoding","shape":{"type":"string","max_bytes":32}},{"name":"path","shape":{"type":"string","max_bytes":4096}}]}"#,
        r#"{"type":"u32","max_bytes":4}"#,
        &["file_read"],
        &[],
    );
    let r1 = score_root(&parent);
    let err = typed_next_gen_err(&r1, &child);
    assert!(
        matches!(err, NextGenerationError::NotARefinement(RefinementError::InputNarrowed)),
        "got {err}"
    );
}

// ---- T9: Liskov-correct — child output ADDS a field is OK --------------

#[test]
fn t9_structured_field_added_to_output_ok() {
    let parent = descriptor_json(
        "csv-analyze",
        "1.0.0",
        &ch(0xc1),
        r#"{"type":"string","max_bytes":4096}"#,
        // Parent promises {row_count}.
        r#"{"type":"structured","fields":[{"name":"row_count","shape":{"type":"u64","max_bytes":8}}]}"#,
        &["file_read"],
        &[],
    );
    let child = descriptor_json(
        "csv-analyze",
        "1.1.0",
        &ch(0xc2),
        r#"{"type":"string","max_bytes":4096}"#,
        // Child promises {row_count, summary}. Downstream consumers still get row_count.
        r#"{"type":"structured","fields":[{"name":"row_count","shape":{"type":"u64","max_bytes":8}},{"name":"summary","shape":{"type":"string","max_bytes":1024}}]}"#,
        &["file_read"],
        &[],
    );
    let r1 = score_root(&parent);
    let r2 = score_next_gen(&r1, &child).expect("adding an output field is a refinement");
    assert_verifies(&r2);
}

// ---- T9b: Liskov-correct — child output DROPS a field is REJECTED ------

#[test]
fn t9b_structured_field_dropped_from_output_rejected() {
    let parent = descriptor_json(
        "csv-analyze",
        "1.0.0",
        &ch(0xc1),
        r#"{"type":"string","max_bytes":4096}"#,
        // Parent promises {row_count, summary}.
        r#"{"type":"structured","fields":[{"name":"row_count","shape":{"type":"u64","max_bytes":8}},{"name":"summary","shape":{"type":"string","max_bytes":1024}}]}"#,
        &["file_read"],
        &[],
    );
    let child = descriptor_json(
        "csv-analyze",
        "1.1.0",
        &ch(0xc2),
        r#"{"type":"string","max_bytes":4096}"#,
        // Child drops `summary` — consumers depending on it are broken.
        r#"{"type":"structured","fields":[{"name":"row_count","shape":{"type":"u64","max_bytes":8}}]}"#,
        &["file_read"],
        &[],
    );
    let r1 = score_root(&parent);
    let err = typed_next_gen_err(&r1, &child);
    assert!(
        matches!(err, NextGenerationError::NotARefinement(RefinementError::OutputWidened)),
        "got {err}"
    );
}

// ---- T10 ---------------------------------------------------------------

#[test]
fn t10_list_item_refinement_recursive() {
    let parent = descriptor_json(
        "list-ingest",
        "1.0.0",
        &ch(0xd1),
        r#"{"type":"list","item":{"type":"string","max_bytes":1024},"max_items":10}"#,
        r#"{"type":"u32","max_bytes":4}"#,
        &["file_read"],
        &[],
    );

    // Child widens list item bytes and item count on input — OK.
    let widened = descriptor_json(
        "list-ingest",
        "1.1.0",
        &ch(0xd2),
        r#"{"type":"list","item":{"type":"string","max_bytes":2048},"max_items":20}"#,
        r#"{"type":"u32","max_bytes":4}"#,
        &["file_read"],
        &[],
    );
    let r1 = score_root(&parent);
    let r2 = score_next_gen(&r1, &widened).expect("list widening should refine");
    assert_verifies(&r2);

    // Reverse: narrower item bytes is rejected.
    let narrowed = descriptor_json(
        "list-ingest",
        "1.1.0",
        &ch(0xd3),
        r#"{"type":"list","item":{"type":"string","max_bytes":512},"max_items":10}"#,
        r#"{"type":"u32","max_bytes":4}"#,
        &["file_read"],
        &[],
    );
    let err = typed_next_gen_err(&r1, &narrowed);
    assert!(
        matches!(err, NextGenerationError::NotARefinement(RefinementError::InputNarrowed)),
        "got {err}"
    );
}

// ---- T11 ---------------------------------------------------------------

#[test]
fn t11_determinism() {
    let r1 = score_root(&pdf_v1());
    let a = score_next_gen(&r1, &pdf_v1_1()).expect("a");
    let b = score_next_gen(&r1, &pdf_v1_1()).expect("b");
    assert_eq!(a.output_hash, b.output_hash, "next_generation must be deterministic");
}

// ---- T12 ---------------------------------------------------------------

#[test]
fn t12_cross_computation_refinement_rejected() {
    let manifest = r#"[{"path":"skills/web-search","content_hash":"10b5b447"}]"#;
    let parent = score("merkle_manifest", manifest).expect("merkle parent");

    let err = next_generation_check(&lineage_input(&parent, &pdf_v1_1()))
        .err()
        .expect("expected error");
    assert!(
        matches!(err, NextGenerationError::InvalidParentComputation(_)),
        "got {err}"
    );
}

// ---- T13: precedence pin — validation runs before refinement -----------
//
// A child that is BOTH malformed (exceeds the 16 MiB shape ceiling) AND a
// refinement violation (output widens beyond parent) must surface as
// `MalformedDescriptor`, never as `NotARefinement`. This is the load-
// bearing test for the spec's "Precedence: validation before refinement"
// clause — a future refactor that swaps the order will break it.

#[test]
fn t13_malformed_child_supersedes_refinement_failure() {
    // Parent's output is small. A child with output > 16 MiB is BOTH:
    //   - malformed (string max_bytes > 16777216)  -> MalformedDescriptor
    //   - output-widening (16777217 > 1024)         -> would be OutputWidened
    // Validation must fire first.
    let small_parent = descriptor_json(
        "pdf-extract",
        "1.0.0",
        &ch(0xa1),
        r#"{"type":"string","max_bytes":4096}"#,
        r#"{"type":"string","max_bytes":1024}"#,
        &["file_read"],
        &[],
    );
    let oversized_widened_child = descriptor_json(
        "pdf-extract",
        "1.0.1",
        &ch(0xa2),
        r#"{"type":"string","max_bytes":4096}"#,
        // Exceeds the 16 MiB shape ceiling AND widens parent's 1024.
        r#"{"type":"string","max_bytes":16777217}"#,
        &["file_read"],
        &[],
    );
    let r1 = score_root(&small_parent);
    let err = typed_next_gen_err(&r1, &oversized_widened_child);
    assert!(
        matches!(err, NextGenerationError::MalformedDescriptor(_)),
        "expected MalformedDescriptor (validation precedes refinement), got {err}"
    );
}


// ---- HIGH-4: build-metadata-only chains are rejected -----------------
//
// SemVer §11 says build metadata MUST be ignored for ordering. R2 makes
// that explicit by comparing only (major, minor, patch), so the
// well-known SemVer-crate quirk around build metadata cannot leak
// through.

#[test]
fn high4_build_metadata_only_bump_rejected() {
    let parent = descriptor_json(
        "pdf-extract",
        "1.0.0+a",
        &ch(0xa1),
        r#"{"type":"string","max_bytes":4096}"#,
        r#"{"type":"string","max_bytes":16777216}"#,
        &["file_read"],
        &[],
    );
    let child = descriptor_json(
        "pdf-extract",
        "1.0.0+b",
        &ch(0xa2),
        r#"{"type":"string","max_bytes":4096}"#,
        r#"{"type":"string","max_bytes":16777216}"#,
        &["file_read"],
        &[],
    );
    let r1 = score_root(&parent);
    let err = typed_next_gen_err(&r1, &child);
    assert!(
        matches!(err, NextGenerationError::NotARefinement(RefinementError::VersionNotIncreased)),
        "expected VersionNotIncreased, got {err}"
    );
}

// ---- HIGH-5: prerelease ghost chains are rejected ---------------------
//
// Without R2's strict-triple rule a malicious skill could mint an
// unbounded chain `1.0.0-alpha.1 → 1.0.0-alpha.2 → … → 1.0.0-alpha.999`
// with zero structural change. The strict-triple rule blocks all of
// these by requiring the numeric (major, minor, patch) to advance.

#[test]
fn high5_prerelease_only_bump_rejected() {
    let parent = descriptor_json(
        "pdf-extract",
        "1.0.0-alpha.1",
        &ch(0xa1),
        r#"{"type":"string","max_bytes":4096}"#,
        r#"{"type":"string","max_bytes":16777216}"#,
        &["file_read"],
        &[],
    );
    let child = descriptor_json(
        "pdf-extract",
        "1.0.0-alpha.2",
        &ch(0xa2),
        r#"{"type":"string","max_bytes":4096}"#,
        r#"{"type":"string","max_bytes":16777216}"#,
        &["file_read"],
        &[],
    );
    let r1 = score_root(&parent);
    let err = typed_next_gen_err(&r1, &child);
    assert!(
        matches!(err, NextGenerationError::NotARefinement(RefinementError::VersionNotIncreased)),
        "expected VersionNotIncreased, got {err}"
    );
}

#[test]
fn high5_prerelease_to_release_same_triple_rejected() {
    let parent = descriptor_json(
        "pdf-extract",
        "1.0.0-alpha",
        &ch(0xa1),
        r#"{"type":"string","max_bytes":4096}"#,
        r#"{"type":"string","max_bytes":16777216}"#,
        &["file_read"],
        &[],
    );
    let child = descriptor_json(
        "pdf-extract",
        "1.0.0",
        &ch(0xa2),
        r#"{"type":"string","max_bytes":4096}"#,
        r#"{"type":"string","max_bytes":16777216}"#,
        &["file_read"],
        &[],
    );
    let r1 = score_root(&parent);
    let err = typed_next_gen_err(&r1, &child);
    assert!(
        matches!(err, NextGenerationError::NotARefinement(RefinementError::VersionNotIncreased)),
        "expected VersionNotIncreased, got {err}"
    );
}

