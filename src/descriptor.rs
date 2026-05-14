//! `SkillDescriptor` — a sealed, bounded, canonicalizable skill interface descriptor.
//!
//! The descriptor carries the Lyra skill interface:
//! - name, version
//! - content_hash (blake3 of the skill body)
//! - input_shape, output_shape (typed Lyra shape enum)
//! - effects (declared side effects)
//! - references (other skill names this skill depends on)
//!
//! Construction is ONLY via the sealed builder. Bounds checks fire in
//! `build()` before any value exists, satisfying invariant I4.

// ---- Shape type vocabulary ----
// This enum mirrors the 8 Lyra shapes. Each variant captures the parameters
// the type needs (max_bytes for leaf types, inner shape for list, named
// fields for structured).

/// A Lyra shape descriptor. This is the *runtime representation* of what
/// the compile-time `ConstrainedTypeShape` types express.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Shape {
    U8 { max_bytes: u64 },
    U16 { max_bytes: u64 },
    U32 { max_bytes: u64 },
    U64 { max_bytes: u64 },
    String { max_bytes: u64 },
    Bytes { max_bytes: u64 },
    Structured { fields: Vec<NamedField> },
    List { item: Box<Shape>, max_items: u64 },
}

/// A single named field in a structured shape.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NamedField {
    pub name: String,
    pub shape: Shape,
}

/// A declared effect tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectKind {
    None,
    FileRead,
    FileWrite,
    WebRead,
    WebWrite,
    Terminal,
    Llm,
}

// ---- SkillDescriptor ----

/// A validated skill interface descriptor. Created ONLY via
/// `SkillDescriptorBuilder::build()`. Immutable once constructed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SkillDescriptor {
    /// CID of the schema this descriptor instantiates. v1: must equal
    /// [`crate::schema::LYRA_SKILL_SCHEMA_V1_CID`]. Anything else surfaces
    /// as `UnsupportedSchema` during build, and as `unsupported_schema`
    /// during envelope verification (typed outcome, not a hard error).
    schema: String,
    name: String,
    version: String,
    content_hash: [u8; 32],
    input_shape: Shape,
    output_shape: Shape,
    effects: Vec<EffectKind>,
    references: Vec<String>,
}

/// Errors that can occur during descriptor construction.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DescriptorBuildError {
    EmptyName,
    InvalidName(String),
    EmptyVersion,
    InvalidVersion(String),
    InvalidContentHash(String),
    ShapeValidationError(String),
    InvalidReference(String),
    /// Effect appeared more than once in the descriptor's `effects`
    /// list. Authors must dedupe at write time; the protocol no longer
    /// silently canonicalizes duplicates because the wire form would
    /// then differ from what the author actually wrote.
    DuplicateEffect(String),
    /// `none` (pure-function claim) appeared alongside one or more
    /// real effects. The previous behavior silently stripped `none` in
    /// that case, leaving the wire form ("we are pure AND we do X")
    /// disagreeing with the canonical attestation ("we do X"). Authors
    /// must pick one or the other.
    ContradictoryNoneEffect,
    /// The `schema` field is required and must equal a recognized
    /// schema CID. v1 recognizes only [`crate::schema::LYRA_SKILL_SCHEMA_V1_CID`].
    /// The wrapped string is the offending value (empty if the field
    /// was missing entirely).
    UnsupportedSchema(String),
}

impl core::fmt::Display for DescriptorBuildError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DescriptorBuildError::EmptyName => write!(f, "name must not be empty"),
            DescriptorBuildError::InvalidName(e) => write!(f, "invalid name: {e}"),
            DescriptorBuildError::EmptyVersion => write!(f, "version must not be empty"),
            DescriptorBuildError::InvalidVersion(e) => write!(f, "invalid version: {e}"),
            DescriptorBuildError::InvalidContentHash(e) => write!(f, "invalid content_hash: {e}"),
            DescriptorBuildError::ShapeValidationError(e) => write!(f, "shape validation: {e}"),
            DescriptorBuildError::InvalidReference(e) => write!(f, "invalid reference: {e}"),
            DescriptorBuildError::DuplicateEffect(e) => write!(f, "duplicate effect: {e}"),
            DescriptorBuildError::ContradictoryNoneEffect => write!(
                f, "effect `none` is incompatible with any other effect; \
                    declare `[none]` for a pure function or omit `none` \
                    when other effects are present",
            ),
            DescriptorBuildError::UnsupportedSchema(s) => {
                if s.is_empty() {
                    write!(
                        f,
                        "schema field is required; expected schema CID = {}",
                        crate::schema::LYRA_SKILL_SCHEMA_V1_CID
                    )
                } else {
                    write!(
                        f,
                        "unsupported schema {s:?}; this build recognizes only {}",
                        crate::schema::LYRA_SKILL_SCHEMA_V1_CID
                    )
                }
            }
        }
    }
}

/// Maximum length of a skill `name`. Matches the agentskills.io v0.1 limit.
pub const MAX_NAME_LENGTH: usize = 64;

