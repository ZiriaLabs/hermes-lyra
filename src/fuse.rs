//! Skill fusion — atomic, type-safe composition of two skills into one.
//!
//! `fuse(s1, s2)` mints a new SKILL.md whose contract is the categorical
//! composition of the parents:
//!
//! ```text
//!   input_shape  ← s1.input_shape           (precondition propagates from head)
//!   output_shape ← s2.output_shape          (postcondition propagates from tail)
//!   effects      ← sort(unique(s1 ∪ s2))    (effect union — strictly monotone)
//!   references   ← [name@hash, name@hash]   (parents pinned by content_hash)
//!   content_hash ← BLAKE3("FUSED" 0x00 h1 0x00 h2)
//!   version      ← "0.1.0"                  (fresh genesis; lineage is in references)
//! ```
//!
//! ## Why this is sound
//!
//! `gate::check_composable(s1, s2)` is a precondition. It guarantees
//! `s1.output_shape ⊑ s2.input_shape` under Lyra's Liskov rules. Given
//! that, the fused contract is the *categorical composition* of the two
//! parent contracts: a caller who satisfies `s1.input_shape` is, by
//! transitivity, satisfying everything the chain needs; the chain
//! produces `s2.output_shape`, which is exactly the fused output.
//!
//! ## Determinism
//!
//! The content_hash is derived only from the parents' content_hashes,
//! so `fuse(s1, s2)` is byte-identical across processes and runs. The
//! produced SKILL.md is byte-identical when `skill_md` is None
//! (scaffold path); when `skill_md` is provided, the fence content is
//! identical and the surrounding prose is preserved.
//!
//! ## What this gate does NOT do
//!
//! * It does not invoke either parent's *implementation*. Lyra never
//!   ran code; it certifies typed contracts. Implementation correctness
//!   of the fused skill is the implementer's responsibility.
//! * It is not a refinement. R5 (`effects ⊆ parent`) is generally
//!   violated under fusion: `effects = s1 ∪ s2` widens. Fusion is a
//!   *sideways* relation, not a downward one. Use `skill_refine` for
//!   parent→child; use `skill_merge` for sibling→composite.

use crate::descriptor::{EffectKind, SkillDescriptor};

// =========================================================================
// Public API
// =========================================================================

/// Outcome of a `fuse` call. **Every** failure mode produces a
/// typed variant — callers never need to parse a bare error string.
/// The `status` field on the wire is distinct from `certify`'s
/// (`fused` vs `certified`) so callers can tell a fused composite
/// skill apart from a directly-bound one without inspecting parents.
#[derive(Debug, Clone)]
pub enum FuseResult {
    /// Fusion succeeded. The new SKILL.md is fully certified and
    /// self-verifying. `descriptor_json` is the canonical JSON form
    /// of the fused descriptor (also embedded in `skill_md`).
    Fused {
        skill_md: String,
        descriptor_json: String,
        proof: crate::bridge::EmbeddedProof,
        /// Sorted, deduplicated union of effects from the two parents,
        /// in canonical wire form (e.g. `["llm","web_read"]`).
        /// Lifted to the top level so an agent can route on capability
        /// requirements without re-parsing `descriptor_json`. Equivalent
        /// to the `effects` array inside the fused descriptor.
        effects_union: Vec<String>,
    },
    /// The two skills are not type-composable. `at_step` is always 0
    /// for a pair (the producer is index 0, consumer is index 1).
    Incompatible { at_step: usize, reason: String },
    /// Producer or consumer descriptor did not parse (or, after fusion,
    /// the *fused* descriptor failed format-level validation). `which`
    /// is `"producer"`, `"consumer"`, or `"fused"`.
    MalformedDescriptor { which: String, reason: String },
    /// Parents fused successfully at the type-shape level but the
    /// resulting structured product exceeds the 16 MiB universal cap.
    /// Distinct from `MalformedDescriptor` so an agent routes it like
    /// a refinement rollback rather than a format error.
    CapacityExceeded { reason: String },
}

