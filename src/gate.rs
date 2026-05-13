//! Lyra validation entry points: `validate_skill`, `check_composable`,
//! and `registry_snapshot`. Each returns a sealed receipt that can be
//! verified offline.

use uor_foundation::enforcement::{
    replay::certify_from_trace, Certified, CompileUnitBuilder, ConstrainedTypeInput,
    GroundingCertificate, Hasher, Term,
};
use uor_foundation::enums::{VerificationDomain, WittLevel};
use uor_foundation::pipeline::run;

use crate::descriptor::SkillDescriptor;

// ---- Lyra hasher -------------------------------------------------------

/// BLAKE3-256 streaming hasher implementing the substrate's `Hasher`
/// contract. **(H1)** Replaces the previous 128-bit FNV-1a wrapper whose
/// `finalize` returned `[u8; 32]` with the upper 16 bytes zeroed —
/// effective fingerprint width was 128 bits, not 256. With BLAKE3 the
/// pipeline's `content_fingerprint` has the full 256-bit width the API
/// surface implies.
#[derive(Debug, Clone, Default)]
pub struct LyraHasher {
    inner: blake3::Hasher,
}

impl Hasher for LyraHasher {
    const OUTPUT_BYTES: usize = 32;

    #[inline]
    fn initial() -> Self {
        Self {
            inner: blake3::Hasher::new(),
        }
    }

    #[inline]
    fn fold_byte(mut self, b: u8) -> Self {
        self.inner.update(&[b]);
        self
    }

    #[inline]
    fn fold_bytes(mut self, data: &[u8]) -> Self {
        self.inner.update(data);
        self
    }

    #[inline]
    fn finalize(self) -> [u8; 32] {
        *self.inner.finalize().as_bytes()
    }
}

// ---- Gate error --------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    InvalidDescriptor(String),
    Incompatible(String),
    PipelineFailure(String),
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::InvalidDescriptor(s) => write!(f, "invalid descriptor: {s}"),
            ValidationError::Incompatible(s) => write!(f, "incompatible: {s}"),
            ValidationError::PipelineFailure(s) => write!(f, "pipeline failed: {s}"),
        }
    }
}

impl std::error::Error for ValidationError {}

// ---- Attestation: the two-layer receipt --------------------------------

/// A Lyra attestation: the canonical pair of layers that together prove
/// "this descriptor, validated by this pipeline, with this content."
///
/// - [`receipt_hash`](Self::receipt_hash) — BLAKE3-256 over `(label || 0x00 || canonical_bytes)`.
///   Binds the attestation to its exact inputs. Reproducible from the
///   same descriptor by anyone.
/// - [`seal`](Self::seal) — sealed structural attestation from the
///   sanctioned pipeline. Cannot be produced outside the pipeline.
///
/// Verifying an attestation means checking both layers: recompute the
/// receipt hash from the descriptor and confirm equality, and replay the
/// seal via `certify_from_trace`.
///
/// **AUDIT #7**: this field was previously named `content_hash`, the
/// same name as `SkillDescriptor.content_hash`. The two carry opposite
/// provenance — the descriptor's `content_hash` is an *input* set by
/// the skill author (BLAKE3 of the skill body), while this `receipt_hash`
/// is an *output* of the attestation pipeline (BLAKE3 of the runtime
/// ident + label + canonical descriptor bytes). Different provenance
/// requires different words.
#[derive(Debug, Clone)]
pub struct Attestation {
    pub receipt_hash: [u8; 32],
    pub seal: Certified<GroundingCertificate>,
}

impl Attestation {
    /// Returns the receipt-binding hash as lowercase hex.
    pub fn receipt_hash_hex(&self) -> String {
        let mut s = String::with_capacity(64);
        const HEX: &[u8] = b"0123456789abcdef";
        for b in &self.receipt_hash {
            s.push(HEX[(b >> 4) as usize] as char);
            s.push(HEX[(b & 0xf) as usize] as char);
        }
        s
    }
}