/// Validate the skill name against the shared agentskills.io / Lyra v0.1 rules:
/// - 1..=64 ASCII characters
/// - lowercase letters `a-z`, digits `0-9`, or hyphen `-`
/// - must not start or end with a hyphen
/// - must not contain consecutive hyphens (`--`)
/// Validate a **content-addressed reference**. A reference is the envelope
/// CID of another SKILL.md — the same string `lyra cid` emits, the same
/// string a multiformats-compliant tool produces over the referenced
/// file's bytes. The CID *is* the reference; no name prefix.
///
/// Why no name?
/// - The name lives inside the referenced file's frontmatter. Resolving
///   a CID gets you back the name as a free byproduct, with no parallel
///   naming scheme to keep in sync.
/// - `name@<hash>` would be two identifiers for the same thing, two
///   things to validate, two things to forge. One CID is enough.
///
/// Strictness: delegates to `crate::cid::Cid::parse`, which rejects
/// CIDv0, wrong multibase prefixes, and malformed payloads.
pub fn validate_reference(r: &str) -> Result<(), DescriptorBuildError> {
    crate::cid::Cid::parse(r).map(|_| ()).map_err(|e| {
        DescriptorBuildError::InvalidReference(format!(
            "reference {r:?} is not a valid CIDv1: {e:?}"
        ))
    })
}

pub fn validate_name(name: &str) -> Result<(), DescriptorBuildError> {
    if name.is_empty() {
        return Err(DescriptorBuildError::EmptyName);
    }
    if name.len() > MAX_NAME_LENGTH {
        return Err(DescriptorBuildError::InvalidName(format!(
            "name length {} exceeds {} character limit",
            name.len(),
            MAX_NAME_LENGTH
        )));
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Err(DescriptorBuildError::InvalidName(
            "name must not start or end with a hyphen".into(),
        ));
    }
    if name.contains("--") {
        return Err(DescriptorBuildError::InvalidName(
            "name must not contain consecutive hyphens".into(),
        ));
    }
    if !name.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-')) {
        return Err(DescriptorBuildError::InvalidName(
            "name must contain only lowercase ASCII letters, digits, and hyphens".into(),
        ));
    }
    Ok(())
}

impl std::error::Error for DescriptorBuildError {}

// ---- Builder ----

pub struct SkillDescriptorBuilder {
    name: Option<String>,
    version: Option<String>,
    content_hash: Option<[u8; 32]>,
    content_hash_hex: Option<String>,
    /// Discriminated error from `content_hash_hex` (empty / wrong-length
    /// / non-hex). Surfaced at build time so the caller gets the
    /// actual diagnostic instead of the generic "content_hash is required".
    content_hash_error: Option<String>,
    input_shape: Option<Shape>,
    output_shape: Option<Shape>,
    effects: Vec<EffectKind>,
    references: Vec<String>,
    /// CID of the schema this descriptor instantiates. v1 builders default
    /// this to [`crate::schema::LYRA_SKILL_SCHEMA_V1_CID`] when the caller
    /// does not set it — the only currently-recognized value, so requiring
    /// it would just be ceremony for the common case. Anything *other*
    /// than the recognized CID is rejected at `build()` with
    /// [`DescriptorBuildError::UnsupportedSchema`].
    schema: Option<String>,
}

impl SkillDescriptorBuilder {
    pub fn new() -> Self {
        Self {
            name: None,
            version: None,
            content_hash: None,
            content_hash_hex: Some(String::new()),
            content_hash_error: None,
            input_shape: None,
            output_shape: None,
            effects: vec![],
            references: vec![],
            schema: None,
        }
    }

