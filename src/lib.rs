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
/// Format: `hermes-lyra/<protocol-version>+uor-foundation/<substrate-version>`.
///
/// The protocol version is `LYRA_PROTOCOL_VERSION` (a const string, NOT
/// `CARGO_PKG_VERSION`). The crate version moves with every release; the
/// protocol version moves only when the canonical bytes or acceptance
/// set change in a way that invalidates older receipts. v0.2.x crate
/// versions all use protocol version `0.2.0` — strict-parse tightening
/// in v0.2.1 narrows the acceptance set without shifting canonical bytes
/// for already-valid input, so the protocol ident is stable across the
/// 0.2.x crate line. The substrate version is `UOR_FOUNDATION_VERSION`,
/// set by `build.rs` from `Cargo.lock` so the ident tracks the
/// actually-linked substrate by construction.
///
/// Bump `LYRA_PROTOCOL_VERSION` only when changing this is intended —
/// every receipt with the previous ident becomes `unsupported_protocol`
/// under the new build.
pub const LYRA_PROTOCOL_VERSION: &str = "0.3.0";

/// The protocol identifier prefix folded into every content-addressed
/// hash. Stable across crate patch releases — moves only on protocol
/// bumps. Used by [`crate::cid::Cid::from_canonical_input`].
pub const LYRA_PROTOCOL_ID_PREFIX: &str = "hermes-lyra/0.3";

pub const LYRA_RUNTIME_IDENT: &str = concat!(
    "hermes-lyra/", "0.3.0",
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


pub mod shape;
pub mod version;
pub mod descriptor;
pub mod schema;
pub mod proof_strategy;
pub mod cid;
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
