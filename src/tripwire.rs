//! The five canonical gates.
//!
//! ```text
//! anchor    embed a typed contract + self-verifying proof in any SKILL.md
//! verify    re-derive the embedded proof; return descriptor + status
//! evolve    refinement gate (R1–R5); promote new version or roll back
//! compose   composition gate; works for a pair or an N-skill chain
//! ```
//!
//! Each gate operates on `SKILL.md` files (or, transparently, on raw
//! descriptor JSON). Each one is canonical: one operation, one outcome
//! shape, one purpose. The agent surface is exactly these five; lower-
//! level computation primitives (`lyra_score`, `skill_verify`) live in
//! the library and CLI for power users.

use crate::cli_api::{base64_encode, hex_encode, score};
use crate::computations::{next_generation_check, NextGenerationError};
use crate::receipt::Receipt;
use crate::refinement::RefinementError;

// =========================================================================
// evolve — the refinement gate (R1–R5)
// =========================================================================

/// Outcome of a `refine` call. **Every** failure path produces a
/// structured variant. Three modes:
///
///   * `Promote`             — child is a legitimate refinement.
///   * `Rollback`            — child violates an R1–R5 rule.
///   * `MalformedDescriptor` — either parent or child failed to parse
///                             as a Lyra descriptor.
#[derive(Debug, Clone)]
pub enum RefineResult {
    /// The child is a Liskov-substitutable refinement of the parent.
    /// `lineage_receipt` is the audit artifact the operator records.
    Promote { lineage_receipt: Receipt },
    /// The child is not a legitimate refinement. `rule_fired` names
    /// the R1–R5 rule that rejected it; `reason` carries the typed
    /// error for diagnostics.
    Rollback { rule_fired: String, reason: String },
    /// The parent or child descriptor did not parse. `which` is
    /// `"parent"` or `"child"`.
    MalformedDescriptor { which: String, reason: String },
}

impl RefineResult {
    /// JSON wire form — the same shape the MCP tool emits and the CLI prints.
    pub fn to_json(&self) -> String {
        match self {
            RefineResult::Promote { lineage_receipt } => format!(
                r#"{{"status":"promote","lineage_receipt":{}}}"#,
                lineage_receipt.to_json()
            ),
            RefineResult::Rollback { rule_fired, reason } => format!(
                r#"{{"status":"rollback","rule_fired":"{}","reason":"{}"}}"#,
                json_escape(rule_fired),
                json_escape(reason),
            ),
            RefineResult::MalformedDescriptor { which, reason } => format!(
                r#"{{"status":"malformed_descriptor","which":"{}","reason":"{}"}}"#,
                json_escape(which),
                json_escape(reason),
            ),
        }
    }
}