    /// Declare which schema this descriptor instantiates. v1 recognizes
    /// only [`crate::schema::LYRA_SKILL_SCHEMA_V1_CID`]. When unset, the
    /// builder defaults to that value at `build()` time — the field is
    /// authoritative on the wire, but ergonomic at construction.
    pub fn schema(mut self, cid: impl Into<String>) -> Self {
        self.schema = Some(cid.into());
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn version(mut self, v: impl Into<String>) -> Self {
        self.version = Some(v.into());
        self
    }

    /// Set content hash from raw 32 bytes.
    pub fn content_hash_bytes(mut self, hash: [u8; 32]) -> Self {
        self.content_hash = Some(hash);
        self.content_hash_hex = Some(hex_encode_hash(&hash));
        self
    }

    /// Set content hash from hex string.
    pub fn content_hash_hex(mut self, s: impl Into<String>) -> Self {
        let s = s.into();
        match hex_decode_hash(&s) {
            Ok(hash) => {
                self.content_hash = Some(hash);
                self.content_hash_hex = Some(s);
                self.content_hash_error = None;
            }
            Err(e) => {
                self.content_hash = None;
                self.content_hash_error = Some(e);
            }
        }
        self
    }

    pub fn input_shape(mut self, s: Shape) -> Self {
        self.input_shape = Some(s);
        self
    }

    pub fn output_shape(mut self, s: Shape) -> Self {
        self.output_shape = Some(s);
        self
    }

    pub fn effect(mut self, e: EffectKind) -> Self {
        self.effects.push(e);
        self
    }

    pub fn reference(mut self, name: impl Into<String>) -> Self {
        self.references.push(name.into());
        self
    }

    pub fn build(self) -> Result<SkillDescriptor, DescriptorBuildError> {
        let name = self.name.ok_or(DescriptorBuildError::EmptyName)?;
        validate_name(&name)?;

        // M1: SemVer is validated at build time, not deferred to refinement.
        let version = self.version.ok_or(DescriptorBuildError::EmptyVersion)?;
        if version.is_empty() {
            return Err(DescriptorBuildError::EmptyVersion);
        }
        crate::version::parse(&version)
            .map_err(|e| DescriptorBuildError::InvalidVersion(format!("{e}: {version}")))?;

        let content_hash = match self.content_hash {
            Some(h) => h,
            None => {
                let msg = self
                    .content_hash_error
                    .unwrap_or_else(|| "content_hash is required".into());
                return Err(DescriptorBuildError::InvalidContentHash(msg));
            }
        };

        let input_shape = self.input_shape.ok_or(DescriptorBuildError::ShapeValidationError(
            "input_shape is required".into(),
        ))?;
        let output_shape = self.output_shape.ok_or(DescriptorBuildError::ShapeValidationError(
            "output_shape is required".into(),
        ))?;

        validate_shape(&input_shape)?;
        validate_shape(&output_shape)?;

        // Effects: detect duplicates and reject loudly. Previously the
        // builder silently sort+dedup'd, which meant the descriptor an
        // author wrote (e.g. `["llm_call","llm_call"]`) was accepted but
        // differed byte-for-byte from the canonicalized form used to
        // compute output_hash. Strict rejection keeps wire form = author
        // form (after normalization steps the author actually witnesses).
        let mut effects = self.effects;
        effects.sort_by_key(|e| effect_code(*e));
        for window in effects.windows(2) {
            if window[0] == window[1] {
                return Err(DescriptorBuildError::DuplicateEffect(
                    format!("{:?}", window[0]),
                ));
            }
        }
        // H-7: `None` declared alongside real effects is a contradiction
        // ("pure AND does X"). Previously the builder silently stripped
        // `None`, leaving the SKILL.md author claiming purity while the
        // attestation said otherwise. Reject loudly so the wire form
        // never lies. `[None]` alone is preserved (purity claim).
        if effects.len() > 1 && effects.iter().any(|e| *e == EffectKind::None) {
            return Err(DescriptorBuildError::ContradictoryNoneEffect);
        }

        // M5 + S4: every reference must be a *pinned* `name@<64-hex>`.
        // Pinning closes the silent-version-drift gap that name-only
        // references admit.
        let references = self.references;
        for r in &references {
            validate_reference(r)?;
        }

        // Schema field: default to the v1 CID when unset; reject anything
        // other than recognized values. This is the load-bearing
        // schemas-first wedge: every descriptor declares its schema.
        let schema = match self.schema {
            None => crate::schema::LYRA_SKILL_SCHEMA_V1_CID.to_string(),
            Some(s) if s == crate::schema::LYRA_SKILL_SCHEMA_V1_CID => s,
            Some(other) => {
                return Err(DescriptorBuildError::UnsupportedSchema(other));
            }
        };

        Ok(SkillDescriptor {
            schema,
            name,
            version,
            content_hash,
            input_shape,
            output_shape,
            effects,
            references,
        })
    }
}

impl SkillDescriptor {
    pub fn builder() -> SkillDescriptorBuilder {
        SkillDescriptorBuilder::new()
    }

    pub fn name(&self) -> &str { &self.name }
    pub fn version(&self) -> &str { &self.version }
    pub fn content_hash(&self) -> &[u8; 32] { &self.content_hash }
    pub fn content_hash_hex(&self) -> String {
        hex_encode_hash(&self.content_hash)
    }
    pub fn input_shape(&self) -> &Shape { &self.input_shape }
    pub fn output_shape(&self) -> &Shape { &self.output_shape }
    pub fn effects(&self) -> &[EffectKind] { &self.effects }
    pub fn references(&self) -> &[String] { &self.references }
    pub fn schema(&self) -> &str { &self.schema }

