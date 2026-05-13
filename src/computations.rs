//! Deterministic protocol computations: validate, compose, resolve,
//! and snapshot. Each is a pure function — same input always produces
//! the same output.

/// BLAKE3-256 of the input bytes.
fn blake3_hash(input: &[u8]) -> Vec<u8> {
    blake3::hash(input).as_bytes().to_vec()
}

/// BLAKE3-256 of `(runtime_ident || 0x00 || computation_label || 0x00 || bytes)`.
/// This folds the runtime identifier into every computation hash so seals
/// produced under different substrate versions are visibly distinct.
fn runtime_hash(label: &str, bytes: &[u8]) -> Vec<u8> {
    let mut h = blake3::Hasher::new();
    h.update(crate::LYRA_RUNTIME_IDENT.as_bytes());
    h.update(&[0x00]);
    h.update(label.as_bytes());
    h.update(&[0x00]);
    h.update(bytes);
    h.finalize().as_bytes().to_vec()
}

/// Dispatch a computation by id.
pub fn run(computation_id: &str, input: &str) -> Result<Vec<u8>, String> {
    match computation_id {
        "skill_interface_hash" => skill_interface_hash(input),
        "skill_reference_resolve" => skill_reference_resolve(input),
        "merkle_manifest" => merkle_manifest(input),
        "compose_interfaces" => compose_interfaces(input),
        "next_generation" => next_generation(input),
        _ => Err(format!("unknown computation: {computation_id}")),
    }
}

// ------------------------------------------------------------------
// Protocol computations
// ------------------------------------------------------------------

/// Validates a skill interface descriptor and returns its canonical hash.
///
/// Input: JSON object with required fields:
///   { "name": "...", "version": "...", "input_shape": {...},
///     "output_shape": {...}, "effects": [...], "references": [...],
///     "content_hash": "..." }
///
/// Validation rules:
/// - All required fields present.
/// - shape kinds are from the Lyra vocabulary: "u8", "u16", "u32", "u64",
///   "string", "bytes", "structured", "list".
/// - max_bytes <= 16 MiB.
/// - effects are from known set: "none", "file_read", "file_write",
///   "web_read", "web_write", "terminal", "llm".
/// - references are non-empty strings.
/// - content_hash is a valid hex string.
///
/// Output: 32 raw BLAKE3-256 bytes of the canonical descriptor encoding.
///
/// **(C3)** This function parses the input JSON into a typed
/// [`SkillDescriptor`] via the sealed builder and hashes the typed binary
/// canonical bytes. The CLI and the Rust API now share **one** encoder:
/// `SkillDescriptor::canonicalize()`. There is no separate JSON-string
/// canonical form; the previous Rust `{:?}` formatter and ad-hoc sort
/// order are gone.
fn skill_interface_hash(input: &str) -> Result<Vec<u8>, String> {
    let desc = descriptor_from_json(input)
        .map_err(|e| format!("invalid descriptor: {e}"))?;
    let canonical = desc.canonicalize();
    Ok(runtime_hash("skill_interface_hash", &canonical))
}

/// Resolves pinned skill references against a manifest.
///
/// Input: JSON object `{ "skill": <interface>, "manifest": [{name, content_hash}] }`.
///
/// **(S4)** Each reference is a *pinned* `<name>@<64-hex>` string. The
/// resolver requires that **both** the name and the content_hash match
/// a manifest entry — name-only matches are not accepted. Also rejects
/// manifests containing duplicate (name, content_hash) pairs.
fn skill_reference_resolve(input: &str) -> Result<Vec<u8>, String> {
    let map = parse_json(input)?;
    let skill_str = map.get("skill").ok_or("missing skill")?;
    let manifest_str = map.get("manifest").ok_or("missing manifest")?;

    let skill = parse_json(skill_str)?;
    let refs_str = skill.get("references").ok_or("missing references")?;
    let refs = parse_list(refs_str)?;

    let manifest = parse_list_of_maps(manifest_str)?;
    let mut manifest_entries: Vec<(String, String)> = Vec::with_capacity(manifest.len());
    for entry in &manifest {
        let name = get_str(entry, "name")?.to_string();
        let hash = get_str(entry, "content_hash")?.to_string();
        let pair = (name, hash);
        if manifest_entries.contains(&pair) {
            return Err(format!(
                "duplicate manifest entry: name={:?} content_hash={:?}",
                pair.0, pair.1
            ));
        }
        manifest_entries.push(pair);
    }

    let mut resolved: Vec<String> = Vec::new();
    for r in refs {
        // S4: split the pinned reference into (name, content_hash).
        let (ref_name, ref_hash) = r.split_once('@').ok_or_else(|| {
            format!("reference {r:?} must have form 'name@<64-hex>'")
        })?;
        if ref_hash.len() != 64 {
            return Err(format!(
                "reference {r:?}: hash must be 64 hex chars, got {}",
                ref_hash.len()
            ));
        }
        let mut found = false;
        for (m_name, m_hash) in &manifest_entries {
            if m_name == ref_name && m_hash == ref_hash {
                resolved.push(m_hash.clone());
                found = true;
                break;
            }
        }
        if !found {
            return Err(format!("unresolved reference: {r}"));
        }
    }

    // **AUDIT #5**: explicit length-prefixed canonical bytes for the
    // resolved list. Prior code used `format!("{:?}", resolved)` whose
    // output is governed only by Rust's `Debug` impl — the spec makes
    // no stability guarantee, so a toolchain bump could silently flip
    // the canonical hash. Length-prefixed encoding is the same pattern
    // used in `SkillDescriptor::canonicalize` and is fully spec-pinned.
    //
    // Format: `u32(count) || for each item: u32(len) || bytes`.
    let mut canonical = Vec::with_capacity(64 + resolved.iter().map(|s| s.len() + 4).sum::<usize>());
    canonical.extend_from_slice(&(resolved.len() as u32).to_le_bytes());
    for item in &resolved {
        canonical.extend_from_slice(&(item.len() as u32).to_le_bytes());
        canonical.extend_from_slice(item.as_bytes());
    }
    Ok(runtime_hash("skill_reference_resolve", &canonical))
}

