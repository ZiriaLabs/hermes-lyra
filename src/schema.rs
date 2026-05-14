//! `lyra-skill/v1` — the root schema for Lyra skill descriptors.
//!
//! A schema in Lyra is a CID-addressed object whose canonical bytes declare:
//! - The schema's `kind`, `version`, and IRI (human-readable identity).
//! - The UOR [`ConstrainedTypeShape`] it instantiates (machine-verifiable
//!   structural invariants the kernel grinds at preflight).
//!
//! Two parallel addresses exist for the same schema:
//!   - **Envelope CID** — `Cid::from_raw_blob` over the canonical bytes.
//!     This is the multiformats-standard, IPFS-compatible address that
//!     every SKILL.md embeds in its `contract.schema` field.
//!   - **UOR content fingerprint** — emitted by `pipeline::run` when the
//!     schema's constraint set is ground through the kernel. Proves the
//!     schema is internally consistent (adversaries cannot publish a
//!     schema declaring `0 = 1`; the kernel rejects it at preflight).
//!
//! The two POCs in `tests/poc_uor_constraints.rs` and `tests/poc_lyra_schema.rs`
//! empirically verified that this works: well-formed constraint declarations
//! ground; malformed ones (encoding the textbook impossibility `0 = bias≠0`)
//! are rejected with `PipelineFailure::ShapeViolation`. This module turns
//! that finding into a load-bearing schema.
//!
//! ## v1's discriminating rule
//!
//! `references[]` has at most 8 entries, encoded as an `Affine` over 8
//! per-slot indicators:
//!
//!   `1·slot_0 + 1·slot_1 + … + 1·slot_7 - 8 = 0`
//!
//! Coefficient sum is 8 (non-zero) → `is_affine_consistent` returns `true`
//! → the kernel admits the declaration → `pipeline::run` grounds it. A
//! schema author who *forgot* the per-slot coefficients (declaring
//! `[0;8]` with a non-zero bias) would be rejected at preflight.
//!
//! Per-value validation (e.g., "this skill's name is ≤ 64 chars") stays
//! in the binding layer (`SkillDescriptor` builder). The kernel attests
//! the constraint *system* is consistent; the binding layer attests
//! *this value* satisfies it. Separation of concerns is honest.

use uor_foundation::pipeline::{ConstrainedTypeShape, ConstraintRef, AFFINE_MAX_COEFFS};
use uor_foundation_sdk::output_shape;

/// Per-slot coefficients for the references-bound rule. Each of 8 reference
/// slots contributes 1 to the slot-count sum.
const REFERENCE_SLOT_COEFFS: [i64; AFFINE_MAX_COEFFS] = [1, 1, 1, 1, 1, 1, 1, 1];

/// Maximum number of reference slots a v1 skill may declare. Encoded as
/// the negation of the `Affine` bias so the equation reads
/// `sum(slot_0..slot_7) - MAX = 0`.
pub const LYRA_SKILL_V1_MAX_REFERENCES: i64 = 8;

const LYRA_SKILL_V1_CONSTRAINTS: &[ConstraintRef] = &[ConstraintRef::Affine {
    coefficients: REFERENCE_SLOT_COEFFS,
    coefficient_count: 8,
    bias: -LYRA_SKILL_V1_MAX_REFERENCES,
}];

output_shape! {
    pub struct LyraSkillSchemaV1;
    impl ConstrainedTypeShape for LyraSkillSchemaV1 {
        const IRI: &'static str = "https://lyra-protocol.org/schemas/v0.1/skill";
        const SITE_COUNT: usize = 8;
        const CONSTRAINTS: &'static [ConstraintRef] = LYRA_SKILL_V1_CONSTRAINTS;
        const CYCLE_SIZE: u64 = 1;
    }
}

/// Canonical byte serialization of the `lyra-skill/v1` schema declaration.
/// These exact bytes hash to [`LYRA_SKILL_SCHEMA_V1_CID`].
///
/// Format: a single-line canonical JSON object, keys in alphabetical
/// order, plus one trailing newline. Any byte change — even reordering
/// keys or whitespace — changes the CID and breaks the pinned invariant.
///
/// The byte sequence is sealed by the test
/// `canonical_bytes_hash_to_pinned_cid` below; that test is the only
/// load-bearing contract between the canonical bytes and the CID.
pub fn lyra_skill_v1_canonical_bytes() -> Vec<u8> {
    let mut s = String::with_capacity(320);
    // Alphabetical keys: constraints, iri, kind, site_count, version.
    s.push_str(
        r#"{"constraints":[{"bias":-8,"coefficient_count":8,"coefficients":[1,1,1,1,1,1,1,1],"type":"affine"}],"iri":""#,
    );
    s.push_str(<LyraSkillSchemaV1 as ConstrainedTypeShape>::IRI);
    s.push_str(r#"","kind":"lyra-skill","site_count":8,"version":"0.1.0"}"#);
    s.push('\n');
    s.into_bytes()
}

/// The pinned envelope CID of [`lyra_skill_v1_canonical_bytes`].
/// Derived as `Cid::from_raw_blob(canonical_bytes).to_string()`
/// (CIDv1 + raw codec `0x55` + BLAKE3-256). Pasted from the first
/// successful run of `canonical_bytes_hash_to_pinned_cid` below.
///
/// Every v1 SKILL.md embeds this string in its `contract.schema` field.
/// Verifiers compare the field byte-for-byte; anything else surfaces as
/// `unsupported_schema` (typed outcome, not an error).
pub const LYRA_SKILL_SCHEMA_V1_CID: &str =
    "bafkr4iepmp73holgr6qox5kq5zh24e5h64yu32kgx6thfqwm33k6rrktju";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cid::Cid;
    use crate::gate::LyraHasher;
    use uor_foundation::enforcement::{CompileUnitBuilder, Term};
    use uor_foundation::enums::{VerificationDomain, WittLevel};
    use uor_foundation::pipeline::run;

    /// **The pinning contract.** If you change the canonical bytes,
    /// this test prints the new CID. Paste it into
    /// `LYRA_SKILL_SCHEMA_V1_CID` and re-run.
    #[test]
    fn canonical_bytes_hash_to_pinned_cid() {
        let bytes = lyra_skill_v1_canonical_bytes();
        let derived = Cid::from_raw_blob(&bytes).to_string();
        assert_eq!(
            derived, LYRA_SKILL_SCHEMA_V1_CID,
            "\n  canonical bytes hash to: {derived}\n  pinned constant is:      {LYRA_SKILL_SCHEMA_V1_CID}\n  → paste {derived:?} into LYRA_SKILL_SCHEMA_V1_CID."
        );
    }

    /// The schema must ground through the UOR kernel. Proves the
    /// constraint set is internally consistent (no `0 = bias≠0`).
    #[test]
    fn kernel_grounds_the_v1_schema() {
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
            .expect("v1 schema CompileUnit must validate");

        run::<LyraSkillSchemaV1, _, LyraHasher>(unit)
            .expect("lyra-skill/v1 must ground through the UOR kernel");
    }
}
