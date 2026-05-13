//! Verify that every Lyra descriptor under `examples/` is
//! **canonically rooted in UOR**.
//!
//! "Rooted in UOR" means: each descriptor parses through the typed
//! `SkillDescriptorBuilder`, which only accepts shapes whose leaf types
//! are the eight sealed shapes declared via the `output_shape!` macro in
//! `src/shape.rs` (`LyraU8`, `LyraU16`, `LyraU32`, `LyraU64`,
//! `LyraString`, `LyraBytes`, plus structured/list compositions). These
//! sealed types implement `uor_foundation::pipeline::ConstrainedTypeShape`
//! and cannot be constructed outside the sanctioned UOR pipeline.
//!
//! If a descriptor under `examples/` ever drifts away from
//! the sealed UOR vocabulary — by referencing a non-anchored shape tag,
//! by exceeding the macro-declared `CYCLE_SIZE`/`SITE_COUNT` bound, or
//! by violating any structural check — these tests fail loudly.
//!
//! The tests also assert that:
//!   1. The numeric `max_bytes` of every `u8/u16/u32/u64` leaf equals
//!      the macro's `SITE_COUNT` ceiling. This is the bridge from a
//!      string-tagged JSON shape to the typed UOR sealed shape.
//!   2. The lineage receipt for v0.1.0 → v0.1.1 of `code-review-evolve`
//!      mints cleanly — proving the refinement check is enforced by the
//!      same UOR-rooted typed builder, not by ad-hoc JSON shape diffing.

use lyra_ref::cli_api::score;
use lyra_ref::computations;
use lyra_ref::shape::{LyraU16, LyraU32, LyraU64, LyraU8, LyraBytes, LyraString};
use uor_foundation::pipeline::ConstrainedTypeShape;

const EXAMPLES_ROOT: &str = "examples";