/// Computes a Merkle-tree root over a registry manifest.
///
/// Input: JSON list of `{ "path": "...", "content_hash": "..." }` entries.
///
/// Properties:
/// - **Canonical ordering** by `path`.
/// - **(C2) Domain separation**: leaves are prefixed with `0x00`, internal
///   nodes with `0x01`. A leaf hash can never equal an internal-node hash
///   for any input, closing the standard second-preimage hole in Merkle
///   constructions.
/// - **Length prefixes** on path and content_hash inside the leaf so
///   `("ab","cd")` ≠ `("a","bcd")`.
/// - **(H5) Leaf count is folded into the root** as the final step, so two
///   manifests of different lengths cannot share a root — even if a longer
///   manifest's last entry happens to equal a shorter manifest's last leaf.
///   The padding-by-duplication ambiguity is gone.
fn merkle_manifest(input: &str) -> Result<Vec<u8>, String> {
    let entries = parse_list_of_maps(input)?;
    if entries.is_empty() {
        // D2: empty manifests are valid (genesis snapshot, fresh
        // registry). Return a distinct sentinel hash so an empty
        // manifest is distinguishable from any non-empty one.
        return Ok(runtime_hash("merkle_manifest", b"\x00EMPTY\x00"));
    }

    let mut pairs: Vec<(String, String)> = entries
        .into_iter()
        .map(|e| {
            let path = get_str(&e, "path").map(String::from)?;
            let hash = get_str(&e, "content_hash").map(String::from)?;
            Ok((path, hash))
        })
        .collect::<Result<Vec<_>, String>>()?;

    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let leaf_count = pairs.len() as u32;

    // Leaves: BLAKE3(0x00 ‖ u32_LE(path_len) ‖ path ‖ u32_LE(hash_len) ‖ hash).
    let mut level: Vec<Vec<u8>> = pairs
        .iter()
        .map(|(p, h)| {
            let mut buf = Vec::with_capacity(1 + 4 + p.len() + 4 + h.len());
            buf.push(0x00); // leaf tag
            buf.extend_from_slice(&(p.len() as u32).to_le_bytes());
            buf.extend_from_slice(p.as_bytes());
            buf.extend_from_slice(&(h.len() as u32).to_le_bytes());
            buf.extend_from_slice(h.as_bytes());
            blake3_hash(&buf)
        })
        .collect();

    // Pad to power of two by duplicating the last leaf. The duplication
    // is harmless because the leaf count is folded into the root below,
    // so two manifests of different lengths cannot collide.
    let target = level.len().next_power_of_two();
    let last = level.last().unwrap().clone();
    while level.len() < target {
        level.push(last.clone());
    }

    // Internal nodes: BLAKE3(0x01 ‖ left ‖ right). Distinct from leaves.
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len() / 2);
        for i in (0..level.len()).step_by(2) {
            let mut concat = Vec::with_capacity(1 + 32 + 32);
            concat.push(0x01); // internal tag
            concat.extend_from_slice(&level[i]);
            concat.extend_from_slice(&level[i + 1]);
            next.push(blake3_hash(&concat));
        }
        level = next;
    }
    let root = level.pop().unwrap();

    // Fold the leaf count into the final hash. Two manifests with
    // different lengths cannot share a Merkle root after this step.
    let mut commitment = Vec::with_capacity(4 + 32);
    commitment.extend_from_slice(&leaf_count.to_le_bytes());
    commitment.extend_from_slice(&root);
    Ok(runtime_hash("merkle_manifest", &commitment))
}

// ------------------------------------------------------------------
// JSON helpers (minimal, no_std-friendly)
// ------------------------------------------------------------------