    /// Canonical deterministic bytes of the descriptor. This is what gets
    /// hashed for the content-integrity receipt.
    pub fn canonicalize(&self) -> Vec<u8> {
        canonicalize_descriptor(self)
    }
}

// ---- Shape validation ----
//
// **One source of truth.** Every numeric cap below is derived from the
// sealed `output_shape!`-declared type in `crate::shape`. That makes the
// `output_shape!` macros the canonical definitions; the runtime
// validator follows.

use uor_foundation::pipeline::ConstrainedTypeShape;
use crate::shape::{LyraU8, LyraU16, LyraU32, LyraU64, LyraString, LyraBytes};

/// Per-leaf-shape upper bounds for `max_bytes`. Derived from each sealed
/// type's `SITE_COUNT` (integer widths) or `CYCLE_SIZE` (variable-length
/// shapes).
const U8_MAX:     u64 = <LyraU8     as ConstrainedTypeShape>::SITE_COUNT as u64;
const U16_MAX:    u64 = <LyraU16    as ConstrainedTypeShape>::SITE_COUNT as u64;
const U32_MAX:    u64 = <LyraU32    as ConstrainedTypeShape>::SITE_COUNT as u64;
const U64_MAX:    u64 = <LyraU64    as ConstrainedTypeShape>::SITE_COUNT as u64;
const STRING_MAX: u64 = <LyraString as ConstrainedTypeShape>::CYCLE_SIZE;
const BYTES_MAX:  u64 = <LyraBytes  as ConstrainedTypeShape>::CYCLE_SIZE;

/// Universal capacity cap (16 MiB), exposed for use by container shapes
/// and any external consumer that needs the value. Anchored in the
/// sealed `LyraString` type's `CYCLE_SIZE` — the macro is the source.
pub const LYRA_MAX_BYTES: u64 = STRING_MAX;

/// Same cap for `list.max_items`. The spec § "shape grammar" defines
/// both `max_bytes` and `max_items` as `uint ∈ [1, 16_777_216]`.
pub const LYRA_MAX_ITEMS: u64 = LYRA_MAX_BYTES;

/// Spec `ident` regex `[A-Za-z_][A-Za-z0-9_]{0,63}` admits at most 64 chars.
pub const LYRA_MAX_IDENT_LENGTH: usize = 64;

/// Validate a structured-field identifier against the spec's
/// `[A-Za-z_][A-Za-z0-9_]{0,63}` regex.
pub fn validate_ident(s: &str) -> Result<(), DescriptorBuildError> {
    if s.is_empty() {
        return Err(shape_err("structured field name must not be empty"));
    }
    if s.len() > LYRA_MAX_IDENT_LENGTH {
        return Err(shape_err(&format!(
            "structured field name length {} exceeds {LYRA_MAX_IDENT_LENGTH}",
            s.len()
        )));
    }
    let bytes = s.as_bytes();
    let first_ok = matches!(bytes[0], b'A'..=b'Z' | b'a'..=b'z' | b'_');
    if !first_ok {
        return Err(shape_err(
            "structured field name must start with [A-Za-z_]",
        ));
    }
    for b in &bytes[1..] {
        if !matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_') {
            return Err(shape_err(&format!(
                "structured field name {s:?} contains a non-ident byte"
            )));
        }
    }
    Ok(())
}

fn validate_shape(s: &Shape) -> Result<(), DescriptorBuildError> {
    // H2: spec defines `uint ::= [1, 16777216]`. Reject zero on every
    // leaf so descriptors with `max_bytes = 0` cannot mint a receipt.
    // All upper bounds come from the sealed `output_shape!` types in
    // `crate::shape` — those macro outputs are the source of truth.
    match s {
        Shape::U8 { max_bytes }     => check_range("u8 max_bytes",     *max_bytes, 1, U8_MAX)?,
        Shape::U16 { max_bytes }    => check_range("u16 max_bytes",    *max_bytes, 1, U16_MAX)?,
        Shape::U32 { max_bytes }    => check_range("u32 max_bytes",    *max_bytes, 1, U32_MAX)?,
        Shape::U64 { max_bytes }    => check_range("u64 max_bytes",    *max_bytes, 1, U64_MAX)?,
        Shape::String { max_bytes } => check_range("string max_bytes", *max_bytes, 1, STRING_MAX)?,
        Shape::Bytes  { max_bytes } => check_range("bytes max_bytes",  *max_bytes, 1, BYTES_MAX)?,
        Shape::Structured { fields } => {
            if fields.is_empty() {
                return Err(shape_err("structured shape must have at least one field"));
            }
            // F5: structured-field count is encoded as `u16` in the
            // canonical encoding. Bound the count so the encoder never
            // truncates silently.
            if fields.len() > u16::MAX as usize {
                return Err(shape_err(&format!(
                    "structured shape has {} fields, max {}",
                    fields.len(),
                    u16::MAX
                )));
            }
            // L1 + H4: every field name is validated against the ident regex.
            // Names are bounded to 64 bytes, so the u16 length field used in
            // the canonical encoding never overflows.
            let mut seen: Vec<&str> = Vec::with_capacity(fields.len());
            for f in fields {
                validate_ident(&f.name)?;
                if seen.contains(&f.name.as_str()) {
                    return Err(shape_err(&format!(
                        "structured field name {:?} repeats",
                        f.name
                    )));
                }
                seen.push(&f.name);
                validate_shape(&f.shape)?;
            }
        }
        Shape::List { item, max_items } => {
            check_range("list max_items", *max_items, 1, LYRA_MAX_ITEMS)?;
            validate_shape(item)?;
        }
    }
    // H3: structured capacity (product of leaf max_bytes) must fit in
    // the 16 MiB universal cap. Saturating-multiply guards against
    // u128 overflow.
    let cap = shape_capacity(s);
    if cap > LYRA_MAX_BYTES as u128 {
        // Prefixing with CAPACITY_EXCEEDED_TAG lets the refinement and
        // fusion gates distinguish "exceeds 16 MiB" from "missing name",
        // "bad hex", etc. — capacity overflow during evolution is a
        // legitimate refinement-time rejection, not a malformation.
        return Err(shape_err(&format!(
            "{CAPACITY_EXCEEDED_TAG} {cap} exceeds {} bytes (16 MiB)",
            LYRA_MAX_BYTES,
        )));
    }
    Ok(())
}