/// Run the refinement gate. Both inputs are accepted as descriptor
/// JSON or `SKILL.md` text — the bridge auto-detects. Failure modes
/// are routed into typed `RefineResult` variants, never bare error
/// strings.
pub fn check_refine(parent_input: &str, child_input: &str) -> Result<RefineResult, String> {
    let parent_json = match crate::bridge::descriptor_from_anywhere(parent_input) {
        Ok(s) => s,
        Err(e) => return Ok(RefineResult::MalformedDescriptor {
            which: "parent".into(), reason: e,
        }),
    };
    let child_json = match crate::bridge::descriptor_from_anywhere(child_input) {
        Ok(s) => s,
        Err(e) => return Ok(RefineResult::MalformedDescriptor {
            which: "child".into(), reason: e,
        }),
    };

    // 1. Mint the parent's interface receipt. A malformed parent
    //    surfaces here as a typed `MalformedDescriptor` outcome.
    let parent_receipt = match score("skill_interface_hash", &parent_json) {
        Ok(r) => r,
        Err(e) => return Ok(RefineResult::MalformedDescriptor {
            which: "parent".into(), reason: e,
        }),
    };

    // 2. Run the lineage check against the typed pipeline. Typed errors
    //    let us name R1–R5 precisely.
    let pr_b64 = base64_encode(parent_receipt.to_json().as_bytes());
    let ng_input = format!(
        r#"{{"parent_receipt":"{pr_b64}","child_descriptor":{child_json}}}"#,
    );

    match next_generation_check(&ng_input) {
        Ok(output) => {
            let lineage = Receipt {
                computation_id: "next_generation".to_string(),
                input: ng_input,
                output_hash: hex_encode(&output),
                runtime: crate::LYRA_RUNTIME_IDENT.to_string(),
            };
            Ok(RefineResult::Promote { lineage_receipt: lineage })
        }
        Err(NextGenerationError::NotARefinement(re)) => Ok(RefineResult::Rollback {
            rule_fired: refinement_rule_name(&re).to_string(),
            reason: format!("{re}"),
        }),
        Err(NextGenerationError::MalformedDescriptor(s)) => {
            // **Audit fix**: capacity overflow is NOT a malformation.
            // A child that legitimately adds a structured field but
            // pushes the field-product past the 16 MiB universal cap
            // is type-substitutable under R4 (output_narrows allows
            // field-additions); it just exceeds the absolute size
            // budget. Route as a typed rollback so agents handle it
            // like R1–R5 rejections (don't promote), not like format
            // errors (fix your descriptor).
            if crate::descriptor::is_capacity_exceeded_error(&s) {
                Ok(RefineResult::Rollback {
                    rule_fired: "CapacityExceeded".to_string(),
                    reason: s,
                })
            } else {
                Ok(RefineResult::MalformedDescriptor {
                    which: "child".into(),
                    reason: s,
                })
            }
        }
        Err(NextGenerationError::BadInput(s)) => Err(format!("bad input: {s}")),
        Err(e) => Err(format!("{e}")),
    }
}

fn refinement_rule_name(e: &RefinementError) -> &'static str {
    match e {
        RefinementError::NameChanged         => "R1_NameChanged",
        RefinementError::VersionNotIncreased => "R2_VersionNotIncreased",
        RefinementError::InvalidVersion(_)   => "R2_InvalidVersion",
        RefinementError::InputNarrowed       => "R3_InputNarrowed",
        RefinementError::OutputWidened       => "R4_OutputWidened",
        RefinementError::EffectAdded         => "R5_EffectAdded",
        RefinementError::Detail(_)           => "Unknown",
    }
}

// =========================================================================
// compose — the composition gate (pair or chain)
// =========================================================================

/// Outcome of a `compose` call. **Every** failure path produces a
/// structured variant — callers should never need to parse a bare
/// error string. Two distinct failure modes:
///
///   * `Incompatible`        — both descriptors parsed successfully,
///                             but their shapes do not compose.
///   * `MalformedDescriptor` — one of the inputs did not even parse
///                             as a valid Lyra descriptor.
/// `at_step` is the **transition index** — `skills[at_step] → skills[at_step+1]`
/// is the edge that failed. The producer's array position is `at_step`;
/// the consumer's is `at_step + 1`. For a 2-element chain this is
/// always `0` (the single producer→consumer edge). For a 3-element
/// chain, the second edge (`skills[1] → skills[2]`) is `at_step = 1`,
/// not `2`. `MalformedDescriptor` deliberately points at the offending
/// element directly (so `at_step` there is the array index of the bad
/// descriptor), since a parse failure has no "edge" to attribute to.
#[derive(Debug, Clone)]
pub enum ComposeResult {
    /// All adjacent pairs compose. `effects_union` is the sorted,
    /// deduplicated set of effects across every skill in the chain —
    /// surfaces what the pipeline can do, in aggregate, so an operator
    /// approving the chain isn't blind to the accumulated capability
    /// (audit H-3: a `Compatible` chain of `web_write` + `file_write`
    /// + `llm` would otherwise look indistinguishable from a pure
    /// chain).
    Compatible { effects_union: Vec<String> },
    /// Both descriptors parsed; the shapes do not compose at the edge
    /// `skills[at_step] → skills[at_step+1]`.
    Incompatible { at_step: usize, reason: String },
    /// The descriptor at `skills[at_step]` did not parse as a valid
    /// Lyra descriptor. `at_step` is the array index of the bad element.
    MalformedDescriptor { at_step: usize, reason: String },
}