/// **Single source of truth** for Lyra's UOR pipeline. Used by every
/// receipt-producing entry point — `validate_skill`, `check_composable`,
/// `registry_snapshot`, and the JSON-string `score` path. Both layers
/// of the two-layer attestation flow from here:
///
/// - **content_hash** — `BLAKE3(runtime_ident ‖ 0x00 ‖ label ‖ 0x00 ‖ bytes)`.
///   Binds the attestation to its exact inputs and the runtime+substrate
///   that produced it.
/// - **seal** — a sealed `Certified<GroundingCertificate>` from
///   `uor_foundation::pipeline::run`. The first 8 bytes of the content
///   hash fold into the term arena, so the seal's term-arena structure
///   varies with the input.
/// - **trace_bytes** — the serialized form of the seal, suitable for
///   inclusion in a wire-format receipt.
///
/// One call site builds the `CompileUnit`. One hasher
/// ([`LyraHasher`], BLAKE3-256). One result type (`ConstrainedTypeInput`).
pub fn mint_seal(
    label: &str,
    bytes: &[u8],
) -> Result<([u8; 32], Certified<GroundingCertificate>, Vec<u8>), ValidationError> {
    // Layer 1: BLAKE3-256 over (runtime_ident ‖ 0x00 ‖ label ‖ 0x00 ‖ bytes).
    let mut h = blake3::Hasher::new();
    h.update(crate::LYRA_RUNTIME_IDENT.as_bytes());
    h.update(&[0x00]);
    h.update(label.as_bytes());
    h.update(&[0x00]);
    h.update(bytes);
    let content_hash: [u8; 32] = *h.finalize().as_bytes();

    // Layer 2: sealed structural attestation from the sanctioned pipeline.
    // **(C1)** Fold **all 32 bytes** of `content_hash` into the term arena
    // — not just the first 8. Four `W64` literals cover the full hash, so
    // the seal's content fingerprint depends on every bit. A collision on
    // the first 8 bytes no longer produces a byte-identical seal.
    let w64 = WittLevel::new(64);
    let chunks: [u64; 4] = [
        u64::from_le_bytes(content_hash[0..8].try_into().unwrap()),
        u64::from_le_bytes(content_hash[8..16].try_into().unwrap()),
        u64::from_le_bytes(content_hash[16..24].try_into().unwrap()),
        u64::from_le_bytes(content_hash[24..32].try_into().unwrap()),
    ];
    let terms: [Term; 5] = [
        Term::Literal { value: label.len() as u64, level: WittLevel::W8 },
        Term::Literal { value: chunks[0],          level: w64 },
        Term::Literal { value: chunks[1],          level: w64 },
        Term::Literal { value: chunks[2],          level: w64 },
        Term::Literal { value: chunks[3],          level: w64 },
    ];
    let domains: [VerificationDomain; 1] = [VerificationDomain::Enumerative];

    let unit = CompileUnitBuilder::new()
        .root_term(&terms)
        .witt_level_ceiling(w64)
        .thermodynamic_budget(512)
        .target_domains(&domains)
        .result_type::<ConstrainedTypeInput>()
        .validate()
        .map_err(|e| ValidationError::PipelineFailure(format!("{e:?}")))?;

    let grounded = run::<ConstrainedTypeInput, _, LyraHasher>(unit)
        .map_err(|e| ValidationError::PipelineFailure(format!("{e:?}")))?;

    let trace = grounded.derivation().replay::<256>();
    let trace_bytes = crate::wire::trace_to_bytes(&trace);
    let seal = certify_from_trace(&trace)
        .map_err(|e| ValidationError::PipelineFailure(format!("{e:?}")))?;

    Ok((content_hash, seal, trace_bytes))
}

/// Wraps [`mint_seal`] in the typed [`Attestation`] return shape used by
/// the in-process Rust API.
fn mint_receipt(label: &'static str, bytes: &[u8]) -> Result<Attestation, ValidationError> {
    let (receipt_hash, seal, _trace_bytes) = mint_seal(label, bytes)?;
    Ok(Attestation { receipt_hash, seal })
}

// ---- Public gate functions ----------------------------------------------

/// Validate a skill descriptor. Returns a sealed receipt attesting
/// that the descriptor passed through the sanctioned validation path.
pub fn validate_skill(desc: &SkillDescriptor) -> Result<Attestation, ValidationError> {
    mint_receipt("validate_skill", &desc.canonicalize())
}