/// Stable tag prefix on every shape-capacity-exceeded error message.
/// Higher layers (`tripwire::check_refine`, `fuse::fuse_skills`)
/// recognize this tag to route capacity overflow as a typed refinement
/// outcome rather than collapsing it to `malformed_descriptor`.
pub const CAPACITY_EXCEEDED_TAG: &str = "shape capacity";

/// Returns `true` iff `msg` is a capacity-overflow error emitted by
/// `validate_shape`. Used by upstream gates to differentiate this case
/// from other shape-validation errors without parsing the full string.
pub fn is_capacity_exceeded_error(msg: &str) -> bool {
    msg.contains(CAPACITY_EXCEEDED_TAG) && msg.contains("exceeds")
}

fn check_range(field: &str, value: u64, min: u64, max: u64) -> Result<(), DescriptorBuildError> {
    if value < min {
        return Err(shape_err(&format!("{field} {value} below minimum {min}")));
    }
    if value > max {
        return Err(shape_err(&format!("{field} {value} exceeds maximum {max}")));
    }
    Ok(())
}

/// Total capacity of a shape, in bytes, computed in `u128` so overflow
/// is detectable. For structured shapes the cap is the **product** of
/// child capacities (spec § 3); for lists it is `item_capacity * max_items`.
fn shape_capacity(s: &Shape) -> u128 {
    match s {
        Shape::U8     { max_bytes }
        | Shape::U16  { max_bytes }
        | Shape::U32  { max_bytes }
        | Shape::U64  { max_bytes }
        | Shape::String { max_bytes }
        | Shape::Bytes  { max_bytes } => *max_bytes as u128,
        Shape::Structured { fields } => {
            let mut prod: u128 = 1;
            for f in fields {
                prod = prod.saturating_mul(shape_capacity(&f.shape));
            }
            prod
        }
        Shape::List { item, max_items } => {
            shape_capacity(item).saturating_mul(*max_items as u128)
        }
    }
}

fn shape_err(msg: &str) -> DescriptorBuildError {
    DescriptorBuildError::ShapeValidationError(msg.into())
}

// ---- Canonical encoding ----
// Deterministic wire format. Simple and verifiable:
// <schema_len><schema><name_len><name><version_len><version><content_hash_32>
// <input_shape_bytes><output_shape_bytes><effects_bytes><references_bytes>
//
// `schema` sits first so future schema-aware parsers can read it before
// committing to a particular field layout. v1 is the only recognized
// value but the position is forward-compatible.

fn canonicalize_descriptor(d: &SkillDescriptor) -> Vec<u8> {
    let mut buf = Vec::with_capacity(256);
    buf.extend_from_slice(&(d.schema.len() as u32).to_le_bytes());
    buf.extend_from_slice(d.schema.as_bytes());
    buf.extend_from_slice(&(d.name.len() as u32).to_le_bytes());
    buf.extend_from_slice(d.name.as_bytes());
    buf.extend_from_slice(&(d.version.len() as u32).to_le_bytes());
    buf.extend_from_slice(d.version.as_bytes());
    buf.extend_from_slice(&d.content_hash);
    // Encode shapes deterministically
    encode_shape(&mut buf, &d.input_shape);
    encode_shape(&mut buf, &d.output_shape);
    // Effects as sorted byte codes
    let mut effect_codes: Vec<u8> = d.effects.iter().map(|e| effect_code(*e)).collect();
    effect_codes.sort();
    buf.extend_from_slice(&(effect_codes.len() as u16).to_le_bytes());
    buf.extend_from_slice(&effect_codes);
    // References sorted lexicographically
    let mut sorted_refs: Vec<&str> = d.references.iter().map(|s| s.as_str()).collect();
    sorted_refs.sort();
    buf.extend_from_slice(&(sorted_refs.len() as u16).to_le_bytes());
    for r in sorted_refs {
        buf.extend_from_slice(&(r.len() as u16).to_le_bytes());
        buf.extend_from_slice(r.as_bytes());
    }
    buf
}