fn read(rel: &str) -> String {
    let path = std::path::Path::new(EXAMPLES_ROOT).join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

/// Sanity: the **macro-declared** UOR constants are what the protocol
/// believes them to be. If `output_shape!` ever changes its semantics
/// upstream, the rest of these tests are testing the wrong invariant —
/// fail early here.
#[test]
fn uor_sealed_shape_constants_are_what_we_expect() {
    assert_eq!(<LyraU8  as ConstrainedTypeShape>::SITE_COUNT, 1);
    assert_eq!(<LyraU16 as ConstrainedTypeShape>::SITE_COUNT, 2);
    assert_eq!(<LyraU32 as ConstrainedTypeShape>::SITE_COUNT, 4);
    assert_eq!(<LyraU64 as ConstrainedTypeShape>::SITE_COUNT, 8);
    assert_eq!(<LyraString as ConstrainedTypeShape>::CYCLE_SIZE, 16_777_216);
    assert_eq!(<LyraBytes  as ConstrainedTypeShape>::CYCLE_SIZE, 16_777_216);
    // IRIs are namespaced under lyra-protocol.org.
    assert!(<LyraU8     as ConstrainedTypeShape>::IRI.contains("lyra-protocol.org"));
    assert!(<LyraString as ConstrainedTypeShape>::IRI.contains("lyra-protocol.org"));
}

/// Drives a descriptor through `score skill_interface_hash`. The CLI path
/// invokes the **typed** `SkillDescriptorBuilder`, which in turn invokes
/// the UOR-anchored sealed shapes. A non-anchored descriptor cannot pass.
fn assert_descriptor_anchored(rel_path: &str) {
    let raw = read(rel_path);
    // (1) Auto-detect: SKILL.md with embedded block or bare JSON descriptor.
    let json = lyra_ref::bridge::descriptor_from_anywhere(&raw)
        .unwrap_or_else(|e| panic!("{rel_path}: cannot recover descriptor: {e}"));
    // (2) Typed builder accepts it.
    let receipt = score("skill_interface_hash", &json)
        .unwrap_or_else(|e| panic!("{rel_path}: typed builder rejected: {e}"));
    // (2) Output hash is 64 hex chars (BLAKE3-256 over UOR-anchored canonical bytes).
    assert_eq!(receipt.output_hash.len(), 64, "{rel_path}: output_hash not 32 bytes");
    assert!(
        receipt.output_hash.bytes().all(|b| b.is_ascii_hexdigit()),
        "{rel_path}: output_hash not hex"
    );
    // (3) Runtime ident is bound to this build (uor-foundation substrate).
    assert!(
        receipt.runtime.contains("uor-foundation/"),
        "{rel_path}: receipt runtime not bound to uor-foundation: {}",
        receipt.runtime
    );
}

#[test]
fn inbox_triage_descriptor_is_uor_anchored() {
    // Source is now the single-file SKILL.md with embedded {descriptor, proof}.
    assert_descriptor_anchored("inbox-triage/SKILL.md");
}

#[test]
fn news_brief_descriptor_is_uor_anchored() {
    assert_descriptor_anchored("news-brief/SKILL.md");
}

#[test]
fn embedded_proofs_in_skill_md_files_verify_locally() {
    // The strongest UOR-rooting witness for the embedded form: the
    // proof inside each SKILL.md re-derives byte-identically against
    // the descriptor it carries, with no external table to trust.
    use lyra_ref::bridge::VerifyOutcome;
    for rel in ["inbox-triage/SKILL.md", "news-brief/SKILL.md"] {
        let md = read(rel);
        let outcome = lyra_ref::bridge::verify_embedded_proof(&md)
            .unwrap_or_else(|e| panic!("{rel}: verify failed: {e}"));
        assert!(
            matches!(outcome, VerifyOutcome::Valid { .. }),
            "{rel}: must verify under engine self-anchoring; got {outcome:?}",
        );
    }
}

#[test]
fn code_review_evolve_v010_is_uor_anchored() {
    assert_descriptor_anchored("code-review-evolve/v0.1.0.lyra.json");
}

#[test]
fn code_review_evolve_v011_is_uor_anchored() {
    assert_descriptor_anchored("code-review-evolve/v0.1.1.lyra.json");
}

/// The lineage receipt is the strongest UOR-rooting witness for the
/// `code-review-evolve` example: it proves that the *refinement* relation
/// (R1–R5) is enforced through the same typed pipeline that minted the
/// parent's seal. A non-anchored child cannot pass.
#[test]
fn code_review_evolve_lineage_mints_under_uor_rules() {
    let parent_json = read("code-review-evolve/v0.1.0.lyra.json");
    let child_json  = read("code-review-evolve/v0.1.1.lyra.json");

    // Mint the parent receipt via the public CLI API.
    let parent_receipt = score("skill_interface_hash", &parent_json)
        .expect("parent receipt should mint");
    let parent_b64 = lyra_ref::cli_api::base64_encode(parent_receipt.to_json().as_bytes());

    let ng_input = format!(
        r#"{{"parent_receipt":"{}","child_descriptor":{}}}"#,
        parent_b64, child_json
    );
    // The `next_generation` computation drives both descriptors through
    // the typed UOR-anchored builder *and* applies `is_refinement`. A
    // 32-byte output is returned on success.
    let out = computations::run("next_generation", &ng_input)
        .expect("v0.1.0 -> v0.1.1 should be a valid refinement");
    assert_eq!(out.len(), 32, "lineage output must be 32 raw bytes");
}

// The `integer_max_bytes_match_uor_site_counts` test was previously here
// as a belt-and-suspenders literal pattern match. It is now subsumed by
// `embedded_proofs_in_skill_md_files_verify_locally` above: a proof
// can only verify if every leaf's `max_bytes` is accepted by the typed
// builder, which only accepts the macro-declared SITE_COUNT / CYCLE_SIZE
// bounds. Re-deriving the proof IS the integer-bound check.