/// Decode a JSON string literal token (`"..."` with standard escapes) to
/// its underlying Rust `String`. The **single canonical implementation**
/// for unquoting; modules elsewhere import this rather than rolling
/// their own. Handles every JSON string escape: `\"`, `\\`, `\/`, `\n`,
/// `\r`, `\t`, `\b`, `\f`, and `\uXXXX`.
pub(crate) fn unquote_json_string(token: &str) -> Result<String, String> {
    let s = token.trim();
    let b = s.as_bytes();
    if b.len() < 2 || b[0] != b'"' || b[b.len() - 1] != b'"' {
        return Err(format!("expected JSON string, got {s:?}"));
    }
    let inner = &s[1..s.len() - 1];
    let mut out = String::with_capacity(inner.len());
    let mut it = inner.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('/') => out.push('/'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('b') => out.push('\u{0008}'),
            Some('f') => out.push('\u{000C}'),
            Some('u') => {
                let h: String = (0..4).filter_map(|_| it.next()).collect();
                if h.len() != 4 {
                    return Err("truncated \\u escape".into());
                }
                let cp = u32::from_str_radix(&h, 16).map_err(|e| e.to_string())?;
                let ch = char::from_u32(cp)
                    .ok_or_else(|| format!("invalid code point U+{cp:04X}"))?;
                out.push(ch);
            }
            Some(other) => return Err(format!("unknown escape \\{other}")),
            None => return Err("trailing backslash".into()),
        }
    }
    Ok(out)
}

pub(crate) fn parse_json(input: &str) -> Result<std::collections::HashMap<String, String>, String> {
    let mut map = std::collections::HashMap::new();
    // Strip exactly ONE opening `{` and ONE closing `}`. The previous
    // `trim_*_matches` form stripped repeated braces, which silently ate
    // closing braces from string values like `{"key":"val}}"}` and
    // broke the certify→verify round trip whenever a nested object
    // argument happened to land last in the outer object.
    let trimmed = input.trim();
    let body = trimmed
        .strip_prefix('{')
        .ok_or_else(|| format!("expected JSON object opening, got {trimmed:?}"))?
        .strip_suffix('}')
        .ok_or_else(|| format!("expected JSON object closing, got {trimmed:?}"))?;
    for pair in split_json_pairs(body) {
        let (k, v) = split_key_value(pair)
            .ok_or_else(|| format!("malformed pair (no top-level colon): {pair}"))?;
        let k = k.trim().trim_matches('"');
        let v = v.trim();
        if map.insert(k.to_string(), v.to_string()).is_some() {
            return Err(format!("duplicate key: {k}"));
        }
    }
    Ok(map)
}

/// Split a JSON object body on top-level commas, **respecting string
/// boundaries** so commas inside string values do not mis-split. Also
/// respects standard JSON escape sequences (`\"`, `\\`) — a backslash
/// inside a string consumes the next byte verbatim.
fn split_json_pairs(body: &str) -> Vec<&str> {
    let bytes = body.as_bytes();
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut start = 0usize;
    let mut i = 0usize;
    let mut out = Vec::new();
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            if c == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_string = false;
            }
        } else {
            match c {
                b'"' => in_string = true,
                b'{' | b'[' => depth += 1,
                b'}' | b']' => depth -= 1,
                b',' if depth == 0 => {
                    out.push(body[start..i].trim());
                    start = i + 1;
                }
                _ => {}
            }
        }
        i += 1;
    }
    if start < body.len() {
        out.push(body[start..].trim());
    }
    out
}

