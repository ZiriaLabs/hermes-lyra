//! Refinement predicate for Lyra descriptors.
//!
//! A child descriptor *refines* a parent iff it is **Liskov-substitutable**
//! for the parent: anywhere the parent's signature is accepted, the child
//! can be dropped in safely. Concretely, the child must accept everything
//! the parent accepts on the input side, and produce everything the parent
//! promises on the output side.
//!
//! This relation gates `next_generation` lineage receipts:
//! **a mutation that does not typecheck against its parent cannot mint a
//! next_generation receipt.**

use crate::descriptor::{Shape, SkillDescriptor};

/// Reason a child descriptor does not refine its parent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefinementError {
    /// child.name != parent.name (R1)
    NameChanged,
    /// child.version is not strictly greater than parent.version (R2)
    VersionNotIncreased,
    /// One of the version strings does not parse as SemVer (R2)
    InvalidVersion(String),
    /// Child input is not a superset of parent input (R3)
    InputNarrowed,
    /// Child output is not a superset of parent output (R4)
    OutputWidened,
    /// Child declared an effect the parent did not declare (R5)
    EffectAdded,
    /// Generic detail (used internally; not produced by the v0.1 rules)
    Detail(String),
}

impl core::fmt::Display for RefinementError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            RefinementError::NameChanged          => write!(f, "NameChanged"),
            RefinementError::VersionNotIncreased  => write!(f, "VersionNotIncreased"),
            RefinementError::InvalidVersion(s)    => write!(f, "InvalidVersion({s})"),
            RefinementError::InputNarrowed        => write!(f, "InputNarrowed"),
            RefinementError::OutputWidened        => write!(f, "OutputWidened"),
            RefinementError::EffectAdded          => write!(f, "EffectAdded"),
            RefinementError::Detail(s)            => write!(f, "Detail({s})"),
        }
    }
}

impl std::error::Error for RefinementError {}

/// Returns `Ok(())` iff `child` is a Liskov-substitutable refinement of `parent`.
///
/// Rules:
/// - **R1.** `child.name == parent.name`.
/// - **R2.** `semver(child.version) > semver(parent.version)`.
/// - **R3.** Child input is a *superset* of parent input (caller-side widening).
/// - **R4.** Child output is a *superset* of parent output (consumer-side preservation).
/// - **R5.** `child.effects ⊆ parent.effects` — no new side effects.
/// - **R6.** References are advisory; not gated.
///
/// **Structured fields**, by set semantics:
///   * For inputs (R3): a record satisfying the parent must satisfy the child.
///     Required fields shrink: `child.fields ⊆ parent.fields`.
///   * For outputs (R4): a record produced by the child must satisfy the parent.
///     Promised fields grow: `child.fields ⊇ parent.fields`.
pub fn is_refinement(
    parent: &SkillDescriptor,
    child: &SkillDescriptor,
) -> Result<(), RefinementError> {
    // R1
    if parent.name() != child.name() {
        return Err(RefinementError::NameChanged);
    }

    // R2 — parse both versions as SemVer (rejects malformed strings),
    // then require a **strictly-increasing (major, minor, patch) tuple**.
    //
    // **HIGH-5**: SemVer's full ordering allows unbounded prerelease and
    // build-metadata refinement chains (e.g., `1.0.0-alpha.1 → 1.0.0-alpha.2
    // → 1.0.0-alpha.3 → …`, or `1.0.0+a → 1.0.0+b`). Both are pollution
    // vectors for any registry that accepts the chain. R2 ignores
    // prerelease and build metadata for ordering: a refinement requires a
    // real numeric version bump.
    let parent_ver = crate::version::parse(parent.version())
        .map_err(|e| RefinementError::InvalidVersion(format!("parent: {e}")))?;
    let child_ver = crate::version::parse(child.version())
        .map_err(|e| RefinementError::InvalidVersion(format!("child: {e}")))?;
    let parent_triple = (parent_ver.major, parent_ver.minor, parent_ver.patch);
    let child_triple  = (child_ver.major,  child_ver.minor,  child_ver.patch);
    if child_triple <= parent_triple {
        return Err(RefinementError::VersionNotIncreased);
    }

    // R3 — input widens
    input_widens(parent.input_shape(), child.input_shape())?;

    // R4 — output narrows (each parent-promised field must still appear,
    // with a refined sub-shape; child may add fields beyond that).
    output_narrows(parent.output_shape(), child.output_shape())?;

    // R5 — effects ⊆
    for child_eff in child.effects() {
        if !parent.effects().contains(child_eff) {
            return Err(RefinementError::EffectAdded);
        }
    }

    // R6 — references are advisory.
    Ok(())
}