impl ComposeResult {
    pub fn to_json(&self) -> String {
        match self {
            ComposeResult::Compatible { effects_union } => {
                let mut effs = String::with_capacity(32);
                effs.push('[');
                for (i, e) in effects_union.iter().enumerate() {
                    if i > 0 { effs.push(','); }
                    effs.push('"');
                    effs.push_str(&json_escape(e));
                    effs.push('"');
                }
                effs.push(']');
                format!(r#"{{"status":"compatible","effects_union":{effs}}}"#)
            }
            ComposeResult::Incompatible { at_step, reason } => format!(
                r#"{{"status":"incompatible","at_step":{at_step},"reason":"{}"}}"#,
                json_escape(reason),
            ),
            ComposeResult::MalformedDescriptor { at_step, reason } => format!(
                r#"{{"status":"malformed_descriptor","at_step":{at_step},"reason":"{}"}}"#,
                json_escape(reason),
            ),
        }
    }
}

/// Run the composition gate. Accepts a slice of `SKILL.md` text or
/// descriptor JSON (auto-detected per element). Length 2 is a pair
/// check; length ≥ 3 is a chain check. Length < 2 is an error.
///
/// Failure modes are routed into typed `ComposeResult` variants, NOT
/// `Err(String)`. The outer `Result` is reserved for catastrophic
/// failures (e.g. internal pipeline crashes) that callers cannot
/// route meaningfully.
pub fn check_compose(skills: &[&str]) -> Result<ComposeResult, String> {
    if skills.len() < 2 {
        return Err("compose needs at least 2 skills".into());
    }
    // Collect every effect from every successfully-parsed descriptor;
    // on a fully-compatible chain we emit the sorted, deduplicated
    // union so callers can see the aggregate capability surface.
    let mut all_effects: Vec<crate::descriptor::EffectKind> = Vec::new();
    for i in 0..skills.len() - 1 {
        // Extract descriptor JSON. Malformed input → typed Result variant,
        // not Err. Caller can distinguish "wrong descriptor" from "wrong
        // composition" by status.
        let producer_json = match crate::bridge::descriptor_from_anywhere(skills[i]) {
            Ok(s) => s,
            Err(e) => return Ok(ComposeResult::MalformedDescriptor { at_step: i, reason: e }),
        };
        let consumer_json = match crate::bridge::descriptor_from_anywhere(skills[i + 1]) {
            Ok(s) => s,
            Err(e) => return Ok(ComposeResult::MalformedDescriptor { at_step: i + 1, reason: e }),
        };
        let producer = match crate::computations::descriptor_from_json(&producer_json) {
            Ok(d) => d,
            Err(e) => return Ok(ComposeResult::MalformedDescriptor { at_step: i, reason: e }),
        };
        let consumer = match crate::computations::descriptor_from_json(&consumer_json) {
            Ok(d) => d,
            Err(e) => return Ok(ComposeResult::MalformedDescriptor { at_step: i + 1, reason: e }),
        };
        match crate::gate::check_composable(&producer, &consumer) {
            Ok(_) => {
                for e in producer.effects() {
                    if *e != crate::descriptor::EffectKind::None
                        && !all_effects.contains(e)
                    {
                        all_effects.push(*e);
                    }
                }
                if i == skills.len() - 2 {
                    for e in consumer.effects() {
                        if *e != crate::descriptor::EffectKind::None
                            && !all_effects.contains(e)
                        {
                            all_effects.push(*e);
                        }
                    }
                }
                continue;
            }
            Err(crate::gate::ValidationError::Incompatible(reason)) => {
                return Ok(ComposeResult::Incompatible { at_step: i, reason });
            }
            Err(other) => return Err(format!("{other:?}")),
        }
    }
    all_effects.sort_by_key(|e| effect_sort_key(*e));
    let effects_union: Vec<String> = all_effects
        .into_iter()
        .map(|e| effect_wire_name(e).to_string())
        .collect();
    Ok(ComposeResult::Compatible { effects_union })
}

fn effect_sort_key(e: crate::descriptor::EffectKind) -> u8 {
    use crate::descriptor::EffectKind::*;
    match e {
        None => 0, FileRead => 1, FileWrite => 2,
        WebRead => 3, WebWrite => 4, Terminal => 5, Llm => 6,
    }
}

fn effect_wire_name(e: crate::descriptor::EffectKind) -> &'static str {
    use crate::descriptor::EffectKind::*;
    match e {
        None => "none", FileRead => "file_read", FileWrite => "file_write",
        WebRead => "web_read", WebWrite => "web_write",
        Terminal => "terminal", Llm => "llm",
    }
}

