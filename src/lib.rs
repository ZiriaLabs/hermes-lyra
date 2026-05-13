//! Reference library for the Lyra Protocol.
//!
//! Provides typed skill descriptors, composability checks, and
//! verifiable receipts.
//!
//! # Quick reference
//!
//! - [`SkillDescriptor`] — typed skill declaration; built via
//!   [`SkillDescriptor::builder`].
//! - [`validate_skill`] — validate a descriptor and emit a receipt.
//! - [`check_composable`] — check whether two skills compose.
//! - [`registry_snapshot`] — hash a manifest of registered skills.
//! - [`jsonld`] — JSON-LD edge format for cross-framework interchange.
//!
//! # Example
//!
//! ```no_run
//! use lyra_ref::{SkillDescriptor, validate_skill, Attestation};
//! use lyra_ref::descriptor::{Shape, EffectKind};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let desc = SkillDescriptor::builder()
//!     .name("pdf-extract")
//!     .version("1.0.0")
//!     .content_hash_hex(&"a1".repeat(32))
//!     .input_shape(Shape::String { max_bytes: 4096 })
//!     .output_shape(Shape::String { max_bytes: 16_777_216 })
//!     .effect(EffectKind::FileRead)
//!     .build()?;
//!
//! let att: Attestation = validate_skill(&desc)?;
//! // Layer 1: receipt_hash binds to the exact descriptor bytes.
//! let _hex: String = att.receipt_hash_hex();
//! // Layer 2: sealed structural attestation from the pipeline.
//! let _seal = &att.seal;
//! # Ok(())
//! # }
//! ```

/// Identifier of the implementation + substrate that produced a receipt.
///
/// Folded into the canonical bytes of every attestation so seals
/// produced under different substrate versions are *visibly* distinct
/// rather than silently divergent.
///
/// Format: `hermes-lyra/<crate-version>+uor-foundation/<substrate-version>`.
///
/// Both versions come from compile-time env vars. The crate version is
/// `CARGO_PKG_VERSION` (cargo sets this automatically); the substrate
/// version is `UOR_FOUNDATION_VERSION`, set by `build.rs` from
/// `Cargo.lock` so the ident tracks the actually-linked substrate by
/// construction.
pub const LYRA_RUNTIME_IDENT: &str = concat!(
    "hermes-lyra/", env!("CARGO_PKG_VERSION"),
    "+uor-foundation/", env!("UOR_FOUNDATION_VERSION"),
);

/// **(S3)** Older runtime identifiers that this build can still verify.
///
/// `verify` accepts a receipt whose `runtime` is `LYRA_RUNTIME_IDENT`
/// OR appears in this list. Entries are added only when a substrate
/// upgrade is byte-equivalent for the canonical-bytes and pipeline
/// surfaces Lyra depends on (i.e., it does not change `mint_seal`'s
/// output bytes for any input). Without this list a runtime upgrade
/// would invalidate every prior receipt with no migration path; with
/// it, append-only audit chains survive backwards-compatible
/// substrate upgrades.
///
/// Empty for v0.1.0 (no predecessors). v0.2+ adds older idents here
/// as they are explicitly audited and certified compatible.
pub const COMPATIBLE_RUNTIMES: &[&str] = &[];

/// Returns true iff a receipt's claimed runtime is acceptable to this verifier.
pub fn runtime_is_compatible(runtime: &str) -> bool {
    runtime == LYRA_RUNTIME_IDENT || COMPATIBLE_RUNTIMES.contains(&runtime)
}

/// Canonical repository URI for the Lyra Protocol. Embedded as the
/// `spec_uri` field of every minted proof so a **cold-start** agent
/// with zero prior knowledge of Lyra can fetch the rules and the
/// reference implementation without searching. This is a *hint*, not
/// authority — verifiers do not consult it during the verify path;
/// the authoritative identifier remains `protocol`. The URI is
/// resolvable by any mirror (Git clone, HTTPS fetch, archived
/// snapshot); no single host gates verification.
pub const LYRA_SPEC_URI: &str = "https://github.com/ZiriaLabs/hermes-lyra";


pub mod shape;
pub mod version;
pub mod descriptor;
pub mod gate;
pub mod jsonld;
pub mod install;
pub mod refinement;

// CLI-facing modules: JSON-string API used by the `lyra` binary.
pub mod bridge;
pub mod cli_api;
pub mod computations;
pub mod demo;
pub mod fuse;
pub mod mcp;
pub mod receipt;
pub mod tripwire;
pub mod wire;

pub use descriptor::{
    validate_name, validate_reference, DescriptorBuildError,
    SkillDescriptor, SkillDescriptorBuilder,
};
pub use gate::{validate_skill, check_composable, registry_snapshot, Attestation, ValidationError};
pub use refinement::{is_refinement, RefinementError};
pub use computations::{next_generation_check, NextGenerationError};
pub use receipt::Receipt;
pub use cli_api::{score, verify, VerifyOutcome};