fn encode_shape(buf: &mut Vec<u8>, s: &Shape) {
    match s {
        Shape::U8 { max_bytes } => {
            buf.push(0);
            buf.extend_from_slice(&max_bytes.to_le_bytes());
        }
        Shape::U16 { max_bytes } => {
            buf.push(1);
            buf.extend_from_slice(&max_bytes.to_le_bytes());
        }
        Shape::U32 { max_bytes } => {
            buf.push(2);
            buf.extend_from_slice(&max_bytes.to_le_bytes());
        }
        Shape::U64 { max_bytes } => {
            buf.push(3);
            buf.extend_from_slice(&max_bytes.to_le_bytes());
        }
        Shape::String { max_bytes } => {
            buf.push(4);
            buf.extend_from_slice(&max_bytes.to_le_bytes());
        }
        Shape::Bytes { max_bytes } => {
            buf.push(5);
            buf.extend_from_slice(&max_bytes.to_le_bytes());
        }
        Shape::Structured { fields } => {
            buf.push(6);
            let mut sorted: Vec<&NamedField> = fields.iter().collect();
            sorted.sort_by_key(|f| &f.name);
            buf.extend_from_slice(&(sorted.len() as u16).to_le_bytes());
            for f in sorted {
                // H4: encode field-name length as u16. Validation already
                // caps field names at 64 bytes, so this never overflows.
                // Matches the u16 used for the other length fields in the
                // canonical encoding.
                buf.extend_from_slice(&(f.name.len() as u16).to_le_bytes());
                buf.extend_from_slice(f.name.as_bytes());
                encode_shape(buf, &f.shape);
            }
        }
        Shape::List { item, max_items } => {
            buf.push(7);
            buf.extend_from_slice(&max_items.to_le_bytes());
            encode_shape(buf, item);
        }
    }
}

fn effect_code(e: EffectKind) -> u8 {
    match e {
        EffectKind::None => 0,
        EffectKind::FileRead => 1,
        EffectKind::FileWrite => 2,
        EffectKind::WebRead => 3,
        EffectKind::WebWrite => 4,
        EffectKind::Terminal => 5,
        EffectKind::Llm => 6,
    }
}

// ---- hex helpers ----

fn hex_encode_hash(hash: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for b in hash {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

fn hex_decode_hash(s: &str) -> Result<[u8; 32], String> {
    if s.len() != 64 {
        return Err(format!("expected 64 hex chars, got {}", s.len()));
    }
    let mut bytes = [0u8; 32];
    for i in 0..32 {
        let lo = hex_nibble(s.as_bytes()[i*2+1])?;
        let hi = hex_nibble(s.as_bytes()[i*2])?;
        bytes[i] = (hi << 4) | lo;
    }
    Ok(bytes)
}

fn hex_nibble(b: u8) -> Result<u8, String> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(format!("invalid hex nibble: {}", b as char)),
    }
}

// ---- tests ----