impl FuseResult {
    /// JSON wire form — the same shape the MCP tool emits.
    pub fn to_json(&self) -> String {
        match self {
            FuseResult::Fused { skill_md, descriptor_json, proof, effects_union } => {
                let mut effs = String::with_capacity(64);
                effs.push('[');
                for (i, e) in effects_union.iter().enumerate() {
                    if i > 0 { effs.push(','); }
                    effs.push('"');
                    effs.push_str(&json_escape(e));
                    effs.push('"');
                }
                effs.push(']');
                format!(
                    r#"{{"status":"fused","skill_md":"{}","descriptor":{},"effects_union":{},"proof":{{"output_hash":"{}","runtime":"{}"}}}}"#,
                    json_escape(skill_md),
                    descriptor_json,
                    effs,
                    proof.output_hash,
                    json_escape(&proof.runtime),
                )
            }
            FuseResult::Incompatible { at_step, reason } => format!(
                r#"{{"status":"incompatible","at_step":{at_step},"reason":"{}"}}"#,
                json_escape(reason),
            ),
            FuseResult::MalformedDescriptor { which, reason } => format!(
                r#"{{"status":"malformed_descriptor","which":"{}","reason":"{}"}}"#,
                json_escape(which),
                json_escape(reason),
            ),
            FuseResult::CapacityExceeded { reason } => format!(
                r#"{{"status":"capacity_exceeded","reason":"{}"}}"#,
                json_escape(reason),
            ),
        }
    }
}