/// Split a key:value pair on the first top-level colon that is not
/// inside a string. Returns `(key, value)` substrings.
fn split_key_value(pair: &str) -> Option<(&str, &str)> {
    let bytes = pair.as_bytes();
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            if c == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_string = false;
            }
        } else {
            match c {
                b'"' => in_string = true,
                b'{' | b'[' => depth += 1,
                b'}' | b']' => depth -= 1,
                b':' if depth == 0 => {
                    return Some((&pair[..i], &pair[i + 1..]));
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// Look up a field's raw JSON token from the parse_json map and return
/// its **textual representation**.
///
/// * If the token is a JSON string literal (`"…"`), decode escapes via
///   the canonical `unquote_json_string`.
/// * Otherwise (number, boolean, `null`, raw composite), return the
///   token trimmed.
///
/// This is the single helper used across `computations.rs` for both
/// string fields (`name`, `version`, `type`) and number-token fields
/// (`max_bytes`, `max_items`) — the caller decides whether to further
/// `parse::<u64>()` the result. Replaces the earlier `unquote_one`
/// (which was strip-only and silently dropped JSON escapes inside
/// string values).
fn get_str(
    map: &std::collections::HashMap<String, String>,
    key: &str,
) -> Result<String, String> {
    let raw = map.get(key).ok_or_else(|| format!("missing {key}"))?;
    let trimmed = raw.trim();
    if trimmed.starts_with('"') {
        unquote_json_string(trimmed).map_err(|e| format!("{key}: {e}"))
    } else {
        Ok(trimmed.to_string())
    }
}

fn get_map(map: &std::collections::HashMap<String, String>, key: &str) -> Result<std::collections::HashMap<String, String>, String> {
    let s = map.get(key).ok_or_else(|| format!("missing {key}"))?;
    parse_json(s)
}

fn parse_list(s: &str) -> Result<Vec<String>, String> {
    // Strip exactly one `[` and one `]` (same fix as parse_json: the
    // greedy trim_*_matches form silently ate brackets from values).
    let trimmed = s.trim();
    let inner = trimmed
        .strip_prefix('[')
        .ok_or_else(|| format!("expected JSON array, got {trimmed:?}"))?
        .strip_suffix(']')
        .ok_or_else(|| format!("expected JSON array, got {trimmed:?}"))?;
    if inner.trim().is_empty() {
        return Ok(Vec::new());
    }
    // **MED-2**: require each element to be a JSON-quoted string. Unquoted
    // tokens (e.g. `[network_read]`) are rejected. Decode escapes through
    // the canonical unquote so e.g. `"a\\nb"` returns `a\nb` correctly
    // rather than `a\\nb` verbatim.
    let mut out = Vec::new();
    for piece in split_json_pairs(inner) {
        out.push(unquote_json_string(piece)?);
    }
    Ok(out)
}

fn parse_list_of_maps(s: &str) -> Result<Vec<std::collections::HashMap<String, String>>, String> {
    let trimmed = s.trim();
    let inner = trimmed
        .strip_prefix('[')
        .ok_or_else(|| format!("expected JSON array, got {trimmed:?}"))?
        .strip_suffix(']')
        .ok_or_else(|| format!("expected JSON array, got {trimmed:?}"))?;
    if inner.trim().is_empty() {
        return Ok(Vec::new());
    }
    // Walk objects at depth 1 while tracking string state so braces inside
    // string values do not start a spurious object.
    let bytes = inner.as_bytes();
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut start = 0usize;
    let mut i = 0usize;
    let mut out = Vec::new();
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            if c == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_string = false;
            }
        } else {
            match c {
                b'"' => in_string = true,
                b'{' => {
                    if depth == 0 {
                        start = i;
                    }
                    depth += 1;
                }
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        out.push(parse_json(&inner[start..=i])?);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    Ok(out)
}

// Note: the prior `validate_shape`, `canonicalize_shape`, and
// `validate_effect` helpers were removed when `skill_interface_hash`
// collapsed onto the typed builder (C3). The typed `SkillDescriptor`
// builder + `canonicalize()` are the single source of truth.

// ------------------------------------------------------------------
// Composability: the stud-and-tube check
// ------------------------------------------------------------------

/// Checks if producer.output_shape is compatible with consumer.input_shape.
///
/// Input: JSON object { "producer": <interface>, "consumer": <interface> }
///
/// Returns a deterministic hash of either:
///   "COMPATIBLE"   -- shapes compose
///   "INCOMPATIBLE:<reason>"  -- mismatch with precise reason
fn compose_interfaces(input: &str) -> Result<Vec<u8>, String> {
    // **AUDIT #2**: single source of truth for composition. Route through
    // the typed builder + `gate::check_composable` rather than a parallel
    // string-walking implementation. One implementation = one audit
    // surface = no risk of the two paths drifting.
    let map = parse_json(input)?;
    let producer_str = map.get("producer").ok_or("missing producer")?;
    let consumer_str = map.get("consumer").ok_or("missing consumer")?;
    let producer = descriptor_from_json(producer_str)
        .map_err(|e| format!("producer: {e}"))?;
    let consumer = descriptor_from_json(consumer_str)
        .map_err(|e| format!("consumer: {e}"))?;
    match crate::gate::check_composable(&producer, &consumer) {
        Ok(_) => Ok(runtime_hash("compose_interfaces", b"COMPATIBLE")),
        Err(crate::gate::ValidationError::Incompatible(reason)) => Ok(runtime_hash(
            "compose_interfaces",
            format!("INCOMPATIBLE:{reason}").as_bytes(),
        )),
        Err(e) => Err(format!("compose_interfaces: {e:?}")),
    }
}

// ------------------------------------------------------------------
// next_generation — lineage receipts
// ------------------------------------------------------------------

/// Mints a lineage receipt sealing `parent → child` as a valid refinement.
///
/// Input:
/// ```json
/// {
///   "parent_receipt": "<base64-encoded receipt envelope>",
///   "child_descriptor": <canonical-form descriptor JSON object>
/// }
/// ```
///
/// Behavior (in order):
/// 1. Decode `parent_receipt` from base64 and parse the envelope.
/// 2. Re-verify the parent receipt by re-running its computation and
///    comparing `output_hash`. Failure → `ParentReceiptInvalid`.
/// 3. Require `parent.computation_id ∈ {skill_interface_hash, next_generation}`.
/// 4. Extract the parent descriptor (from `parent.input` for a root, or
///    from `parent.input.child_descriptor` for a chain link).
/// 5. Parse the child descriptor with the typed builder (full validation).
/// 6. Call `refinement::is_refinement(&parent, &child)`. Failure →
///    `NotARefinement(reason)`.
/// 7. Compute the child's interface hash via `skill_interface_hash`.
/// 8. Output `blake3("EVOLVED" || 0x00 || parent.output_hash || 0x00 ||
///    child_interface_hash)`.
/// Typed error variants for `next_generation`. Dispatch-level failures
/// (`ParentReceiptInvalid`, `InvalidParentComputation`) are separated
/// from refinement failures (`NotARefinement(...)`) so callers can
/// pattern-match on cause.
#[derive(Debug, Clone)]
pub enum NextGenerationError {
    /// The parent receipt failed re-verification (tampered, parse error,
    /// content mismatch, or seal replay failure).
    ParentReceiptInvalid(String),
    /// The parent receipt was produced by a computation that cannot be a
    /// lineage parent (only `skill_interface_hash` and `next_generation`
    /// are valid).
    InvalidParentComputation(String),
    /// One of the descriptors could not be parsed.
    MalformedDescriptor(String),
    /// The child descriptor is not a structural refinement of the parent.
    NotARefinement(crate::refinement::RefinementError),
    /// Malformed `next_generation` input (missing field, bad encoding, etc.).
    BadInput(String),
}

impl core::fmt::Display for NextGenerationError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            NextGenerationError::ParentReceiptInvalid(s)     => write!(f, "ParentReceiptInvalid: {s}"),
            NextGenerationError::InvalidParentComputation(s) => write!(f, "InvalidParentComputation: {s}"),
            NextGenerationError::MalformedDescriptor(s)      => write!(f, "MalformedDescriptor: {s}"),
            NextGenerationError::NotARefinement(e)           => write!(f, "NotARefinement({e})"),
            NextGenerationError::BadInput(s)                 => write!(f, "BadInput: {s}"),
        }
    }
}