#[cfg(test)]
mod tests {
    use super::*;

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
            .effect(EffectKind::WebRead)
            .build()
            .expect("valid descriptor")
    }

    #[test]
    fn builder_produces_valid_descriptor() {
        let d = test_descriptor();
        assert_eq!(d.name(), "web-search");
        assert_eq!(d.version(), "1.0.0");
        assert_eq!(d.references().len(), 0);
    }

    #[test]
    fn builder_rejects_empty_name() {
        let err = SkillDescriptor::builder()
            .name("")
            .version("1.0.0")
            .content_hash_hex("aa".repeat(32).as_str())
            .input_shape(Shape::U8 { max_bytes: 1 })
            .output_shape(Shape::U8 { max_bytes: 1 })
            .build();
        assert!(err.is_err());
    }

    #[test]
    fn builder_rejects_invalid_hex_hash() {
        let err = SkillDescriptor::builder()
            .name("x")
            .version("1.0.0")
            .content_hash_hex("not-hex!")
            .input_shape(Shape::U8 { max_bytes: 1 })
            .output_shape(Shape::U8 { max_bytes: 1 })
            .build();
        assert!(err.is_err());
    }

    #[test]
    fn canonicalize_is_deterministic() {
        let d1 = test_descriptor();
        let d2 = test_descriptor();
        assert_eq!(d1.canonicalize(), d2.canonicalize());
    }

    #[test]
    fn canonicalize_differs_on_content_hash_change() {
        let d1 = test_descriptor();
        // Build a new descriptor with a different hash
        let d2 = SkillDescriptor::builder()
            .name("web-search")
            .version("1.0.0")
            .content_hash_hex("ff".repeat(32).as_str())
            .input_shape(Shape::String { max_bytes: 256 })
            .output_shape(Shape::List {
                item: Box::new(Shape::String { max_bytes: 4096 }),
                max_items: 100,
            })
            .effect(EffectKind::WebRead)
            .build()
            .unwrap();
        assert_ne!(d1.canonicalize(), d2.canonicalize());
    }

    #[test]
    fn shape_validation_rejects_over_limit() {
        let err = SkillDescriptor::builder()
            .name("x")
            .version("1.0.0")
            .content_hash_hex("aa".repeat(32).as_str())
            .input_shape(Shape::String { max_bytes: 16_777_217 })
            .output_shape(Shape::U8 { max_bytes: 1 })
            .build();
        let err = err.expect_err("expected shape validation error");
        assert!(
            matches!(&err, DescriptorBuildError::ShapeValidationError(_)),
            "got {err:?}"
        );
        let detail = format!("{err}");
        assert!(detail.contains("string max_bytes"), "detail = {detail}");
        assert!(detail.contains("16777216"), "detail = {detail}");
    }

    // ---- regression tests for the red-team audit ----

    #[test]
    fn h2_zero_max_bytes_rejected() {
        let err = SkillDescriptor::builder()
            .name("x")
            .version("1.0.0")
            .content_hash_hex("aa".repeat(32).as_str())
            .input_shape(Shape::String { max_bytes: 0 })
            .output_shape(Shape::U8 { max_bytes: 1 })
            .build();
        assert!(matches!(err, Err(DescriptorBuildError::ShapeValidationError(_))));
    }

    #[test]
    fn h3_structured_capacity_product_enforced() {
        // 100 fields of string max=16 MiB ⇒ product overflows the cap.
        let mut fields = Vec::with_capacity(100);
        for i in 0..100 {
            fields.push(NamedField {
                name: format!("f{i}"),
                shape: Shape::String { max_bytes: 16_777_216 },
            });
        }
        let err = SkillDescriptor::builder()
            .name("x")
            .version("1.0.0")
            .content_hash_hex("aa".repeat(32).as_str())
            .input_shape(Shape::Structured { fields })
            .output_shape(Shape::U8 { max_bytes: 1 })
            .build();
        let err = err.expect_err("expected structured-product violation");
        let msg = format!("{err}");
        assert!(is_capacity_exceeded_error(&msg), "got {err}");
    }

    #[test]
    fn h4_l1_field_ident_validated() {
        // Non-ident field name: starts with digit.
        let err = SkillDescriptor::builder()
            .name("x")
            .version("1.0.0")
            .content_hash_hex("aa".repeat(32).as_str())
            .input_shape(Shape::Structured {
                fields: vec![NamedField {
                    name: "1bad".into(),
                    shape: Shape::U8 { max_bytes: 1 },
                }],
            })
            .output_shape(Shape::U8 { max_bytes: 1 })
            .build();
        assert!(matches!(err, Err(DescriptorBuildError::ShapeValidationError(_))));

        // Overlong field name (>64 bytes) is also rejected.
        let long = "a".repeat(65);
        let err = SkillDescriptor::builder()
            .name("x")
            .version("1.0.0")
            .content_hash_hex("aa".repeat(32).as_str())
            .input_shape(Shape::Structured {
                fields: vec![NamedField {
                    name: long,
                    shape: Shape::U8 { max_bytes: 1 },
                }],
            })
            .output_shape(Shape::U8 { max_bytes: 1 })
            .build();
        assert!(matches!(err, Err(DescriptorBuildError::ShapeValidationError(_))));
    }

    #[test]
    fn m1_invalid_semver_rejected() {
        let err = SkillDescriptor::builder()
            .name("x")
            .version("not-semver")
            .content_hash_hex("aa".repeat(32).as_str())
            .input_shape(Shape::U8 { max_bytes: 1 })
            .output_shape(Shape::U8 { max_bytes: 1 })
            .build();
        assert!(matches!(err, Err(DescriptorBuildError::InvalidVersion(_))));
    }

    #[test]
    fn duplicate_effects_are_rejected_not_silently_deduped() {
        // Audit fix: previously the builder silently sort+dedup'd
        // duplicate effects. That meant the descriptor an author wrote
        // (e.g. `["llm_call","llm_call"]`) would be accepted, but the
        // canonical form used for output_hash differed byte-for-byte.
        // Strict rejection keeps wire-form = author-form.
        let err = SkillDescriptor::builder()
            .name("x")
            .version("1.0.0")
            .content_hash_hex("aa".repeat(32).as_str())
            .input_shape(Shape::U8 { max_bytes: 1 })
            .output_shape(Shape::U8 { max_bytes: 1 })
            .effect(EffectKind::WebRead)
            .effect(EffectKind::WebRead)
            .build()
            .expect_err("duplicates must be rejected loudly, not silently deduped");
        assert!(matches!(err, DescriptorBuildError::DuplicateEffect(_)));
        assert!(format!("{err}").contains("duplicate effect"));
    }

    #[test]
    fn effects_are_sorted_canonically_when_distinct() {
        // Distinct effects are sorted by effect_code so the canonical
        // form is independent of insertion order.
        let desc = SkillDescriptor::builder()
            .name("x")
            .version("1.0.0")
            .content_hash_hex("aa".repeat(32).as_str())
            .input_shape(Shape::U8 { max_bytes: 1 })
            .output_shape(Shape::U8 { max_bytes: 1 })
            .effect(EffectKind::WebRead)
            .effect(EffectKind::FileRead)
            .build()
            .expect("valid");
        assert_eq!(desc.effects(), &[EffectKind::FileRead, EffectKind::WebRead]);
    }

    // PASS-7: whitespace-only references must be rejected by the pinned
    // `name@<64-hex>` rule — the empty/blank form has no '@' separator,
    // so `validate_reference` errors before name validation runs.
    #[test]
    fn pass7_whitespace_only_reference_rejected() {
        assert!(matches!(
            validate_reference("   "),
            Err(DescriptorBuildError::InvalidReference(_))
        ));
        assert!(matches!(
            validate_reference(""),
            Err(DescriptorBuildError::InvalidReference(_))
        ));
        assert!(matches!(
            validate_reference("\t\n"),
            Err(DescriptorBuildError::InvalidReference(_))
        ));
    }

    #[test]
    fn m5_invalid_reference_rejected() {
        let err = SkillDescriptor::builder()
            .name("x")
            .version("1.0.0")
            .content_hash_hex("aa".repeat(32).as_str())
            .input_shape(Shape::U8 { max_bytes: 1 })
            .output_shape(Shape::U8 { max_bytes: 1 })
            .reference("BadName!") // uppercase + '!' — fails name rules
            .build();
        assert!(matches!(err, Err(DescriptorBuildError::InvalidReference(_))));
    }

    // ---- name validation ----

    fn build_with_name(name: &str) -> Result<SkillDescriptor, DescriptorBuildError> {
        SkillDescriptor::builder()
            .name(name)
            .version("1.0.0")
            .content_hash_hex("aa".repeat(32).as_str())
            .input_shape(Shape::U8 { max_bytes: 1 })
            .output_shape(Shape::U8 { max_bytes: 1 })
            .build()
    }

    #[test]
    fn name_accepts_kebab_case() {
        assert!(build_with_name("web-search").is_ok());
        assert!(build_with_name("pdf-processing").is_ok());
        assert!(build_with_name("x").is_ok());
        assert!(build_with_name("a1-b2-c3").is_ok());
    }

    #[test]
    fn name_rejects_uppercase() {
        assert!(matches!(build_with_name("Web-Search"), Err(DescriptorBuildError::InvalidName(_))));
    }

    #[test]
    fn name_rejects_underscore() {
        assert!(matches!(build_with_name("web_search"), Err(DescriptorBuildError::InvalidName(_))));
    }

    #[test]
    fn name_rejects_leading_or_trailing_hyphen() {
        assert!(matches!(build_with_name("-web"), Err(DescriptorBuildError::InvalidName(_))));
        assert!(matches!(build_with_name("web-"), Err(DescriptorBuildError::InvalidName(_))));
    }

    #[test]
    fn name_rejects_consecutive_hyphens() {
        assert!(matches!(build_with_name("web--search"), Err(DescriptorBuildError::InvalidName(_))));
    }

    #[test]
    fn name_rejects_over_64_chars() {
        let long = "a".repeat(65);
        assert!(matches!(build_with_name(&long), Err(DescriptorBuildError::InvalidName(_))));
    }

    fn build_with_hash(h: &str) -> Result<SkillDescriptor, DescriptorBuildError> {
        SkillDescriptor::builder()
            .name("ok")
            .version("1.0.0")
            .content_hash_hex(h)
            .input_shape(Shape::String { max_bytes: 16 })
            .output_shape(Shape::String { max_bytes: 16 })
            .effect(EffectKind::None)
            .build()
    }

    #[test]
    fn content_hash_error_empty_is_discriminated() {
        let err = build_with_hash("").unwrap_err();
        match err {
            DescriptorBuildError::InvalidContentHash(m) => {
                assert!(m.contains("got 0"), "msg: {m}");
            }
            other => panic!("expected InvalidContentHash, got {other:?}"),
        }
    }

    #[test]
    fn content_hash_error_wrong_length_is_discriminated() {
        let err = build_with_hash("deadbeef").unwrap_err();
        match err {
            DescriptorBuildError::InvalidContentHash(m) => {
                assert!(m.contains("got 8"), "msg: {m}");
            }
            other => panic!("expected InvalidContentHash, got {other:?}"),
        }
    }

    #[test]
    fn content_hash_error_non_hex_is_discriminated() {
        let bad = "zz".to_string() + &"0".repeat(62);
        let err = build_with_hash(&bad).unwrap_err();
        match err {
            DescriptorBuildError::InvalidContentHash(m) => {
                assert!(m.contains("invalid hex nibble"), "msg: {m}");
            }
            other => panic!("expected InvalidContentHash, got {other:?}"),
        }
    }

    #[test]
    fn content_hash_missing_falls_back_to_generic() {
        let err = SkillDescriptor::builder()
            .name("ok")
            .version("1.0.0")
            .input_shape(Shape::String { max_bytes: 16 })
            .output_shape(Shape::String { max_bytes: 16 })
            .effect(EffectKind::None)
            .build()
            .unwrap_err();
        match err {
            DescriptorBuildError::InvalidContentHash(m) => {
                assert!(m.contains("required"), "msg: {m}");
            }
            other => panic!("expected InvalidContentHash, got {other:?}"),
        }
    }
}