/// Child input shape must accept everything parent input shape accepts.
///
/// For structured: every field the *child* requires must already exist in
/// the parent (so the caller's parent-typed value will satisfy the child).
/// `child.fields ⊆ parent.fields`. Removing fields on the input side is
/// permitted; *adding* required fields is rejected.
fn input_widens(parent: &Shape, child: &Shape) -> Result<(), RefinementError> {
    use Shape::*;
    match (parent, child) {
        (U8  { max_bytes: p }, U8  { max_bytes: c }) if c >= p => Ok(()),
        (U16 { max_bytes: p }, U16 { max_bytes: c }) if c >= p => Ok(()),
        (U32 { max_bytes: p }, U32 { max_bytes: c }) if c >= p => Ok(()),
        (U64 { max_bytes: p }, U64 { max_bytes: c }) if c >= p => Ok(()),
        (String { max_bytes: p }, String { max_bytes: c }) if c >= p => Ok(()),
        (Bytes  { max_bytes: p }, Bytes  { max_bytes: c }) if c >= p => Ok(()),
        (Structured { fields: pf }, Structured { fields: cf }) => {
            // Every child-required field must exist in the parent and
            // be input-widening at that field's sub-shape.
            for child_field in cf {
                let parent_field = pf
                    .iter()
                    .find(|f| f.name == child_field.name)
                    .ok_or(RefinementError::InputNarrowed)?;
                input_widens(&parent_field.shape, &child_field.shape)?;
            }
            Ok(())
        }
        (List { item: pi, max_items: pm }, List { item: ci, max_items: cm }) => {
            input_widens(pi, ci)?;
            if cm < pm {
                return Err(RefinementError::InputNarrowed);
            }
            Ok(())
        }
        _ => Err(RefinementError::InputNarrowed),
    }
}

/// Child output shape must satisfy every promise parent output shape makes.
///
/// For structured: every field the *parent* promised must still appear in
/// the child, with a sub-shape that is itself output-narrowing.
/// `child.fields ⊇ parent.fields`. Adding fields on the output side is
/// permitted; *dropping* promised fields is rejected.
fn output_narrows(parent: &Shape, child: &Shape) -> Result<(), RefinementError> {
    use Shape::*;
    match (parent, child) {
        (U8  { max_bytes: p }, U8  { max_bytes: c }) if c <= p => Ok(()),
        (U16 { max_bytes: p }, U16 { max_bytes: c }) if c <= p => Ok(()),
        (U32 { max_bytes: p }, U32 { max_bytes: c }) if c <= p => Ok(()),
        (U64 { max_bytes: p }, U64 { max_bytes: c }) if c <= p => Ok(()),
        (String { max_bytes: p }, String { max_bytes: c }) if c <= p => Ok(()),
        (Bytes  { max_bytes: p }, Bytes  { max_bytes: c }) if c <= p => Ok(()),
        (Structured { fields: pf }, Structured { fields: cf }) => {
            // Every parent-promised field must exist in the child.
            for parent_field in pf {
                let child_field = cf
                    .iter()
                    .find(|f| f.name == parent_field.name)
                    .ok_or(RefinementError::OutputWidened)?;
                output_narrows(&parent_field.shape, &child_field.shape)?;
            }
            Ok(())
        }
        (List { item: pi, max_items: pm }, List { item: ci, max_items: cm }) => {
            output_narrows(pi, ci)?;
            if cm > pm {
                return Err(RefinementError::OutputWidened);
            }
            Ok(())
        }
        _ => Err(RefinementError::OutputWidened),
    }
}