impl std::error::Error for NextGenerationError {}

/// Typed entry point for the lineage computation. Returns the 32-byte
/// output bytes on success.
///
/// The hash construction binds **raw 32-byte values**, never their hex
/// representations: `BLAKE3(b"EVOLVED" ‖ 0x00 ‖ parent_output_hash[32] ‖
/// 0x00 ‖ child_interface_hash[32])`. `parent.output_hash` is decoded from
/// its 64-char hex form to 32 raw bytes before being fed to the hasher;
/// `child_interface_hash` is the raw 32 bytes returned by `blake3::hash`
/// on the child's canonical bytes (never the hex form).
/// Maximum lineage-chain depth observable by a single `verify` /
/// `next_generation` invocation. **(F3)** Bounds the O(N²) parent-
/// re-verification recursion so a malicious chain cannot turn a single
/// verify call into an unbounded compute job. The constant is generous
/// (256 chain links is plausibly more than any honest evolution path
/// will need) but finite. A v0.2 memoization layer is expected to drop
/// the recursion cost from O(N²) to O(N), at which point this bound
/// can be raised or removed.
pub const MAX_LINEAGE_DEPTH: usize = 256;

std::thread_local! {
    static LINEAGE_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

struct LineageDepthGuard;
impl LineageDepthGuard {
    fn enter() -> Result<Self, NextGenerationError> {
        LINEAGE_DEPTH.with(|d| {
            let cur = d.get();
            if cur >= MAX_LINEAGE_DEPTH {
                Err(NextGenerationError::ParentReceiptInvalid(format!(
                    "lineage chain depth {cur} exceeds MAX_LINEAGE_DEPTH={MAX_LINEAGE_DEPTH}"
                )))
            } else {
                d.set(cur + 1);
                Ok(LineageDepthGuard)
            }
        })
    }
}
impl Drop for LineageDepthGuard {
    fn drop(&mut self) {
        LINEAGE_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

pub fn next_generation_check(input: &str) -> Result<Vec<u8>, NextGenerationError> {
    use NextGenerationError as NGE;
    // F3: bump-and-check the per-thread depth counter on entry; the
    // guard's Drop restores it on every exit path.
    let _depth_guard = LineageDepthGuard::enter()?;

    let map = parse_json(input).map_err(NGE::BadInput)?;
    let parent_b64 = get_str(&map, "parent_receipt").map_err(NGE::BadInput)?;
    let child_json = map.get("child_descriptor")
        .ok_or_else(|| NGE::BadInput("missing child_descriptor".into()))?
        .clone();

    // S1: decode parent envelope
    let envelope_bytes = crate::cli_api::base64_decode(&parent_b64)
        .map_err(|e| NGE::ParentReceiptInvalid(format!("base64: {e}")))?;
    let envelope_json = std::str::from_utf8(&envelope_bytes)
        .map_err(|_| NGE::ParentReceiptInvalid("not UTF-8".into()))?;
    let parent_receipt = crate::receipt::Receipt::from_json(envelope_json)
        .map_err(|e| NGE::ParentReceiptInvalid(format!("parse: {e}")))?;

    // S2: re-verify parent receipt
    match crate::cli_api::verify(
        &parent_receipt.computation_id,
        &parent_receipt.input,
        &parent_receipt,
    ) {
        Ok(crate::cli_api::VerifyOutcome::Ok { .. }) => {}
        Ok(crate::cli_api::VerifyOutcome::ContentMismatch { .. }) => {
            return Err(NGE::ParentReceiptInvalid("content mismatch".into()));
        }
        Err(e) => return Err(NGE::ParentReceiptInvalid(e)),
    }

    // S3: parent must be a root or a prior chain link
    let parent_cid = parent_receipt.computation_id.as_str();
    if parent_cid != "skill_interface_hash" && parent_cid != "next_generation" {
        return Err(NGE::InvalidParentComputation(parent_cid.to_string()));
    }

    // S4: extract the parent descriptor JSON
    let parent_descriptor_json = if parent_cid == "skill_interface_hash" {
        parent_receipt.input.clone()
    } else {
        let inner = parse_json(&parent_receipt.input)
            .map_err(|e| NGE::ParentReceiptInvalid(format!("inner parse: {e}")))?;
        inner.get("child_descriptor")
            .ok_or_else(|| NGE::ParentReceiptInvalid(
                "parent next_generation missing child_descriptor".into()
            ))?
            .clone()
    };

    // S5: parse both descriptors via the typed builder
    let parent_desc = descriptor_from_json(&parent_descriptor_json)
        .map_err(|e| NGE::ParentReceiptInvalid(format!("parse parent descriptor: {e}")))?;
    let child_desc = descriptor_from_json(&child_json)
        .map_err(NGE::MalformedDescriptor)?;

    // S6: refinement check
    crate::refinement::is_refinement(&parent_desc, &child_desc)
        .map_err(NGE::NotARefinement)?;

    // S7: child_interface_hash via the canonicalizer used by skill_interface_hash.
    // This returns the raw 32 BLAKE3 output bytes, never hex.
    let child_interface_hash: Vec<u8> = skill_interface_hash(&child_json)
        .map_err(NGE::MalformedDescriptor)?;

    // S8: output = BLAKE3(b"EVOLVED" || 0x00 || parent_output_hash[32] || 0x00 || child_interface_hash[32]).
    // `parent.output_hash` is hex-encoded in the envelope; decode to 32
    // raw bytes before feeding the hasher.
    let parent_hash_bytes: [u8; 32] = hex_decode_32(&parent_receipt.output_hash)
        .map_err(|e| NGE::ParentReceiptInvalid(format!("output_hash decode: {e}")))?;
    let mut buf = Vec::with_capacity(7 + 1 + 32 + 1 + 32);
    buf.extend_from_slice(b"EVOLVED");
    buf.push(0);
    buf.extend_from_slice(&parent_hash_bytes);
    buf.push(0);
    buf.extend_from_slice(&child_interface_hash);

    Ok(runtime_hash("next_generation", &buf))
}

/// String-erased wrapper used by the string-keyed `run` dispatcher.
fn next_generation(input: &str) -> Result<Vec<u8>, String> {
    next_generation_check(input).map_err(|e| e.to_string())
}

fn hex_decode_32(s: &str) -> Result<[u8; 32], String> {
    if s.len() != 64 {
        return Err(format!("expected 64 hex chars, got {}", s.len()));
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        let byte = u8::from_str_radix(&s[2*i..2*i+2], 16)
            .map_err(|_| format!("invalid hex at byte {i}"))?;
        out[i] = byte;
    }
    Ok(out)
}

// ------------------------------------------------------------------
// JSON → typed SkillDescriptor (used by next_generation)
// ------------------------------------------------------------------

pub fn descriptor_from_json(json: &str) -> Result<crate::descriptor::SkillDescriptor, String> {
    use crate::descriptor::SkillDescriptor;
    let map = parse_json(json)?;
    // H-5: reject any key not in the canonical 7-field set. Unknown
    // fields would be silently ignored by `canonicalize_descriptor`
    // (which only encodes the known set), giving an attacker a place
    // to stash unauthenticated metadata that downstream consumers
    // might read alongside a `Valid` proof. Strict schema → no
    // unsigned-fields-of-influence.
    const KNOWN: &[&str] = &[
        "content_hash", "effects", "input_shape", "name",
        "output_shape", "references", "version",
    ];
    for key in map.keys() {
        if !KNOWN.contains(&key.as_str()) {
            return Err(format!("unknown descriptor field: {key}"));
        }
    }
    let name = get_str(&map, "name")?;
    let version = get_str(&map, "version")?;
    let content_hash = get_str(&map, "content_hash")?;

    let input_shape_map = get_map(&map, "input_shape")?;
    let output_shape_map = get_map(&map, "output_shape")?;
    let input_shape = shape_from_map(&input_shape_map)?;
    let output_shape = shape_from_map(&output_shape_map)?;

    let effects_str = map.get("effects").ok_or("missing effects")?;
    let effects_raw = parse_list(effects_str)?;
    let mut effects = Vec::with_capacity(effects_raw.len());
    for e in &effects_raw {
        effects.push(effect_from_str(e)?);
    }

    let refs_str = map.get("references").ok_or("missing references")?;
    let refs = parse_list(refs_str)?;

    let mut b = SkillDescriptor::builder()
        .name(name)
        .version(version)
        .content_hash_hex(content_hash)
        .input_shape(input_shape)
        .output_shape(output_shape);
    for e in effects {
        b = b.effect(e);
    }
    for r in refs {
        b = b.reference(r);
    }
    b.build().map_err(|e| e.to_string())
}

fn shape_from_map(map: &std::collections::HashMap<String, String>) -> Result<crate::descriptor::Shape, String> {
    use crate::descriptor::{NamedField, Shape};
    let kind = get_str(map, "type")?;
    match kind.as_str() {
        "u8"  => Ok(Shape::U8  { max_bytes: get_u64(map, "max_bytes")? }),
        "u16" => Ok(Shape::U16 { max_bytes: get_u64(map, "max_bytes")? }),
        "u32" => Ok(Shape::U32 { max_bytes: get_u64(map, "max_bytes")? }),
        "u64" => Ok(Shape::U64 { max_bytes: get_u64(map, "max_bytes")? }),
        "string" => Ok(Shape::String { max_bytes: get_u64(map, "max_bytes")? }),
        "bytes"  => Ok(Shape::Bytes  { max_bytes: get_u64(map, "max_bytes")? }),
        "structured" => {
            let fields_str = map.get("fields").ok_or("structured missing fields")?;
            let field_maps = parse_list_of_maps(fields_str)?;
            let mut fields = Vec::with_capacity(field_maps.len());
            for fm in field_maps {
                let fname = get_str(&fm, "name")?;
                let shape_str = fm.get("shape").ok_or("field missing shape")?;
                let shape_inner = parse_json(shape_str)?;
                let shape = shape_from_map(&shape_inner)?;
                fields.push(NamedField { name: fname.to_string(), shape });
            }
            Ok(Shape::Structured { fields })
        }
        "list" => {
            let item_str = map.get("item").ok_or("list missing item")?;
            let item_inner = parse_json(item_str)?;
            let item = shape_from_map(&item_inner)?;
            let max_items = get_u64(map, "max_items")?;
            Ok(Shape::List { item: Box::new(item), max_items })
        }
        other => Err(format!("unknown shape: {other}")),
    }
}

fn effect_from_str(s: &str) -> Result<crate::descriptor::EffectKind, String> {
    use crate::descriptor::EffectKind;
    match s {
        "none"           => Ok(EffectKind::None),
        "file_read"      => Ok(EffectKind::FileRead),
        "file_write"     => Ok(EffectKind::FileWrite),
        "web_read"   => Ok(EffectKind::WebRead),
        "web_write"  => Ok(EffectKind::WebWrite),
        "terminal"   => Ok(EffectKind::Terminal),
        "llm"        => Ok(EffectKind::Llm),
        other            => Err(format!("unknown effect: {other}")),
    }
}

fn get_u64(map: &std::collections::HashMap<String, String>, key: &str) -> Result<u64, String> {
    get_str(map, key)?.parse::<u64>().map_err(|_| format!("{key} not u64"))
}

// ------------------------------------------------------------------
// JSON parser regression tests
// ------------------------------------------------------------------
//
// These pin the fix for the audit-flagged splitter bugs: top-level
// commas and colons inside string values must not split, and escape
// sequences (\", \\) must not terminate strings prematurely.

#[cfg(test)]
mod json_parser_tests {
    use super::*;

    #[test]
    fn comma_inside_string_does_not_split() {
        let pairs = split_json_pairs(r#""a":"one,two","b":"three""#);
        assert_eq!(pairs, vec![r#""a":"one,two""#, r#""b":"three""#]);
    }

    #[test]
    fn colon_inside_string_does_not_split() {
        let kv = split_key_value(r#""a":"x:y:z""#).expect("split");
        assert_eq!(kv.0.trim().trim_matches('"'), "a");
        assert_eq!(kv.1.trim().trim_matches('"'), "x:y:z");
    }

    #[test]
    fn escaped_quote_in_string_does_not_close_string() {
        // String value contains an escaped quote followed by a comma —
        // the comma must not be treated as a top-level delimiter.
        let pairs = split_json_pairs(r#""a":"x\"y,z","b":"q""#);
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0], r#""a":"x\"y,z""#);
        assert_eq!(pairs[1], r#""b":"q""#);
    }

    #[test]
    fn brace_inside_string_does_not_open_object() {
        let pairs = split_json_pairs(r#""a":"open{brace,here","b":1"#);
        assert_eq!(pairs.len(), 2);
    }

    #[test]
    fn parse_json_rejects_duplicate_keys() {
        let r = parse_json(r#"{"name":"a","name":"b"}"#);
        assert!(r.is_err(), "duplicate keys must error");
    }

    #[test]
    fn parse_list_handles_strings_with_commas() {
        let v = parse_list(r#"["x,y","z"]"#).unwrap();
        assert_eq!(v, vec!["x,y", "z"]);
    }

    // CRIT-3: comma inside a quoted effect string must not be smuggled into
    // an extra element.
    #[test]
    fn crit3_effects_comma_injection_distinguished() {
        // One-element list with a comma in the value.
        let v1 = parse_list(r#"["network_read,file_write"]"#).unwrap();
        assert_eq!(v1, vec!["network_read,file_write"]);
        // Two-element list (the intended form) parses to two values.
        let v2 = parse_list(r#"["network_read","file_write"]"#).unwrap();
        assert_eq!(v2, vec!["network_read", "file_write"]);
        // The two are distinct.
        assert_ne!(v1, v2);
    }

    // CRIT-3 end-to-end: the one-element form fails effect validation
    // (`"network_read,file_write"` is not in the closed vocabulary).
    // It never reaches canonicalization, so it cannot collide with the
    // two-element form's hash.
    #[test]
    fn crit3_effects_comma_injection_rejected_at_validation() {
        let bad = r#"{"content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","effects":["network_read,file_write"],"input_shape":{"type":"u8","max_bytes":1},"name":"x","output_shape":{"type":"u8","max_bytes":1},"references":[],"version":"1.0.0"}"#;
        let err = skill_interface_hash(bad).expect_err("must reject smuggled effect");
        assert!(
            err.contains("unknown effect"),
            "expected effect-vocabulary error, got {err}",
        );
    }

    // HIGH-1: duplicate name keys at the top level are rejected by the
    // outer parser, so a "name desync" between displayed and sealed
    // identity cannot occur.
    #[test]
    fn high1_duplicate_name_keys_rejected() {
        let bad = r#"{"name":"web-search","name":"evil-skill","content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","effects":["none"],"input_shape":{"type":"u8","max_bytes":1},"output_shape":{"type":"u8","max_bytes":1},"references":[],"version":"1.0.0"}"#;
        let err = skill_interface_hash(bad).expect_err("must reject duplicate keys");
        assert!(
            err.contains("duplicate key"),
            "expected duplicate-key error, got {err}",
        );
    }

    // HIGH-2: duplicate `type` keys inside a nested shape object are
    // rejected by the inner parser invocation.
    #[test]
    fn high2_duplicate_type_keys_in_shape_rejected() {
        let bad = r#"{"content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","effects":["none"],"input_shape":{"type":"u8","type":"string","max_bytes":16777216},"name":"x","output_shape":{"type":"u8","max_bytes":1},"references":[],"version":"1.0.0"}"#;
        let err = skill_interface_hash(bad).expect_err("must reject duplicate type keys");
        assert!(
            err.contains("duplicate key"),
            "expected duplicate-key error, got {err}",
        );
    }

    // MED-2: unquoted list elements are rejected. `[network_read]` is not
    // valid JSON; coercing it to `["network_read"]` would have let a
    // producer smuggle bare identifiers past the parser.
    #[test]
    fn med2_parse_list_rejects_unquoted_elements() {
        let bad = parse_list("[network_read]");
        assert!(bad.is_err(), "unquoted list element must error, got {bad:?}");
        let bad2 = parse_list(r#"[network_read,"file_write"]"#);
        assert!(bad2.is_err(), "mixed quoted/unquoted must error, got {bad2:?}");
        // Sanity: the properly-quoted form still parses.
        let ok = parse_list(r#"["network_read"]"#).unwrap();
        assert_eq!(ok, vec!["network_read"]);
    }

    #[test]
    fn parse_list_of_maps_handles_strings_with_braces() {
        let v = parse_list_of_maps(r#"[{"name":"a{b","x":"1"},{"name":"c","x":"2"}]"#).unwrap();
        assert_eq!(v.len(), 2);
        // First map's "name" must round-trip with the literal '{' inside.
        assert_eq!(v[0].get("name").unwrap().trim_matches('"'), "a{b");
    }

    #[test]
    fn skill_interface_hash_rejects_injection_attempt() {
        // Adversarial input that, under the OLD splitter, would have
        // confused the field boundaries via a `,"` inside what the
        // attacker wants to look like a name. Under the new parser
        // the boundary is preserved; the malformed name then fails
        // validate_name (regex [a-z0-9-]+).
        let bad = r#"{"content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","effects":["file_read"],"input_shape":{"type":"string","max_bytes":256},"name":"bad\",\"version\":\"99.99.99","output_shape":{"type":"string","max_bytes":4096},"references":[],"version":"1.0.0"}"#;
        let err = skill_interface_hash(bad).unwrap_err();
        assert!(
            err.contains("invalid name") || err.contains("InvalidName"),
            "expected name validation to reject the injection, got: {err}"
        );
    }
}