// =========================================================================
// helpers
// =========================================================================

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"'  => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

// =========================================================================
// tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const V010: &str = include_str!("../examples/code-review-evolve/v0.1.0.lyra.json");
    const V011: &str = include_str!("../examples/code-review-evolve/v0.1.1.lyra.json");
    const V020_BAD: &str = include_str!("../examples/code-review-evolve/v0.2.0-bad.lyra.json");

    #[test]
    fn refine_promotes_legitimate_refinement() {
        match check_refine(V010, V011).unwrap() {
            RefineResult::Promote { lineage_receipt } => {
                assert_eq!(lineage_receipt.output_hash.len(), 64);
            }
            other => panic!("expected promote, got {other:?}"),
        }
    }

    #[test]
    fn refine_rolls_back_on_dropped_field() {
        match check_refine(V010, V020_BAD).unwrap() {
            RefineResult::Rollback { rule_fired, .. } => assert_eq!(rule_fired, "R4_OutputWidened"),
            other => panic!("expected rollback, got {other:?}"),
        }
    }

    #[test]
    fn refine_routes_malformed_descriptor_as_typed_variant() {
        // Audit #1+#2: a malformed descriptor must NOT collapse to
        // Err(String). It surfaces as RefineResult::MalformedDescriptor
        // so callers route it distinctly from Rollback.
        let bad = r#"{"not":"a","valid":"descriptor"}"#;
        match check_refine(V010, bad).unwrap() {
            RefineResult::MalformedDescriptor { which, .. } => assert_eq!(which, "child"),
            other => panic!("expected MalformedDescriptor, got {other:?}"),
        }
    }

    /// **Audit fix**: the structured-output + field-addition path is
    /// the most natural way an agent evolves a skill. With realistic
    /// field sizes, adding any new field can push the structured
    /// product past the 16 MiB cap. The gate must route this as a
    /// **rollback** (don't promote — too big) with `rule_fired:
    /// CapacityExceeded`, NOT as `malformed_descriptor` (which
    /// would tell the agent its descriptor format is broken when
    /// in fact it's perfectly valid syntax).
    ///
    /// Parent shape:  structured { findings: list<max=16> of {
    ///                  severity: u8(1), file: string(128),
    ///                  message: string(512) } }
    ///   Item product:  1 × 128 × 512 = 65,536
    ///   List × items:  65,536 × 16 = 1,048,576 (1 MiB) — fits.
    ///
    /// Child adds `category: string(32)`:
    ///   Item product:  1 × 128 × 512 × 32 = 2,097,152
    ///   List × items:  2,097,152 × 16 = 33,554,432 (32 MiB) — exceeds.
    #[test]
    fn refine_surfaces_capacity_exceeded_for_structured_field_addition() {
        let parent = r#"{
            "content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1",
            "effects":["llm"],
            "input_shape":{"type":"u8","max_bytes":1},
            "name":"capacity-demo",
            "output_shape":{"type":"structured","fields":[
              {"name":"findings","shape":{"type":"list","max_items":16,"item":{"type":"structured","fields":[
                {"name":"severity","shape":{"type":"u8","max_bytes":1}},
                {"name":"file","shape":{"type":"string","max_bytes":128}},
                {"name":"message","shape":{"type":"string","max_bytes":512}}
              ]}}}
            ]},
            "references":[],
            "version":"0.1.0"
        }"#;
        let child = r#"{
            "content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1",
            "effects":["llm"],
            "input_shape":{"type":"u8","max_bytes":1},
            "name":"capacity-demo",
            "output_shape":{"type":"structured","fields":[
              {"name":"findings","shape":{"type":"list","max_items":16,"item":{"type":"structured","fields":[
                {"name":"severity","shape":{"type":"u8","max_bytes":1}},
                {"name":"file","shape":{"type":"string","max_bytes":128}},
                {"name":"message","shape":{"type":"string","max_bytes":512}},
                {"name":"category","shape":{"type":"string","max_bytes":32}}
              ]}}}
            ]},
            "references":[],
            "version":"0.1.1"
        }"#;
        match check_refine(parent, child).unwrap() {
            RefineResult::Rollback { rule_fired, reason } => {
                assert_eq!(
                    rule_fired, "CapacityExceeded",
                    "structured field-addition that exceeds capacity must surface as \
                     Rollback{{rule_fired: CapacityExceeded}}, NOT MalformedDescriptor",
                );
                assert!(
                    reason.contains("shape capacity") && reason.contains("exceeds"),
                    "reason should describe the capacity overflow: {reason}",
                );
            }
            other => panic!(
                "expected Rollback with rule_fired=CapacityExceeded, got {other:?} \
                 — the capacity check is masking the refinement path",
            ),
        }
    }

    #[test]
    fn evolve_names_each_rule() {
        let parent = r#"{"content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","effects":["none"],"input_shape":{"type":"u8","max_bytes":1},"name":"a","output_shape":{"type":"u8","max_bytes":1},"references":[],"version":"1.0.0"}"#;
        let child  = r#"{"content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","effects":["none"],"input_shape":{"type":"u8","max_bytes":1},"name":"b","output_shape":{"type":"u8","max_bytes":1},"references":[],"version":"1.0.1"}"#;
        match check_refine(parent, child).unwrap() {
            RefineResult::Rollback { rule_fired, .. } => assert_eq!(rule_fired, "R1_NameChanged"),
            _ => panic!(),
        }
    }

    #[test]
    fn compose_pair_compatible() {
        let s = r#"{"content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","effects":["none"],"input_shape":{"type":"u8","max_bytes":1},"name":"s","output_shape":{"type":"u8","max_bytes":1},"references":[],"version":"1.0.0"}"#;
        match check_compose(&[s, s]).unwrap() {
            ComposeResult::Compatible { .. } => {}
            other => panic!("expected compatible, got {other:?}"),
        }
    }

    #[test]
    fn compose_pair_incompatible_on_type_mismatch() {
        let producer = r#"{"content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","effects":["none"],"input_shape":{"type":"u8","max_bytes":1},"name":"p","output_shape":{"type":"u8","max_bytes":1},"references":[],"version":"1.0.0"}"#;
        let consumer = r#"{"content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","effects":["none"],"input_shape":{"type":"string","max_bytes":1},"name":"c","output_shape":{"type":"u8","max_bytes":1},"references":[],"version":"1.0.0"}"#;
        match check_compose(&[producer, consumer]).unwrap() {
            ComposeResult::Incompatible { at_step, .. } => assert_eq!(at_step, 0),
            other => panic!("expected incompatible, got {other:?}"),
        }
    }

    #[test]
    fn compose_chain_intact() {
        let s = r#"{"content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","effects":["none"],"input_shape":{"type":"u8","max_bytes":1},"name":"s","output_shape":{"type":"u8","max_bytes":1},"references":[],"version":"1.0.0"}"#;
        match check_compose(&[s, s, s, s]).unwrap() {
            ComposeResult::Compatible { .. } => {}
            other => panic!("expected compatible, got {other:?}"),
        }
    }

    #[test]
    fn compose_chain_breaks_at_first_incompatibility() {
        let ok = r#"{"content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","effects":["none"],"input_shape":{"type":"u8","max_bytes":1},"name":"s","output_shape":{"type":"u8","max_bytes":1},"references":[],"version":"1.0.0"}"#;
        let bad = r#"{"content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","effects":["none"],"input_shape":{"type":"string","max_bytes":1},"name":"b","output_shape":{"type":"u8","max_bytes":1},"references":[],"version":"1.0.0"}"#;
        match check_compose(&[ok, ok, bad, ok]).unwrap() {
            ComposeResult::Incompatible { at_step, .. } => assert_eq!(at_step, 1),
            other => panic!("expected incompatible, got {other:?}"),
        }
    }

    #[test]
    fn compose_errors_on_singleton() {
        let s = r#"{"content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","effects":["none"],"input_shape":{"type":"u8","max_bytes":1},"name":"s","output_shape":{"type":"u8","max_bytes":1},"references":[],"version":"1.0.0"}"#;
        assert!(check_compose(&[s]).is_err());
    }
}