/// Fuse two skills into a new self-contained, certified SKILL.md.
///
/// `producer_input` and `consumer_input` may each be SKILL.md text or
/// raw descriptor JSON — auto-detected via `bridge::descriptor_from_anywhere`.
///
/// `name_override`: caller-supplied name for the fused skill. When
/// `None`, the name is auto-derived. The auto-derivation is
/// deterministic (so the same parents always produce the same fused
/// skill) and always passes Lyra's name validator:
///   * If `"<p_name>-<c_name>"` ≤ 64 chars and is a legal name, use it.
///   * Otherwise, use `"fused-<8-hex-of-h1||h2>"`, which is always 14
///     chars and always valid.
///
/// `existing_md`: optional SKILL.md prose to preserve. When `None`,
/// a minimal scaffold is emitted. When provided, its body is upgraded
/// in place and the lyra fence is replaced with the fused descriptor.
pub fn fuse_skills(
    producer_input: &str,
    consumer_input: &str,
    name_override: Option<&str>,
    existing_md: Option<&str>,
) -> Result<FuseResult, String> {
    // 1. Lift both inputs to descriptor JSON, then to typed descriptors.
    //    Malformed input → typed Result variant, not Err. Callers can
    //    distinguish "wrong descriptor" from "wrong composition" by status.
    let producer_json = match crate::bridge::descriptor_from_anywhere(producer_input) {
        Ok(s) => s,
        Err(e) => return Ok(FuseResult::MalformedDescriptor {
            which: "producer".into(), reason: e,
        }),
    };
    let consumer_json = match crate::bridge::descriptor_from_anywhere(consumer_input) {
        Ok(s) => s,
        Err(e) => return Ok(FuseResult::MalformedDescriptor {
            which: "consumer".into(), reason: e,
        }),
    };
    let producer = match crate::computations::descriptor_from_json(&producer_json) {
        Ok(d) => d,
        Err(e) => return Ok(FuseResult::MalformedDescriptor {
            which: "producer".into(), reason: e,
        }),
    };
    let consumer = match crate::computations::descriptor_from_json(&consumer_json) {
        Ok(d) => d,
        Err(e) => return Ok(FuseResult::MalformedDescriptor {
            which: "consumer".into(), reason: e,
        }),
    };

    // 2. Type-check at the gate. If incompatible, return the structured
    //    Incompatible variant rather than an ad-hoc error string.
    match crate::gate::check_composable(&producer, &consumer) {
        Ok(_) => {}
        Err(crate::gate::ValidationError::Incompatible(reason)) => {
            return Ok(FuseResult::Incompatible { at_step: 0, reason });
        }
        Err(other) => return Err(format!("compose check: {other}")),
    }

    // 3. Derive the fused descriptor's identity fields.
    let fused_name = derive_fused_name(name_override, &producer, &consumer)?;
    let fused_hash = derive_fused_content_hash(&producer, &consumer);

    // 4. Build the canonical JSON form of the fused descriptor. Key
    //    order is alphabetical — matches Lyra's existing canonical
    //    examples and keeps `version` last (the parser's last-key
    //    sensitivity is not relevant when we control serialization,
    //    but consistency matters for anyone reading the file).
    let fused_json = build_fused_descriptor_json(
        &fused_hash,
        &fused_name,
        &producer,
        &consumer,
    );

    // 5. Re-validate by round-tripping through the typed parser. Two
    //    failure modes get distinct typed outcomes (audit fix):
    //      * Capacity overflow — parents fit individually but their
    //        union's structured product exceeds the 16 MiB cap.
    //        Surface as `FuseResult::CapacityExceeded` so agents
    //        route it like a refinement rollback, not a format error.
    //      * Other validation failure — surface as MalformedDescriptor
    //        (in practice unreachable for fuse since both parents
    //        already validated).
    let _validated: SkillDescriptor = match crate::computations::descriptor_from_json(&fused_json) {
        Ok(d) => d,
        Err(e) if crate::descriptor::is_capacity_exceeded_error(&e) => {
            return Ok(FuseResult::CapacityExceeded { reason: e });
        }
        Err(e) => {
            return Ok(FuseResult::MalformedDescriptor {
                which: "fused".into(),
                reason: e,
            });
        }
    };

    // 6. Bind to a SKILL.md (caller-supplied or scaffold). bind_descriptor_to_md
    //    handles both append and replace-existing-fence cases.
    let md = match existing_md {
        Some(s) => s.to_string(),
        None => crate::bridge::scaffold_md_from_descriptor(&fused_json)
            .map_err(|e| format!("scaffold: {e}"))?,
    };
    let (upgraded, proof) = crate::bridge::bind_descriptor_to_md(&md, &fused_json)
        .map_err(|e| format!("bind: {e}"))?;
    // bind_descriptor_to_md rewrites the descriptor's content_hash to
    // BLAKE3 of the bound SKILL.md body (so the proof transitively
    // attests the body). Pull the rewritten descriptor back out so the
    // FuseResult exposes the *bound* form — what verify will see — not
    // the pre-bind form we passed in.
    let fused_json = crate::bridge::extract_frontmatter_contract(&upgraded)
        .ok_or_else(|| "bind: no contract in frontmatter after bind".to_string())?;

    let effects_union: Vec<String> = {
        let mut all: Vec<EffectKind> = Vec::with_capacity(
            producer.effects().len() + consumer.effects().len(),
        );
        for e in producer.effects().iter().chain(consumer.effects().iter()) {
            if *e == EffectKind::None { continue; }
            if !all.contains(e) { all.push(*e); }
        }
        all.sort_by_key(|e| effect_code(*e));
        all.into_iter().map(|e| effect_str(e).to_string()).collect()
    };

    Ok(FuseResult::Fused {
        skill_md: upgraded,
        descriptor_json: fused_json,
        proof,
        effects_union,
    })
}

// =========================================================================
// Internals
// =========================================================================

/// `BLAKE3("FUSED" 0x00 producer_hash[32] 0x00 consumer_hash[32])`.
///
/// Mirrors the `EVOLVED || ph || ch` construction used by
/// `next_generation_check`, with a distinct domain tag so a fused hash
/// can never collide with an evolved hash even if the input bytes line
/// up by accident.
fn derive_fused_content_hash(p: &SkillDescriptor, c: &SkillDescriptor) -> String {
    let mut h = blake3::Hasher::new();
    h.update(b"FUSED");
    h.update(&[0x00]);
    h.update(p.content_hash());
    h.update(&[0x00]);
    h.update(c.content_hash());
    let bytes = *h.finalize().as_bytes();
    hex_encode_32(&bytes)
}