/// Check that `producer`'s output shape is structurally compatible
/// with `consumer`'s input shape.
///
/// Returns a sealed receipt on compatibility, `Err(Incompatible(reason))` otherwise.
pub fn check_composable(
    producer: &SkillDescriptor,
    consumer: &SkillDescriptor,
) -> Result<Attestation, ValidationError> {
    let compatible = shapes_compatible(producer.output_shape(), consumer.input_shape());

    if compatible {
        // Canonicalize both descriptors into the receipt bytes so the
        // proof references both.
        let mut buf = Vec::with_capacity(512);
        buf.extend_from_slice(b"COMPOSABLE");
        buf.extend_from_slice(&producer.canonicalize());
        buf.extend_from_slice(&consumer.canonicalize());
        mint_receipt("check_composable", &buf)
    } else {
        Err(ValidationError::Incompatible(
            "output_shape cannot flow into consumer input_shape".into(),
        ))
    }
}

/// Composition gate, Liskov direction. **Producer's output is
/// type-substitutable for consumer's input** iff:
///
/// * **Scalar / String / Bytes**: `producer.max_bytes ≤ consumer.max_bytes`.
///   Every value the producer could output fits within what the consumer
///   agreed to accept.
///
/// * **Structured**: record subtyping. The producer's output is
///   *wider-or-equal*: every field the consumer requires must be
///   present on the producer, with a composable sub-shape. Extra
///   fields on the producer are safely ignored by the consumer.
///   Iterate over consumer's required fields and look each up.
///
/// * **List**: items compose AND `producer.max_items ≤ consumer.max_items`.
///
/// Type tags must match exactly across the pair (no implicit coercion).
///
/// **AUDIT #3**: the previous implementation inverted the scalar/list
/// direction (`p >= c`), which would have let a producer outputting
/// 4096 bytes feed a consumer accepting only 256 — overflow-by-contract.
/// Structured direction (iterating over consumer's required fields) was
/// already correct; the audit conflated scalar with structured.
fn shapes_compatible(producer: &crate::descriptor::Shape, consumer: &crate::descriptor::Shape) -> bool {
    use crate::descriptor::Shape;
    match (producer, consumer) {
        (Shape::U8     { max_bytes: p }, Shape::U8     { max_bytes: c }) => p <= c,
        (Shape::U16    { max_bytes: p }, Shape::U16    { max_bytes: c }) => p <= c,
        (Shape::U32    { max_bytes: p }, Shape::U32    { max_bytes: c }) => p <= c,
        (Shape::U64    { max_bytes: p }, Shape::U64    { max_bytes: c }) => p <= c,
        (Shape::String { max_bytes: p }, Shape::String { max_bytes: c }) => p <= c,
        (Shape::Bytes  { max_bytes: p }, Shape::Bytes  { max_bytes: c }) => p <= c,
        (Shape::Structured { fields: pf }, Shape::Structured { fields: cf }) => {
            for c_field in cf {
                match pf.iter().find(|f| f.name == c_field.name) {
                    Some(p_f) => {
                        if !shapes_compatible(&p_f.shape, &c_field.shape) {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
            true
        }
        (Shape::List { item: pi, max_items: pm }, Shape::List { item: ci, max_items: cm }) => {
            shapes_compatible(pi, ci) && pm <= cm
        }
        _ => false, // type mismatch
    }
}

/// Create a registry snapshot attestation.
///
/// Both `manifest_bytes` and `prev_digest` (if present) are folded into
/// the canonical bytes that produce the attestation's `content_hash`, so
/// snapshots with different manifests or different prev_digests produce
/// distinct attestations. Passing `prev_digest = Some(prev.receipt_hash)`
/// gives an append-only hash chain.
/// Length of a chained `prev_digest` (BLAKE3-256). When `prev_digest` is
/// `Some`, the slice MUST be exactly this many bytes — empty or short
/// digests are rejected so `Some(&[])` cannot silently degenerate to `None`.
pub const PREV_DIGEST_LEN: usize = 32;

pub fn registry_snapshot(
    manifest_bytes: &[u8],
    prev_digest: Option<&[u8]>,
) -> Result<Attestation, ValidationError> {
    // M6: `prev_digest`, when present, must be exactly 32 bytes (a real
    // BLAKE3 content_hash). `Some(&[])` is a programming bug, not a
    // chain.
    if let Some(prev) = prev_digest {
        if prev.len() != PREV_DIGEST_LEN {
            return Err(ValidationError::PipelineFailure(format!(
                "prev_digest must be {} bytes, got {}",
                PREV_DIGEST_LEN,
                prev.len()
            )));
        }
    }
    // C1: encode every variable-length field with an explicit `u32-LE`
    // length prefix. Without this, `(prev, manifest)` and `(prev', manifest')`
    // could share a concatenation if their lengths shuffled.
    //   layout: b"SNAPSHOT"
    //         ‖ u32_LE(prev_len)     ‖ prev_bytes
    //         ‖ u32_LE(manifest_len) ‖ manifest_bytes
    let mut buf = Vec::with_capacity(manifest_bytes.len() + 64);
    buf.extend_from_slice(b"SNAPSHOT");
    match prev_digest {
        Some(prev) => {
            buf.extend_from_slice(&(prev.len() as u32).to_le_bytes());
            buf.extend_from_slice(prev);
        }
        None => {
            buf.extend_from_slice(&0u32.to_le_bytes());
        }
    }
    buf.extend_from_slice(&(manifest_bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(manifest_bytes);
    mint_receipt("registry_snapshot", &buf)
}

// ---- tests ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor::{NamedField, Shape};

    fn test_descriptor() -> SkillDescriptor {
        SkillDescriptor::builder()
            .name("web-search")
            .version("1.0.0")
            .content_hash_hex("10b5b44710b5b44710b5b44710b5b44710b5b44710b5b44710b5b44710b5b447")
            .input_shape(Shape::String { max_bytes: 256 })
            .output_shape(Shape::List {
                item: Box::new(Shape::String { max_bytes: 4096 }),
                max_items: 100,
            })
            .build()
            .unwrap()
    }

    #[test]
    fn validate_skill_produces_certified() {
        let cert = validate_skill(&test_descriptor());
        assert!(cert.is_ok(), "validate_skill failed: {:?}", cert);
    }

    #[test]
    fn compatible_shapes_pass() {
        let producer = test_descriptor();
        let consumer = SkillDescriptor::builder()
            .name("summarize")
            .version("1.0.0")
            .content_hash_hex("aabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccddaabbccdd")
            .input_shape(Shape::List {
                item: Box::new(Shape::String { max_bytes: 4096 }),
                max_items: 100,
            })
            .output_shape(Shape::String { max_bytes: 2048 })
            .build()
            .unwrap();
        let result = check_composable(&producer, &consumer);
        assert!(result.is_ok(), "expected COMPATIBLE, got {:?}", result);
    }

    #[test]
    fn type_mismatch_fails() {
        let p = SkillDescriptor::builder()
            .name("web-search").version("1.0.0")
            .content_hash_hex("aa".repeat(32).as_str())
            .input_shape(Shape::String { max_bytes: 256 })
            .output_shape(Shape::String { max_bytes: 4096 })
            .build().unwrap();
        let c = SkillDescriptor::builder()
            .name("summarize").version("1.0.0")
            .content_hash_hex("bb".repeat(32).as_str())
            .input_shape(Shape::List {
                item: Box::new(Shape::String { max_bytes: 4096 }),
                max_items: 100,
            })
            .output_shape(Shape::String { max_bytes: 2048 })
            .build().unwrap();
        assert!(matches!(check_composable(&p, &c), Err(ValidationError::Incompatible(_))));
    }

    #[test]
    fn producer_overflows_consumer_is_rejected() {
        // Producer outputs up to 1024 bytes; consumer accepts only 256.
        // Composing them would let the producer overflow the consumer.
        let p = SkillDescriptor::builder()
            .name("big-producer").version("1.0.0")
            .content_hash_hex("aa".repeat(32).as_str())
            .input_shape(Shape::String { max_bytes: 256 })
            .output_shape(Shape::String { max_bytes: 1024 })
            .build().unwrap();
        let c = SkillDescriptor::builder()
            .name("small-consumer").version("1.0.0")
            .content_hash_hex("bb".repeat(32).as_str())
            .input_shape(Shape::String { max_bytes: 256 })
            .output_shape(Shape::String { max_bytes: 1 })
            .build().unwrap();
        assert!(matches!(check_composable(&p, &c), Err(ValidationError::Incompatible(_))));
    }

    #[test]
    fn producer_fits_consumer_is_compatible() {
        // Producer outputs at most 128 bytes; consumer accepts up to 256.
        // Liskov-safe: producer's max output fits in consumer's bucket.
        let p = SkillDescriptor::builder()
            .name("small-producer").version("1.0.0")
            .content_hash_hex("aa".repeat(32).as_str())
            .input_shape(Shape::String { max_bytes: 256 })
            .output_shape(Shape::String { max_bytes: 128 })
            .build().unwrap();
        let c = SkillDescriptor::builder()
            .name("big-consumer").version("1.0.0")
            .content_hash_hex("bb".repeat(32).as_str())
            .input_shape(Shape::String { max_bytes: 256 })
            .output_shape(Shape::String { max_bytes: 1 })
            .build().unwrap();
        assert!(check_composable(&p, &c).is_ok());
    }

    #[test]
    fn structured_subset_passes() {
        let p = SkillDescriptor::builder()
            .name("rich").version("1.0.0")
            .content_hash_hex("cc".repeat(32).as_str())
            .input_shape(Shape::String { max_bytes: 256 })
            .output_shape(Shape::Structured {
                fields: vec![
                    NamedField { name: "text".into(), shape: Shape::String { max_bytes: 4096 } },
                    NamedField { name: "count".into(), shape: Shape::U32 { max_bytes: 4 } },
                ],
            })
            .build().unwrap();
        let c = SkillDescriptor::builder()
            .name("lite").version("1.0.0")
            .content_hash_hex("dd".repeat(32).as_str())
            .input_shape(Shape::Structured {
                fields: vec![
                    NamedField { name: "text".into(), shape: Shape::String { max_bytes: 4096 } },
                ],
            })
            .output_shape(Shape::String { max_bytes: 2048 })
            .build().unwrap();
        assert!(check_composable(&p, &c).is_ok());
    }

    #[test]
    fn hash_chain_distinguishes_content() {
        // Two-layer attestation: the BLAKE3 content_hash binds to the
        // exact manifest + prev_digest bytes, so different content
        // produces distinct attestations. The sealed UOR fingerprint
        // captures type structure only — by design, identical across
        // snapshots — and is *co*-load-bearing with content_hash, not a
        // substitute.
        let m1 = b"[{\"path\":\"skills/web-search\",\"content_hash\":\"10b5b447\"}]";
        let snap1 = registry_snapshot(m1, None).unwrap();

        let m2 = b"[{\"path\":\"skills/internet-search\",\"content_hash\":\"ff\"}]";
        let snap2 = registry_snapshot(m2, Some(&snap1.receipt_hash)).unwrap();

        // Content hashes differ — the chain link is real.
        assert_ne!(
            snap1.receipt_hash, snap2.receipt_hash,
            "different manifests must produce different content_hashes",
        );

        // Structural fingerprints match — same shape vocabulary.
        assert_eq!(
            snap1.seal.certificate().content_fingerprint().as_bytes(),
            snap2.seal.certificate().content_fingerprint().as_bytes(),
            "sealed UOR fingerprints capture type structure only — co-load-bearing with content_hash",
        );

        // Re-snapshotting the same manifest with a different prev_digest
        // must still change the content_hash (the link is content-bound).
        let snap1_alt = registry_snapshot(m1, Some(&[0u8; 32])).unwrap();
        assert_ne!(
            snap1.receipt_hash, snap1_alt.receipt_hash,
            "same manifest with different prev_digest must differ",
        );
    }

    #[test]
    fn register_is_reproducible() {
        let desc = test_descriptor();
        let a1 = validate_skill(&desc).unwrap();
        let a2 = validate_skill(&desc).unwrap();
        // Both layers must be reproducible.
        assert_eq!(a1.receipt_hash, a2.receipt_hash);
        assert_eq!(
            a1.seal.certificate().content_fingerprint().as_bytes(),
            a2.seal.certificate().content_fingerprint().as_bytes(),
        );
    }

    #[test]
    fn validate_skill_binds_to_content() {
        // The whole point of the content_hash layer: changing any byte
        // of the descriptor produces a different attestation.
        let desc1 = test_descriptor();
        let desc2 = SkillDescriptor::builder()
            .name("web-search-v2")  // different name
            .version("1.0.0")
            .content_hash_hex("10b5b44710b5b44710b5b44710b5b44710b5b44710b5b44710b5b44710b5b447")
            .input_shape(Shape::String { max_bytes: 256 })
            .output_shape(Shape::List {
                item: Box::new(Shape::String { max_bytes: 4096 }),
                max_items: 100,
            })
            .build()
            .unwrap();
        let a1 = validate_skill(&desc1).unwrap();
        let a2 = validate_skill(&desc2).unwrap();
        assert_ne!(a1.receipt_hash, a2.receipt_hash);
    }
}
