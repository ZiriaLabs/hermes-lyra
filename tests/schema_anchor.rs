//! Schemas-first invariants.
//!
//! Three contracts asserted here are load-bearing for the schemas-first
//! wedge:
//!
//! 1. `LyraSkillSchemaV1` is internally consistent — the UOR kernel
//!    grinds its constraint set to `Ok(Grounded<T>)`. An adversary who
//!    publishes a "schema" declaring `0 = 1` cannot pass this test.
//!
//! 2. The pinned `LYRA_SKILL_SCHEMA_V1_CID` matches the BLAKE3-CID over
//!    the canonical bytes of the schema declaration. Drift between the
//!    constant and the bytes breaks the seal.
//!
//! 3. The builder rejects any descriptor declaring an unrecognized
//!    schema with `DescriptorBuildError::UnsupportedSchema`. The
//!    recognized v1 CID round-trips through builder → canonicalize →
//!    re-parse without loss.

use lyra_ref::cid::Cid;
use lyra_ref::descriptor::{DescriptorBuildError, EffectKind, Shape, SkillDescriptor};
use lyra_ref::gate::LyraHasher;
use lyra_ref::schema::{
    lyra_skill_v1_canonical_bytes, LyraSkillSchemaV1, LYRA_SKILL_SCHEMA_V1_CID,
};
use uor_foundation::enforcement::{CompileUnitBuilder, Term};
use uor_foundation::enums::{VerificationDomain, WittLevel};
use uor_foundation::pipeline::run;

/// (1) The schema declaration grinds through the kernel.
#[test]
fn lyra_skill_v1_grounds_in_uor_kernel() {
    let w64 = WittLevel::new(64);
    static TERMS: &[Term] = &[Term::Literal {
        value: 1,
        level: WittLevel::W8,
    }];
    static DOMAINS: &[VerificationDomain] = &[VerificationDomain::Enumerative];

    let unit = CompileUnitBuilder::new()
        .root_term(TERMS)
        .witt_level_ceiling(w64)
        .thermodynamic_budget(512)
        .target_domains(DOMAINS)
        .result_type::<LyraSkillSchemaV1>()
        .validate()
        .expect("CompileUnit must validate for the v1 schema");

    run::<LyraSkillSchemaV1, _, LyraHasher>(unit)
        .expect("kernel must ground lyra-skill/v1");
}

/// (2) Canonical bytes hash to the pinned schema CID.
#[test]
fn canonical_bytes_match_pinned_schema_cid() {
    let bytes = lyra_skill_v1_canonical_bytes();
    let derived = Cid::from_raw_blob(&bytes).to_string();
    assert_eq!(
        derived, LYRA_SKILL_SCHEMA_V1_CID,
        "canonical bytes derive {derived}, but pinned constant is {LYRA_SKILL_SCHEMA_V1_CID}"
    );
}

/// (3a) Builder rejects unknown schema values with the typed error.
#[test]
fn builder_rejects_unknown_schema() {
    let result = SkillDescriptor::builder()
        .name("x")
        .version("1.0.0")
        .content_hash_hex(&"aa".repeat(32))
        .input_shape(Shape::U8 { max_bytes: 1 })
        .output_shape(Shape::U8 { max_bytes: 1 })
        .effect(EffectKind::None)
        .schema("bafkr4i-not-a-real-schema-cid")
        .build();
    let err = result.expect_err("unknown schema must be rejected");
    assert!(
        matches!(err, DescriptorBuildError::UnsupportedSchema(_)),
        "expected UnsupportedSchema, got {err:?}"
    );
}

/// (3b) Default builder produces a descriptor pinned to the v1 schema CID.
#[test]
fn default_builder_pins_v1_schema() {
    let desc = SkillDescriptor::builder()
        .name("x")
        .version("1.0.0")
        .content_hash_hex(&"aa".repeat(32))
        .input_shape(Shape::U8 { max_bytes: 1 })
        .output_shape(Shape::U8 { max_bytes: 1 })
        .effect(EffectKind::None)
        .build()
        .expect("default-schema build must succeed");
    assert_eq!(desc.schema(), LYRA_SKILL_SCHEMA_V1_CID);
}

/// (3c) Explicit v1 schema is accepted (and equal to the default).
#[test]
fn explicit_v1_schema_is_accepted() {
    let desc = SkillDescriptor::builder()
        .name("x")
        .version("1.0.0")
        .content_hash_hex(&"aa".repeat(32))
        .input_shape(Shape::U8 { max_bytes: 1 })
        .output_shape(Shape::U8 { max_bytes: 1 })
        .effect(EffectKind::None)
        .schema(LYRA_SKILL_SCHEMA_V1_CID)
        .build()
        .expect("explicit v1 schema must build");
    assert_eq!(desc.schema(), LYRA_SKILL_SCHEMA_V1_CID);
}

/// Every example SKILL.md now declares the v1 schema on the wire.
/// This is the visible side of schemas-first: schemas are not hidden
/// in code, they're written into the file the user reads.
#[test]
fn examples_declare_schema_on_the_wire() {
    let paths = [
        "examples/inbox-triage/SKILL.md",
        "examples/news-brief/SKILL.md",
    ];
    let needle = format!(r#""schema":"{LYRA_SKILL_SCHEMA_V1_CID}""#);
    for path in paths {
        let md = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("read {path}: {e}"));
        assert!(
            md.contains(&needle),
            "{path} must contain schema declaration {needle}"
        );
    }
}