/// Resolve the fused name. Three-step rule:
///   1. If `override_` is provided, use it (after validation).
///   2. Else try `"<p>-<c>"`; accept if it passes `validate_name`.
///   3. Else fall back to `"fused-<hash[..8]>"` (always valid).
fn derive_fused_name(
    override_: Option<&str>,
    p: &SkillDescriptor,
    c: &SkillDescriptor,
) -> Result<String, String> {
    use crate::descriptor::validate_name;

    if let Some(n) = override_ {
        validate_name(n).map_err(|e| format!("name override invalid: {e}"))?;
        return Ok(n.to_string());
    }

    let joined = format!("{}-{}", p.name(), c.name());
    if validate_name(&joined).is_ok() {
        return Ok(joined);
    }

    // Fallback. Take the first 8 hex chars of the fused hash (which
    // we recompute here — cheap, single BLAKE3 call). Always 14 chars,
    // always a-z0-9 with one hyphen, always valid.
    let h = derive_fused_content_hash(p, c);
    Ok(format!("fused-{}", &h[..8]))
}

/// Build the canonical JSON for the fused descriptor. Alphabetical key
/// order, no whitespace between tokens — matches the compact form used
/// elsewhere in the codebase (e.g. tripwire's `next_generation_input`).
fn build_fused_descriptor_json(
    content_hash_hex: &str,
    name: &str,
    p: &SkillDescriptor,
    c: &SkillDescriptor,
) -> String {
    let mut out = String::with_capacity(512);
    out.push('{');
    out.push_str(&format!(r#""content_hash":"{content_hash_hex}""#));
    out.push(',');
    out.push_str(r#""effects":"#);
    out.push_str(&effects_json_array(p, c));
    out.push(',');
    out.push_str(r#""input_shape":"#);
    out.push_str(&shape_to_json(p.input_shape()));
    out.push(',');
    out.push_str(&format!(r#""name":"{}""#, json_escape(name)));
    out.push(',');
    out.push_str(r#""output_shape":"#);
    out.push_str(&shape_to_json(c.output_shape()));
    out.push(',');
    out.push_str(r#""references":"#);
    out.push_str(&references_json_array(p, c));
    out.push(',');
    out.push_str(&format!(r#""version":"0.1.0""#));
    out.push('}');
    out
}

/// Sorted, deduplicated union of effects. `EffectKind::None` is
/// dropped because the descriptor builder strips it (see
/// `descriptor.rs:277`); leaving it in would produce a descriptor
/// whose typed re-parse no longer matches the JSON.
fn effects_json_array(p: &SkillDescriptor, c: &SkillDescriptor) -> String {
    let mut all: Vec<EffectKind> = Vec::with_capacity(p.effects().len() + c.effects().len());
    for e in p.effects().iter().chain(c.effects().iter()) {
        if *e == EffectKind::None { continue; }
        if !all.contains(e) { all.push(*e); }
    }
    // Stable order: by the existing effect_code() ordering. This is
    // deterministic across runs and platforms.
    all.sort_by_key(|e| effect_code(*e));
    let mut out = String::with_capacity(64);
    out.push('[');
    for (i, e) in all.iter().enumerate() {
        if i > 0 { out.push(','); }
        out.push_str(&format!(r#""{}""#, effect_str(*e)));
    }
    out.push(']');
    out
}

/// References as `["name@hash", "name@hash"]`. Order is producer then
/// consumer — preserves the *direction* of the fusion, which a sorter
/// would erase. Re-fusing the same pair in the other direction
/// produces a different reference order and (because the content_hash
/// also includes ordering) a different content_hash, which is
/// correct: `fuse(a,b) ≠ fuse(b,a)` semantically.
fn references_json_array(p: &SkillDescriptor, c: &SkillDescriptor) -> String {
    let p_ref = format!("{}@{}", p.name(), hex_encode_32(p.content_hash()));
    let c_ref = format!("{}@{}", c.name(), hex_encode_32(c.content_hash()));
    format!(r#"["{}","{}"]"#, p_ref, c_ref)
}

// -- shape serializer: typed Shape → canonical JSON --

fn shape_to_json(s: &crate::descriptor::Shape) -> String {
    use crate::descriptor::Shape;
    match s {
        Shape::U8     { max_bytes } => format!(r#"{{"type":"u8","max_bytes":{max_bytes}}}"#),
        Shape::U16    { max_bytes } => format!(r#"{{"type":"u16","max_bytes":{max_bytes}}}"#),
        Shape::U32    { max_bytes } => format!(r#"{{"type":"u32","max_bytes":{max_bytes}}}"#),
        Shape::U64    { max_bytes } => format!(r#"{{"type":"u64","max_bytes":{max_bytes}}}"#),
        Shape::String { max_bytes } => format!(r#"{{"type":"string","max_bytes":{max_bytes}}}"#),
        Shape::Bytes  { max_bytes } => format!(r#"{{"type":"bytes","max_bytes":{max_bytes}}}"#),
        Shape::Structured { fields } => {
            let mut out = String::with_capacity(128);
            out.push_str(r#"{"type":"structured","fields":["#);
            for (i, f) in fields.iter().enumerate() {
                if i > 0 { out.push(','); }
                out.push_str(&format!(
                    r#"{{"name":"{}","shape":{}}}"#,
                    json_escape(&f.name),
                    shape_to_json(&f.shape),
                ));
            }
            out.push_str("]}");
            out
        }
        Shape::List { item, max_items } => format!(
            r#"{{"type":"list","item":{},"max_items":{max_items}}}"#,
            shape_to_json(item),
        ),
    }
}

// -- helpers (mirror the ones in descriptor.rs / jsonld.rs to avoid
//    pub-leaking those internals) --

fn effect_str(e: EffectKind) -> &'static str {
    match e {
        EffectKind::None         => "none",
        EffectKind::FileRead     => "file_read",
        EffectKind::FileWrite    => "file_write",
        EffectKind::WebRead  => "web_read",
        EffectKind::WebWrite => "web_write",
        EffectKind::Terminal => "terminal",
        EffectKind::Llm      => "llm",
    }
}

fn effect_code(e: EffectKind) -> u8 {
    match e {
        EffectKind::None         => 0,
        EffectKind::FileRead     => 1,
        EffectKind::FileWrite    => 2,
        EffectKind::WebRead  => 3,
        EffectKind::WebWrite => 4,
        EffectKind::Terminal => 5,
        EffectKind::Llm      => 6,
    }
}

fn hex_encode_32(bytes: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for b in bytes {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

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
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Two compose-compatible parents:
    //   producer: input {name:string<=64} → output {greeting:string<=256}
    //   consumer: input {greeting:string<=256} → output {ok:u8<=1}, effect=llm
    const PRODUCER: &str = r#"{"content_hash":"a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0","effects":[],"input_shape":{"type":"structured","fields":[{"name":"name","shape":{"type":"string","max_bytes":64}}]},"name":"hermes-greet","output_shape":{"type":"structured","fields":[{"name":"greeting","shape":{"type":"string","max_bytes":256}}]},"references":[],"version":"0.1.0"}"#;
    const CONSUMER: &str = r#"{"content_hash":"b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0","effects":["llm"],"input_shape":{"type":"structured","fields":[{"name":"greeting","shape":{"type":"string","max_bytes":256}}]},"name":"greeting-printer","output_shape":{"type":"structured","fields":[{"name":"ok","shape":{"type":"u8","max_bytes":1}}]},"references":[],"version":"0.1.0"}"#;

    // Incompatible consumer: input expects {x:u32}, not {greeting:string}.
    const INCOMPAT: &str = r#"{"content_hash":"c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0","effects":[],"input_shape":{"type":"structured","fields":[{"name":"x","shape":{"type":"u32","max_bytes":4}}]},"name":"number-cruncher","output_shape":{"type":"structured","fields":[{"name":"y","shape":{"type":"u32","max_bytes":4}}]},"references":[],"version":"0.1.0"}"#;

    fn fuse_ok(p: &str, c: &str) -> (String, String, crate::bridge::EmbeddedProof) {
        match fuse_skills(p, c, None, None).expect("fuse should succeed") {
            FuseResult::Fused { skill_md, descriptor_json, proof, .. } =>
                (skill_md, descriptor_json, proof),
            other => panic!("expected Fused, got {other:?}"),
        }
    }

    #[test]
    fn fuse_compatible_pair_succeeds() {
        let (md, json, proof) = fuse_ok(PRODUCER, CONSUMER);
        // SKILL.md must carry contract: + proof: in the frontmatter,
        // and the contract value must equal the returned descriptor JSON.
        assert!(md.contains("\ncontract: "), "missing contract: key: {md}");
        assert!(md.contains("\nproof:"), "missing proof: key: {md}");
        assert!(md.contains(&json), "contract value must equal fused descriptor JSON");
        assert_eq!(proof.output_hash.len(), 64);
    }

    #[test]
    fn fuse_propagates_input_and_output_shapes() {
        let (_, json, _) = fuse_ok(PRODUCER, CONSUMER);
        // Producer's input_shape lives at the head of the fused descriptor.
        assert!(json.contains(r#""input_shape":{"type":"structured","fields":[{"name":"name","shape":{"type":"string","max_bytes":64}}]}"#),
                "input_shape mismatch in: {json}");
        // Consumer's output_shape lives at the tail.
        assert!(json.contains(r#""output_shape":{"type":"structured","fields":[{"name":"ok","shape":{"type":"u8","max_bytes":1}}]}"#),
                "output_shape mismatch in: {json}");
    }

    #[test]
    fn fuse_unions_effects() {
        let (_, json, _) = fuse_ok(PRODUCER, CONSUMER);
        // Producer has no effects; consumer has [llm]. Union is [llm].
        assert!(json.contains(r#""effects":["llm"]"#),
                "effects mismatch in: {json}");
    }

    #[test]
    fn fuse_exposes_effects_union_at_top_level() {
        // The Fused variant carries the union as a typed Vec<String>...
        let result = fuse_skills(PRODUCER, CONSUMER, None, None).expect("fuse");
        match &result {
            FuseResult::Fused { effects_union, .. } => {
                assert_eq!(effects_union, &vec!["llm".to_string()]);
            }
            other => panic!("expected Fused, got {other:?}"),
        }
        // ...and the wire form lifts it to a top-level field so agents
        // don't need to re-parse the embedded descriptor.
        let wire = result.to_json();
        assert!(
            wire.contains(r#""effects_union":["llm"]"#),
            "effects_union not on wire: {wire}",
        );
    }

    #[test]
    fn fuse_pins_parents_in_references() {
        let (_, json, _) = fuse_ok(PRODUCER, CONSUMER);
        // Both parents must appear as <name>@<64-hex>.
        assert!(json.contains(r#""hermes-greet@a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0""#));
        assert!(json.contains(r#""greeting-printer@b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0""#));
    }

    #[test]
    fn fuse_is_deterministic() {
        let (md_a, json_a, proof_a) = fuse_ok(PRODUCER, CONSUMER);
        let (md_b, json_b, proof_b) = fuse_ok(PRODUCER, CONSUMER);
        assert_eq!(md_a, md_b);
        assert_eq!(json_a, json_b);
        assert_eq!(proof_a.output_hash, proof_b.output_hash);
    }

    #[test]
    fn fuse_is_directional_not_commutative() {
        // fuse(a,b) and fuse(b,a) — second one will fail to type-check
        // (b's output {ok:u8} is not consumable as a's input {name:string}),
        // proving direction matters at the type level.
        match fuse_skills(CONSUMER, PRODUCER, None, None).unwrap() {
            FuseResult::Incompatible { at_step, .. } => assert_eq!(at_step, 0),
            other => panic!("fuse(b,a) must produce Incompatible, got {other:?}"),
        }
    }

    #[test]
    fn fuse_incompatible_returns_structured_failure() {
        match fuse_skills(PRODUCER, INCOMPAT, None, None).unwrap() {
            FuseResult::Incompatible { at_step, reason } => {
                assert_eq!(at_step, 0);
                assert!(reason.contains("output_shape"));
            }
            other => panic!("must reject incompatible pair, got {other:?}"),
        }
    }

    #[test]
    fn fuse_uses_override_name_when_valid() {
        match fuse_skills(PRODUCER, CONSUMER, Some("custom-pipeline"), None).unwrap() {
            FuseResult::Fused { descriptor_json, .. } => {
                assert!(descriptor_json.contains(r#""name":"custom-pipeline""#));
            }
            other => panic!("must succeed, got {other:?}"),
        }
    }

    #[test]
    fn fuse_rejects_invalid_override_name() {
        // Capital letters are not allowed by validate_name.
        let err = fuse_skills(PRODUCER, CONSUMER, Some("BadName"), None).unwrap_err();
        assert!(err.contains("name override invalid"), "got: {err}");
    }

    #[test]
    fn fuse_auto_derives_default_name() {
        let (_, json, _) = fuse_ok(PRODUCER, CONSUMER);
        // "hermes-greet-greeting-printer" is 29 chars, all lowercase, valid → joined form.
        assert!(json.contains(r#""name":"hermes-greet-greeting-printer""#),
                "expected joined name in: {json}");
    }

    #[test]
    fn fuse_falls_back_to_hash_name_when_joined_too_long() {
        // Build two parents with very long valid names that combine > 64 chars.
        let long_name = "a".repeat(40);
        let p = PRODUCER.replace(r#""name":"hermes-greet""#, &format!(r#""name":"{long_name}""#));
        let c = CONSUMER.replace(r#""name":"greeting-printer""#, &format!(r#""name":"{long_name}""#));
        let (_, json, _) = fuse_ok(&p, &c);
        // Joined would be 81 chars → must use fused-XXXXXXXX (14 chars).
        assert!(json.contains(r#""name":"fused-"#), "expected fused- prefix in: {json}");
    }

    #[test]
    fn fuse_round_trips_through_descriptor_parser() {
        // The fused descriptor JSON must be parseable back into a typed
        // SkillDescriptor — anything else means we produced a broken artifact.
        let (_, json, _) = fuse_ok(PRODUCER, CONSUMER);
        let parsed = crate::computations::descriptor_from_json(&json)
            .expect("fused descriptor must round-trip");
        assert_eq!(parsed.name(), "hermes-greet-greeting-printer");
        assert_eq!(parsed.version(), "0.1.0");
        assert_eq!(parsed.effects(), &[EffectKind::Llm]);
        assert_eq!(parsed.references().len(), 2);
    }

    #[test]
    fn fuse_md_verifies_against_certify_path() {
        // The fused SKILL.md must satisfy skill_verify — the headline
        // round-trip property: fuse → certify → verify is consistent.
        let (md, _, _) = fuse_ok(PRODUCER, CONSUMER);
        let outcome = crate::bridge::verify_embedded_proof(&md)
            .expect("verify must not error on a freshly fused MD");
        assert!(
            matches!(outcome, crate::bridge::VerifyOutcome::Valid { .. }),
            "verify must return Valid on fused MD; got {outcome:?}",
        );
    }

    #[test]
    fn fuse_content_hash_changes_with_parents() {
        // Mutate the producer's content_hash → fused hash must change.
        let p_alt = PRODUCER.replace(
            "a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0",
            "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
        );
        let (_, json_orig, _) = fuse_ok(PRODUCER, CONSUMER);
        let (_, json_alt, _) = fuse_ok(&p_alt, CONSUMER);
        assert_ne!(json_orig, json_alt, "fused hash must depend on producer hash");
    }
}
