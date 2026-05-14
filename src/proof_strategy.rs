//! UOR proof-strategy IRIs for Lyra's computation IDs.
//!
//! UOR foundation defines a [`ProofStrategy`] enum naming the methods by
//! which a [`Certificate`] can be issued — `Composition` for "proof by
//! composing sub-proofs," `Computation` for "decidable runtime check,"
//! `EulerPoincare` for nerve-based arguments, and so on.
//!
//! Lyra's four computation IDs each fall into one of these strategies:
//!
//! | computation_id              | strategy     | rationale                                              |
//! |-----------------------------|--------------|--------------------------------------------------------|
//! | `skill_interface_hash`      | Computation  | decidable hash over canonical descriptor bytes         |
//! | `skill_reference_resolve`   | Computation  | decidable CID-membership check against a manifest      |
//! | `compose_interfaces`        | Composition  | producer's output shape composes with consumer's input |
//! | `next_generation`           | Computation  | decidable R1–R5 refinement check                       |
//!
//! Note `next_generation` is *not* classified as `Composition`. A single
//! refinement step is one decidable check, not a composition of two
//! sub-proofs. A *chain* of refinements would compose — but that's not
//! what our `next_generation` operation does today.
//!
//! These IRIs are informational. They never enter the trust path (the
//! envelope CID is the trust path). They exist so JSON-LD consumers and
//! cross-framework tooling can grep "which UOR strategy backs this
//! computation?" against a stable, ontology-canonical URI.
//!
//! [`ProofStrategy`]: https://uor.foundation/proof/ProofStrategy
//! [`Certificate`]: https://uor.foundation/cert/Certificate

/// UOR proof strategy: "by composing proofs of sub-identities."
/// Maps to `ProofStrategy::Composition` in `uor-foundation::enums`.
pub const PROOF_STRATEGY_COMPOSITION_IRI: &str = "https://uor.foundation/proof/Composition";

/// UOR proof strategy: "by computation at a specified quantum level."
/// Maps to `ProofStrategy::Computation` in `uor-foundation::enums`.
/// Lean4 dialect: `by native_decide`.
pub const PROOF_STRATEGY_COMPUTATION_IRI: &str = "https://uor.foundation/proof/Computation";

/// Map a Lyra `computation_id` to the UOR proof-strategy IRI that
/// classifies the proof method backing it.
///
/// Unknown IDs fall through to `Computation` — the conservative default
/// because (a) every Lyra computation today is decidable, and (b)
/// `Computation` is the strategy that makes the fewest claims about
/// algebraic structure.
pub fn proof_strategy_iri(computation_id: &str) -> &'static str {
    match computation_id {
        "compose_interfaces" => PROOF_STRATEGY_COMPOSITION_IRI,
        // skill_interface_hash, skill_reference_resolve, next_generation
        // and anything else added later defaults to decidable computation.
        _ => PROOF_STRATEGY_COMPUTATION_IRI,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The compose gate is the only computation that maps to Composition.
    /// Everything else is Computation. This is the load-bearing
    /// classification; if it ever changes, the spec section must too.
    #[test]
    fn known_strategies_are_pinned() {
        assert_eq!(
            proof_strategy_iri("compose_interfaces"),
            PROOF_STRATEGY_COMPOSITION_IRI
        );
        assert_eq!(
            proof_strategy_iri("skill_interface_hash"),
            PROOF_STRATEGY_COMPUTATION_IRI
        );
        assert_eq!(
            proof_strategy_iri("skill_reference_resolve"),
            PROOF_STRATEGY_COMPUTATION_IRI
        );
        assert_eq!(
            proof_strategy_iri("next_generation"),
            PROOF_STRATEGY_COMPUTATION_IRI
        );
    }

    /// Unknown computation IDs default to the conservative Computation
    /// strategy rather than being rejected. Adding a new gate later
    /// without classifying it just means it's reported as a generic
    /// decidable computation — accurate but unspecific.
    #[test]
    fn unknown_id_falls_back_to_computation() {
        assert_eq!(
            proof_strategy_iri("not_a_real_gate_xyz"),
            PROOF_STRATEGY_COMPUTATION_IRI
        );
        assert_eq!(proof_strategy_iri(""), PROOF_STRATEGY_COMPUTATION_IRI);
    }

    /// IRIs must match the UOR foundation ontology paths verbatim.
    /// If UOR re-roots its IRI namespace, these constants change and
    /// every external JSON-LD consumer of a Lyra receipt is notified
    /// by way of a compile-time string change.
    #[test]
    fn iris_match_uor_foundation_ontology_paths() {
        assert_eq!(
            PROOF_STRATEGY_COMPOSITION_IRI,
            "https://uor.foundation/proof/Composition"
        );
        assert_eq!(
            PROOF_STRATEGY_COMPUTATION_IRI,
            "https://uor.foundation/proof/Computation"
        );
    }
}
