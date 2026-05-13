//! Bridge between `SKILL.md` and `skill.lyra.json`.
//!
//! A `SKILL.md` is YAML frontmatter plus prose — the on-disk format
//! shared by Lyra and other skill ecosystems. The Lyra typed contract
//! and its self-verifying proof attach to a SKILL.md as two top-level
//! YAML frontmatter keys, `contract:` and `proof:`, each holding an
//! inline JSON object literal.
//!
//! ```text
//! ---
//! name: inbox-triage
//! version: "0.1.0"
//! contract: {"name":"inbox-triage","version":"0.1.0", ...}
//! proof:    {"protocol":"hermes-lyra/0.2","spec_uri":"...","output_hash":"...","runtime":"..."}
//! ---
//!
//! # inbox-triage
//! Prose readers see this as before...
//! ```
//!
//! * Markdown readers ignore the frontmatter (it's just YAML).
//! * Lyra reads only the `contract:` and `proof:` keys; the prose
//!   becomes the "skill body" — the BLAKE3 hash of which is bound
//!   into the descriptor's `content_hash`.
//!
//! No fenced-block protocol; the format is standard YAML frontmatter
//! and works with any YAML-aware tooling. The sidecar form
//! (`SKILL.md` + `skill.lyra.json`) is still supported.
//!
//! ## Public surface
//!
//! * [`extract_frontmatter_contract`] — pull the descriptor JSON out
//!   of a SKILL.md's frontmatter.
//! * [`scaffold_md_from_descriptor`] — emit a minimal SKILL.md whose
//!   frontmatter `contract:` is the given Lyra descriptor.
//! * [`bind_descriptor_to_md`] — splice `contract:` and a freshly
//!   minted `proof:` into a SKILL.md.
//! * [`verify_embedded_proof`] — re-derive the proof and surface a
//!   typed outcome (including the verified prose body).
//! * [`descriptor_from_anywhere`] — auto-detect whether an input string
//!   is descriptor JSON or SKILL.md and return the descriptor JSON
//!   regardless. Used by all the high-level agent gates to make them
//!   tolerant of either form.

/// Generate a minimal `SKILL.md` for the given descriptor. The
/// descriptor is embedded as a YAML frontmatter `contract:` key (v0.2
/// format); the prose is a stub the author can flesh
/// out. The scaffold does NOT include a proof — call
/// [`bind_descriptor_to_md`] on the result to mint and embed one.
///
/// Useful for skills that started as a typed contract and want to
/// publish a markdown skill artifact alongside.
pub fn scaffold_md_from_descriptor(descriptor_json: &str) -> Result<String, String> {
    let descriptor = crate::computations::descriptor_from_json(descriptor_json)
        .map_err(|e| format!("descriptor: {e}"))?;
    let name = descriptor.name();
    let version = descriptor.version();

    let mut out = String::with_capacity(descriptor_json.len() + 512);
    out.push_str("---\n");
    out.push_str(&format!("name: {name}\n"));
    out.push_str("description: TODO — one-line summary of what this skill does\n");
    out.push_str(&format!("version: \"{version}\"\n"));
    out.push_str("contract: ");
    out.push_str(descriptor_json.trim());
    out.push('\n');
    out.push_str("---\n\n");
    out.push_str(&format!("# {name}\n\n"));
    out.push_str("TODO — prose describing when to use, the procedure, and pitfalls.\n");
    Ok(out)
}

/// Canonical protocol identifier for this Lyra version. A verifier
/// reads `proof.protocol` to know which rule set governs the canonical
/// form and the hash. Going through the *identifier* (rather than
/// implementation-specific assumptions) makes verification independent
/// of any single reference implementation: any party that has cached
/// the `lyra/0.1` spec can re-derive the proof.
pub const LYRA_PROTOCOL: &str = "hermes-lyra/0.2";

/// A self-verifying proof embedded alongside a descriptor. Four fields:
///   * `protocol`    — names the rule set (e.g. `"hermes-lyra/0.2"`).
///   * `spec_uri`    — informational URI pointing at the canonical
///                     repo. Lets a **cold-start** agent (one with
///                     zero prior knowledge of Lyra) bootstrap: fetch
///                     the rules and the reference implementation.
///                     NOT authoritative — verifiers ignore it; the
///                     authoritative identifier is `protocol`. The
///                     URI is a hint; any mirror serving the same
///                     repo content is equally valid.
///   * `output_hash` — BLAKE3-256 over canonical descriptor bytes.
///   * `runtime`     — which implementation produced this; lets a
///                     verifier confirm byte-exact reproducibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedProof {
    pub protocol: String,
    pub spec_uri: String,
    pub output_hash: String,
    pub runtime: String,
}

/// Auto-detect whether `input` is descriptor JSON or `SKILL.md` and
/// return the **descriptor JSON** either way. Strips the proof wrapper
/// transparently — callers downstream (the typed builder, refinement
/// checks) only need the descriptor.
///
/// For SKILL.md inputs, the descriptor lives in the YAML frontmatter
/// under the `contract:` key.
pub fn descriptor_from_anywhere(input: &str) -> Result<String, String> {
    let trimmed = input.trim_start();
    if trimmed.starts_with('{') {
        // Could be a bare descriptor OR a wrapped {descriptor, proof}.
        // Detect the wrapper by checking for a top-level "descriptor" key.
        if let Ok(map) = crate::computations::parse_json(input) {
            if let Some(desc_raw) = map.get("descriptor") {
                return Ok(desc_raw.clone());
            }
        }
        return Ok(input.to_string());
    }
    if let Some(c) = extract_frontmatter_contract(input) {
        return Ok(c);
    }
    Err(
        "input has no skill contract: expected a JSON object or a SKILL.md \
         with a `contract:` key in its YAML frontmatter"
            .into(),
    )
}

/// Pull the descriptor JSON object out of a SKILL.md's YAML frontmatter
/// `contract:` field. The value MUST be a JSON object literal (inline,
/// possibly multi-line). Anything else returns `None`.
///
/// Recognized form (the v0.2 frontmatter format):
///
/// ```text
/// ---
/// name: inbox-triage
/// version: "0.1.0"
/// contract: {"name":"inbox-triage","version":"0.1.0",...}
/// proof:    {"protocol":"skill-contract/0.2",...}
/// ---
/// ```
///
/// The value is permitted to span multiple lines as long as the JSON
/// braces balance; the extractor consumes from the `{` after `contract:`
/// to the matching `}`. JSON-string contents are respected so braces
/// inside strings don't break balancing.
pub fn extract_frontmatter_contract(md: &str) -> Option<String> {
    let (front, _rest) = split_frontmatter(md)?;
    extract_yaml_inline_json(front, "contract")
}

/// Pull the proof JSON object out of a SKILL.md's YAML frontmatter
/// `proof:` field, if present. Same parsing rules as
/// [`extract_frontmatter_contract`].
pub fn extract_frontmatter_proof(md: &str) -> Option<String> {
    let (front, _rest) = split_frontmatter(md)?;
    extract_yaml_inline_json(front, "proof")
}

/// Return `(frontmatter_text, body_text)` for a SKILL.md, or `None` if
/// the file does not begin with a `---` frontmatter block. The
/// frontmatter text does NOT include the surrounding `---` lines.
fn split_frontmatter(md: &str) -> Option<(&str, &str)> {
    let s = md;
    // Strip any UTF-8 BOM the author might have written.
    let s = s.strip_prefix('\u{feff}').unwrap_or(s);
    if !s.starts_with("---") {
        return None;
    }
    // Require a newline after the opener.
    let after_open_line = s.find('\n')?;
    let rest = &s[after_open_line + 1..];
    // Find the closing `---` line. CommonMark conventions: must be at
    // the start of a line, by itself (allowing trailing whitespace).
    let mut byte_cursor = 0usize;
    for line in rest.lines() {
        let trimmed = line.trim_end();
        if trimmed == "---" || trimmed == "..." {
            let front = &rest[..byte_cursor];
            let body_start = byte_cursor + line.len();
            let body = &rest[body_start..];
            // Skip the single newline after the closing fence.
            let body = body.strip_prefix('\n').unwrap_or(body);
            return Some((front, body));
        }
        byte_cursor += line.len() + 1;
    }
    None
}

/// Hand-rolled extractor for a single YAML key whose value is an inline
/// JSON object literal. Looks for a line at the top YAML indentation
/// level that begins with `<key>:` (after optional spaces), then
/// consumes the brace-balanced JSON object starting at the first `{`
/// on or after that point. Returns the matched substring (the JSON
/// object including its outer braces).
///
/// This is intentionally minimal: it does NOT handle YAML block-style
/// values (`contract:\n  input_shape:\n    ...`). The v0.2 format
/// REQUIRES the value to be a single inline JSON literal — see
/// `extract_frontmatter_contract` doctring.
fn extract_yaml_inline_json(frontmatter: &str, key: &str) -> Option<String> {
    let mut search_pos = 0usize;
    let bytes = frontmatter.as_bytes();
    let needle = format!("{key}:");
    while search_pos < frontmatter.len() {
        // Find the next occurrence of the literal "<key>:".
        let hit = frontmatter[search_pos..].find(&needle)?;
        let abs = search_pos + hit;
        // Confirm this is at the start of a line (top-level YAML key,
        // not a nested one). Walk back to find the previous newline and
        // check that everything between it and `abs` is whitespace.
        let line_start = frontmatter[..abs].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let prefix = &frontmatter[line_start..abs];
        if !prefix.chars().all(|c| c.is_whitespace()) {
            // Not at top level, or comment, or quoted — keep searching.
            search_pos = abs + needle.len();
            continue;
        }
        // Confirm the indentation is zero (the contract/proof keys live
        // at the top of the frontmatter, not nested under another key).
        if !prefix.is_empty() {
            search_pos = abs + needle.len();
            continue;
        }
        // Find the first `{` at or after the colon.
        let mut j = abs + needle.len();
        while j < bytes.len() && bytes[j] != b'{' {
            // Bail out if we hit a newline followed by non-whitespace —
            // that's a different YAML key, not the JSON value.
            if bytes[j] == b'\n' {
                // Skip any leading whitespace on the next line.
                let mut k = j + 1;
                while k < bytes.len() && (bytes[k] == b' ' || bytes[k] == b'\t') {
                    k += 1;
                }
                if k < bytes.len() && bytes[k] != b'{' {
                    // The value is something else (a scalar, a block, etc.).
                    return None;
                }
                j = k;
                continue;
            }
            j += 1;
        }
        if j >= bytes.len() {
            return None;
        }
        // Brace-balanced scan, respecting JSON string boundaries.
        let start = j;
        let mut depth = 0i32;
        let mut in_str = false;
        let mut esc = false;
        while j < bytes.len() {
            let c = bytes[j];
            if in_str {
                if esc {
                    esc = false;
                } else if c == b'\\' {
                    esc = true;
                } else if c == b'"' {
                    in_str = false;
                }
            } else {
                match c {
                    b'"' => in_str = true,
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(frontmatter[start..=j].to_string());
                        }
                    }
                    _ => {}
                }
            }
            j += 1;
        }
        // Unbalanced braces; not a usable value.
        return None;
    }
    None
}

/// Bind a skill descriptor to a SKILL.md by writing the descriptor +
/// proof into the YAML frontmatter under `contract:` and `proof:`. If
/// the SKILL.md already carries those keys, they are replaced. If the
/// SKILL.md has no frontmatter, one is created.
///
/// Returns `(upgraded_md, proof)`. The proof is also embedded in the
/// returned markdown — the struct is for callers that want to log it
/// without re-parsing.
pub fn bind_descriptor_to_md(
    md: &str,
    descriptor_json: &str,
) -> Result<(String, EmbeddedProof), String> {
    // 1. Compute the body hash. The body is the SKILL.md text minus
    //    the frontmatter contract/proof keys, so re-binding produces
    //    the same body hash. Per spec, `descriptor.content_hash` is
    //    BLAKE3-256 of the skill body — we bind that here so the
    //    proof transitively attests the body.
    let body = extract_skill_body(md);
    let body_hash_hex = hex_lower(blake3::hash(body_canonical_bytes(&body)).as_bytes());

    // 2. Rewrite the descriptor's content_hash to the body-derived
    //    value. The author's input is advisory; content_hash is by
    //    spec a derived attestation.
    let descriptor_json = rewrite_content_hash(descriptor_json, &body_hash_hex)?;

    // 3. Validate the (post-rewrite) descriptor through the typed
    //    path. Catches malformed input with a clean error.
    let receipt = crate::cli_api::score("skill_interface_hash", &descriptor_json)
        .map_err(|e| format!("descriptor invalid: {e}"))?;
    let proof = EmbeddedProof {
        protocol: LYRA_PROTOCOL.to_string(),
        spec_uri: crate::LYRA_SPEC_URI.to_string(),
        output_hash: receipt.output_hash.clone(),
        runtime: receipt.runtime.clone(),
    };

    // 4. Build the proof JSON line. Field order: protocol, spec_uri,
    //    output_hash, runtime.
    let proof_json = format!(
        r#"{{"protocol":"{}","spec_uri":"{}","output_hash":"{}","runtime":"{}"}}"#,
        proof.protocol,
        json_escape(&proof.spec_uri),
        proof.output_hash,
        json_escape(&proof.runtime),
    );

    // 5. Splice `contract:` and `proof:` into the frontmatter.
    let upgraded = splice_frontmatter_keys(md, descriptor_json.trim(), &proof_json);
    Ok((upgraded, proof))
}

/// Insert (or replace) the `contract:` and `proof:` keys in a SKILL.md's
/// YAML frontmatter. If the file has no frontmatter, a minimal one is
/// created. The two keys are written as the last two frontmatter keys
/// so author-written keys at the top stay where the author put them.
fn splice_frontmatter_keys(md: &str, contract_json: &str, proof_json: &str) -> String {
    // No frontmatter? Synthesize one and append the original body.
    let Some((front, body)) = split_frontmatter(md) else {
        let mut out = String::with_capacity(md.len() + contract_json.len() + proof_json.len() + 64);
        out.push_str("---\n");
        out.push_str("contract: ");
        out.push_str(contract_json);
        out.push('\n');
        out.push_str("proof:    ");
        out.push_str(proof_json);
        out.push('\n');
        out.push_str("---\n\n");
        out.push_str(md.trim_start_matches('\u{feff}'));
        return out;
    };
    // Strip any existing contract:/proof: lines (and their inline-JSON
    // continuation lines) from the frontmatter. Then re-append the new
    // ones at the end.
    let cleaned_front = remove_frontmatter_key(front, "contract");
    let cleaned_front = remove_frontmatter_key(&cleaned_front, "proof");
    let mut out = String::with_capacity(md.len() + contract_json.len() + proof_json.len() + 64);
    out.push_str("---\n");
    out.push_str(cleaned_front.trim_end());
    if !cleaned_front.trim_end().is_empty() {
        out.push('\n');
    }
    out.push_str("contract: ");
    out.push_str(contract_json);
    out.push('\n');
    out.push_str("proof:    ");
    out.push_str(proof_json);
    out.push('\n');
    out.push_str("---\n");
    if !body.starts_with('\n') {
        out.push('\n');
    }
    out.push_str(body);
    out
}

/// Drop the line(s) carrying a top-level YAML key whose value is an
/// inline JSON object. Walks the frontmatter line by line; when it
/// finds `<key>:` at zero indent, it consumes that line and any
/// continuation lines until the JSON braces balance. Other lines are
/// kept verbatim. Returns a fresh owned String.
fn remove_frontmatter_key(frontmatter: &str, key: &str) -> String {
    let needle = format!("{key}:");
    let mut out = String::with_capacity(frontmatter.len());
    let mut lines = frontmatter.split_inclusive('\n').peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start_matches(|c: char| c == ' ' || c == '\t');
        // Only treat as our key if zero indent AND starts with `<key>:`.
        let is_zero_indent = line.len() == trimmed.len();
        if is_zero_indent && trimmed.starts_with(&needle) {
            // Skip continuation lines until the braces on/after this
            // line balance, or we hit another top-level key, or EOF.
            let mut depth = 0i32;
            let mut in_str = false;
            let mut esc = false;
            let mut started = false;
            for &b in trimmed.as_bytes() {
                if in_str {
                    if esc { esc = false; }
                    else if b == b'\\' { esc = true; }
                    else if b == b'"' { in_str = false; }
                } else {
                    match b {
                        b'"' => in_str = true,
                        b'{' => { depth += 1; started = true; }
                        b'}' => {
                            depth -= 1;
                            if started && depth == 0 { break; }
                        }
                        _ => {}
                    }
                }
            }
            // If we didn't see any `{`, this line might be a scalar
            // value (e.g. `proof: skipped`). Just drop the one line.
            if !started {
                continue;
            }
            // If braces balanced on this line, drop just this line.
            if depth == 0 {
                continue;
            }
            // Otherwise consume continuation lines.
            while let Some(cont) = lines.peek() {
                let cb = cont.as_bytes();
                // Stop if we see another top-level YAML key (zero indent,
                // alpha char, then a colon before any `{`).
                if cb.first().is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
                    && looks_like_top_level_key(cont)
                {
                    break;
                }
                // Consume and continue brace-scan.
                let line2 = lines.next().unwrap();
                for &b in line2.as_bytes() {
                    if in_str {
                        if esc { esc = false; }
                        else if b == b'\\' { esc = true; }
                        else if b == b'"' { in_str = false; }
                    } else {
                        match b {
                            b'"' => in_str = true,
                            b'{' => depth += 1,
                            b'}' => {
                                depth -= 1;
                                if depth == 0 { break; }
                            }
                            _ => {}
                        }
                    }
                }
                if depth == 0 { break; }
            }
            continue;
        }
        out.push_str(line);
    }
    out
}

/// Heuristic: does this line look like a top-level YAML key
/// declaration? Used to stop continuation-consumption of the
/// previous key's inline-JSON value.
fn looks_like_top_level_key(line: &str) -> bool {
    // Zero-indented, has `:` before any `{` or `"`.
    if line.starts_with(' ') || line.starts_with('\t') {
        return false;
    }
    for c in line.chars() {
        match c {
            ':' => return true,
            '{' | '"' | '[' => return false,
            _ => {}
        }
    }
    false
}

/// Body hash domain: trailing whitespace is not part of the body
/// content. `append_lyra_block` and `replace_lyra_block` add/remove
/// blank lines around the fence as part of CommonMark formatting; the
/// content-hash must be stable across those operations so that
/// `bind(bind(x)) == bind(x)` at the proof level. Trimming trailing
/// whitespace at the *single* hashing boundary keeps both certify and
/// verify in lockstep without touching how the body is laid out on
/// disk or shown to the agent.
fn body_canonical_bytes(body: &str) -> &[u8] {
    body.trim_end_matches(|c: char| c.is_ascii_whitespace()).as_bytes()
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

/// Re-emit a descriptor JSON object with `content_hash` set to the
/// given 64-hex value. The rest of the object is preserved verbatim
/// (canonical key order is restored after parse → re-encode via the
/// typed path in `descriptor_from_json` → `to_canonical_json`).
fn rewrite_content_hash(descriptor_json: &str, new_hex: &str) -> Result<String, String> {
    let map = crate::computations::parse_json(descriptor_json)
        .map_err(|e| format!("descriptor JSON parse: {e}"))?;
    // Rebuild in canonical alphabetical key order; only content_hash
    // changes. We splice as a raw string because keys map to *raw* JSON
    // value tokens (preserves nested object/array structure exactly).
    let key_order = [
        "content_hash", "effects", "input_shape", "name",
        "output_shape", "references", "version",
    ];
    let mut out = String::with_capacity(descriptor_json.len());
    out.push('{');
    let mut first = true;
    for k in &key_order {
        let v_raw = if *k == "content_hash" {
            format!(r#""{new_hex}""#)
        } else {
            match map.get(*k) {
                Some(v) => v.clone(),
                None => continue,
            }
        };
        if !first { out.push(','); }
        first = false;
        out.push('"');
        out.push_str(k);
        out.push_str("\":");
        out.push_str(&v_raw);
    }
    out.push('}');
    Ok(out)
}

/// Outcome of `verify_embedded_proof`. Distinct variants make the
/// distinction load-bearing in the type system: a forged descriptor,
/// an unsupported protocol, and a substrate incompatibility are
/// different problems that callers route differently.
///
/// **Gate-as-loader.** On `Valid`, the outcome carries the SKILL.md
/// prose body (the markdown with the `lyra` fenced block removed) so
/// the agent's *only* path to the prose is the verified path. Calling
/// without verifying yields no body; the verification step cannot be
/// skipped while still getting executable instructions.
///
/// Caching is implemented internally (content-hash memo cache) but
/// not exposed on the wire — repeat verifies are an internal
/// optimization, not part of the protocol contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyOutcome {
    /// Proof, protocol, substrate, and re-derived hash all agree.
    /// `body` is the SKILL.md prose minus the `lyra` fenced block,
    /// ready for the agent's LLM to read as the procedure.
    Valid { body: String },
    /// Re-derived `output_hash` does not match the embedded proof.
    /// Descriptor or proof was tampered.
    Mismatch,
    /// The block carries a descriptor but no embedded proof — legacy
    /// bare-descriptor form. Callers may accept under explicit policy.
    NoProof,
    /// The proof was minted under a protocol identifier this build
    /// does not implement. NOT a forgery — a future-version proof is
    /// rejected here so the caller can decide to fetch the future spec.
    UnsupportedProtocol { proof: String, verifier: String },
    /// The proof's `runtime` substrate is not in this build's
    /// `COMPATIBLE_RUNTIMES` set.
    SubstrateIncompatible { proof: String, verifier: String },
}

impl VerifyOutcome {
    /// Short status token suitable for JSON wire / CLI exit-code mapping.
    pub fn status(&self) -> &'static str {
        match self {
            VerifyOutcome::Valid { .. }               => "valid",
            VerifyOutcome::Mismatch                   => "mismatch",
            VerifyOutcome::NoProof                    => "no_proof",
            VerifyOutcome::UnsupportedProtocol { .. } => "unsupported_protocol",
            VerifyOutcome::SubstrateIncompatible {..} => "substrate_incompatible",
        }
    }
}

/// Return the SKILL.md prose with the `lyra` fenced block removed —
/// The "skill body" for hashing/auditing purposes: the SKILL.md text
/// with the frontmatter `contract:` and `proof:` keys stripped out, so
/// re-binding the same descriptor produces the same body hash and the
/// proof transitively attests the entire human-readable artifact (the
/// other frontmatter keys, the prose, everything outside the contract/
/// proof keys themselves). For files with no frontmatter, returns the
/// input unchanged.
pub fn extract_skill_body(md: &str) -> String {
    let Some((front, body)) = split_frontmatter(md) else {
        return md.to_string();
    };
    let cleaned = remove_frontmatter_key(front, "contract");
    let cleaned = remove_frontmatter_key(&cleaned, "proof");
    let mut out = String::with_capacity(md.len());
    out.push_str("---\n");
    out.push_str(cleaned.trim_end());
    if !cleaned.trim_end().is_empty() {
        out.push('\n');
    }
    out.push_str("---\n");
    if !body.is_empty() && !body.starts_with('\n') {
        out.push('\n');
    }
    out.push_str(body);
    out
}

// ============================================================
// Verify cache (gate-as-loader, low-latency repeat verifications)
// ============================================================
//
// Keyed by BLAKE3 of the input SKILL.md bytes. Same bytes in →
// same outcome out, deterministically. Cache hits skip all
// re-derivation and return in microseconds; cache misses pay the
// full ~1ms verify cost. The cache is process-local — no shared
// state, no cross-process trust, no invalidation needed (a
// different content produces a different key).

static VERIFY_CACHE: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<[u8; 32], VerifyOutcome>>,
> = std::sync::OnceLock::new();

fn verify_cache() -> &'static std::sync::Mutex<
    std::collections::HashMap<[u8; 32], VerifyOutcome>,
> {
    VERIFY_CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Re-derive the embedded proof from the descriptor inside `md` and
/// compare across three anchoring axes — protocol, runtime, output_hash.
/// Returns a typed [`VerifyOutcome`].
///
/// **Content-hash memoization**: identical input bytes hit a
/// process-local cache and return in microseconds. Repeated skill
/// loads in a long-running agent process pay verification cost once.
pub fn verify_embedded_proof(md: &str) -> Result<VerifyOutcome, String> {
    let key = *blake3::hash(md.as_bytes()).as_bytes();
    // Fast path: cache hit. Caching is internal; the wire shape does
    // not advertise whether a given response was served from cache.
    {
        let cache = verify_cache().lock().expect("verify cache poisoned");
        if let Some(hit) = cache.get(&key) {
            return Ok(hit.clone());
        }
    }
    let fresh = verify_embedded_proof_inner(md)?;
    {
        let mut cache = verify_cache().lock().expect("verify cache poisoned");
        // Audit M-10: bound the cache. Without a cap, a long-running
        // server fed adversarially-varied SKILL.md inputs accumulates
        // entries unboundedly (one per distinct BLAKE3 key) — a slow
        // memory-exhaustion path. The cap is a simple FIFO-by-insertion
        // proxy: when we hit MAX_VERIFY_CACHE entries, drop one
        // arbitrary entry (HashMap iteration order) before inserting.
        // Loose policy is fine here — the cache is an optimization,
        // never a correctness boundary.
        if cache.len() >= MAX_VERIFY_CACHE {
            if let Some(victim) = cache.keys().next().cloned() {
                cache.remove(&victim);
            }
        }
        cache.insert(key, fresh.clone());
    }
    Ok(fresh)
}

/// Soft cap on the in-process verify-result memo cache. 4096 entries
/// covers any sane agent's hot working set of skills while bounding
/// worst-case memory to ~megabytes. Reads remain O(1); the rare
/// insertion that hits the cap pays one extra HashMap removal.
const MAX_VERIFY_CACHE: usize = 4096;

fn verify_embedded_proof_inner(md: &str) -> Result<VerifyOutcome, String> {
    // Read the descriptor and proof from the frontmatter. Missing either
    // collapses to NoProof — the artifact is not self-verifying.
    let Some(descriptor_json) = extract_frontmatter_contract(md) else {
        return Ok(VerifyOutcome::NoProof);
    };
    let Some(proof_json) = extract_frontmatter_proof(md) else {
        return Ok(VerifyOutcome::NoProof);
    };
    let proof_map = crate::computations::parse_json(&proof_json)
        .map_err(|e| format!("proof is not JSON: {e}"))?;
    let proof = EmbeddedProof {
        protocol: match proof_map.get("protocol") {
            Some(s) => unquote_str(s)?,
            None => LYRA_PROTOCOL.to_string(),
        },
        spec_uri: match proof_map.get("spec_uri") {
            Some(s) => unquote_str(s)?,
            None => crate::LYRA_SPEC_URI.to_string(),
        },
        output_hash: unquote_str(
            proof_map.get("output_hash").ok_or("proof missing output_hash")?,
        )?,
        runtime: unquote_str(proof_map.get("runtime").ok_or("proof missing runtime")?)?,
    };
    // (1) Protocol identifier.
    if proof.protocol != LYRA_PROTOCOL {
        return Ok(VerifyOutcome::UnsupportedProtocol {
            proof: proof.protocol,
            verifier: LYRA_PROTOCOL.to_string(),
        });
    }
    // (2) Substrate compatibility.
    if !crate::runtime_is_compatible(&proof.runtime) {
        return Ok(VerifyOutcome::SubstrateIncompatible {
            proof: proof.runtime,
            verifier: crate::LYRA_RUNTIME_IDENT.to_string(),
        });
    }
    // (3) Re-derive the descriptor hash.
    let receipt = crate::cli_api::score("skill_interface_hash", &descriptor_json)
        .map_err(|e| format!("re-deriving hash: {e}"))?;
    if receipt.output_hash != proof.output_hash {
        return Ok(VerifyOutcome::Mismatch);
    }
    // (4) Body-hash binding. The descriptor's `content_hash` is
    //     BLAKE3-256 of the skill body; `bind_descriptor_to_md` enforces
    //     this at bind time. Re-derive and compare — if an attacker
    //     edited the SKILL.md prose after binding, the body hash
    //     diverges and we surface a Mismatch.
    let body = extract_skill_body(md);
    let body_hash_hex = hex_lower(blake3::hash(body_canonical_bytes(&body)).as_bytes());
    let descriptor_map = crate::computations::parse_json(&descriptor_json)
        .map_err(|e| format!("descriptor parse: {e}"))?;
    let claimed_hash = descriptor_map
        .get("content_hash")
        .ok_or("descriptor missing content_hash")?;
    let claimed_hash_unquoted = unquote_str(claimed_hash)?;
    if claimed_hash_unquoted != body_hash_hex {
        return Ok(VerifyOutcome::Mismatch);
    }
    Ok(VerifyOutcome::Valid { body })
}

// ---- internal helpers ----

/// Delegates to the canonical JSON string decoder. Previously this was
/// a strip-only implementation that returned escape sequences verbatim
/// — safe only because callers happened to pass hex strings. Now uses
/// the same decoder as every other JSON edge.
fn unquote_str(token: &str) -> Result<String, String> {
    crate::computations::unquote_json_string(token)
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        match c {
            '"' | '\\' => { out.push('\\'); out.push(c); }
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // A minimal, well-formed descriptor used across the round-trip tests.
    // `bind_descriptor_to_md` will rewrite `content_hash` to the body's
    // BLAKE3, so the literal here is just a placeholder of the right shape.
    const VALID_DESCRIPTOR: &str = r#"{"name":"sample","version":"1.0.0","content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","effects":["none"],"input_shape":{"type":"u8","max_bytes":1},"output_shape":{"type":"u8","max_bytes":1},"references":[]}"#;

    fn fresh_md() -> String {
        "---\nname: sample\nversion: \"1.0.0\"\n---\n\n# sample\nProse.\n".into()
    }

    #[test]
    fn scaffold_round_trip_via_frontmatter() {
        let md = scaffold_md_from_descriptor(VALID_DESCRIPTOR).expect("scaffold");
        assert!(md.starts_with("---\n"), "scaffold must emit frontmatter");
        assert!(md.contains("contract: "), "scaffold must include contract: key");
        let extracted = descriptor_from_anywhere(&md).expect("extract");
        assert!(extracted.contains("\"name\":\"sample\""));
    }

    #[test]
    fn bind_writes_frontmatter_contract_and_proof() {
        let md = fresh_md();
        let (upgraded, _proof) = bind_descriptor_to_md(&md, VALID_DESCRIPTOR).unwrap();
        // YAML-style keys appear at top of the frontmatter, NOT a code fence.
        assert!(upgraded.contains("\ncontract: {"), "missing contract: key: {upgraded}");
        assert!(upgraded.contains("\nproof:"), "missing proof: key: {upgraded}");
        // The original frontmatter keys must be preserved.
        assert!(upgraded.contains("name: sample"));
        assert!(upgraded.contains("version: \"1.0.0\""));
        // No fenced lyra block left.
        assert!(!upgraded.contains("```lyra"), "no fenced block expected: {upgraded}");
    }

    #[test]
    fn bind_creates_frontmatter_if_missing() {
        // SKILL.md with no `---` frontmatter at all.
        let md = "# sample\nJust prose, no frontmatter.\n";
        let (upgraded, _) = bind_descriptor_to_md(md, VALID_DESCRIPTOR).unwrap();
        assert!(upgraded.starts_with("---\n"), "must synthesize frontmatter: {upgraded}");
        assert!(upgraded.contains("contract: {"));
        assert!(upgraded.contains("proof:"));
        // Original prose preserved verbatim after the new frontmatter.
        assert!(upgraded.contains("# sample"));
        assert!(upgraded.contains("Just prose"));
    }

    #[test]
    fn bind_replaces_existing_contract_and_proof() {
        let md = fresh_md();
        let (once, _) = bind_descriptor_to_md(&md, VALID_DESCRIPTOR).unwrap();
        let (twice, _) = bind_descriptor_to_md(&once, VALID_DESCRIPTOR).unwrap();
        // Exactly one contract: and one proof: line — no duplication.
        let n_contract = twice.matches("\ncontract: ").count();
        let n_proof = twice.matches("\nproof:").count();
        assert_eq!(n_contract, 1, "must replace not append contract:\n{twice}");
        assert_eq!(n_proof, 1, "must replace not append proof:\n{twice}");
    }

    #[test]
    fn bind_is_idempotent_at_proof_level() {
        // Re-binding the same descriptor to the same body must yield the
        // same proof.output_hash (deterministic substrate).
        let md = fresh_md();
        let (md1, p1) = bind_descriptor_to_md(&md, VALID_DESCRIPTOR).unwrap();
        let (_md2, p2) = bind_descriptor_to_md(&md1, VALID_DESCRIPTOR).unwrap();
        assert_eq!(p1.output_hash, p2.output_hash);
        assert_eq!(p1.runtime, p2.runtime);
    }

    #[test]
    fn bind_then_verify_returns_valid_with_body() {
        let md = fresh_md();
        let (upgraded, _proof) = bind_descriptor_to_md(&md, VALID_DESCRIPTOR).unwrap();
        match verify_embedded_proof(&upgraded).unwrap() {
            VerifyOutcome::Valid { body } => {
                // The body excludes the contract:/proof: frontmatter keys
                // but includes the prose and other frontmatter keys.
                assert!(body.contains("name: sample"));
                assert!(body.contains("# sample"));
                assert!(body.contains("Prose."));
                assert!(!body.contains("contract:"));
                assert!(!body.contains("proof:"));
                assert!(!body.contains("output_hash"));
            }
            other => panic!("expected Valid, got {other:?}"),
        }
    }

    #[test]
    fn verify_detects_tampered_descriptor() {
        let md = fresh_md();
        let (upgraded, _) = bind_descriptor_to_md(&md, VALID_DESCRIPTOR).unwrap();
        // Flip the first hex char of content_hash inside the contract: line.
        // The bound descriptor's content_hash is the BLAKE3 of the body; we
        // mutate to a different (but still well-formed 64-hex) value so the
        // proof's output_hash diverges from what re-derivation produces.
        let pat = "\"content_hash\":\"";
        let i = upgraded.find(pat).expect("content_hash present");
        let start = i + pat.len();
        // Swap first hex digit: any char 0-9a-f to its complement.
        let orig = upgraded.as_bytes()[start] as char;
        let flipped = if orig == '0' { '1' } else { '0' };
        let mut t = String::with_capacity(upgraded.len());
        t.push_str(&upgraded[..start]);
        t.push(flipped);
        t.push_str(&upgraded[start + 1..]);
        match verify_embedded_proof(&t).unwrap() {
            VerifyOutcome::Mismatch => (),
            other => panic!("expected Mismatch, got {other:?}"),
        }
    }

    #[test]
    fn verify_returns_no_proof_when_frontmatter_lacks_proof() {
        let md = fresh_md();
        let (upgraded, _) = bind_descriptor_to_md(&md, VALID_DESCRIPTOR).unwrap();
        // Strip the proof: line and everything until the closing brace.
        let stripped: String = upgraded.lines()
            .filter(|line| !line.starts_with("proof:"))
            .collect::<Vec<_>>()
            .join("\n") + "\n";
        match verify_embedded_proof(&stripped).unwrap() {
            VerifyOutcome::NoProof => (),
            other => panic!("expected NoProof, got {other:?}"),
        }
    }

    #[test]
    fn descriptor_from_anywhere_accepts_bare_json() {
        let desc = descriptor_from_anywhere(VALID_DESCRIPTOR).unwrap();
        assert!(desc.contains("\"name\":\"sample\""));
    }

    #[test]
    fn descriptor_from_anywhere_strips_proof_wrapper() {
        let wrapped = format!(
            r#"{{"descriptor":{VALID_DESCRIPTOR},"proof":{{"protocol":"hermes-lyra/0.2","output_hash":"0","runtime":"x"}}}}"#
        );
        let desc = descriptor_from_anywhere(&wrapped).unwrap();
        assert!(desc.contains("\"name\":\"sample\""));
        assert!(!desc.contains("\"proof\""), "wrapper must be stripped");
    }

    #[test]
    fn descriptor_from_anywhere_errors_on_md_without_contract() {
        let md = "---\nname: bare\n---\n\n# bare\nNo contract here.\n";
        let err = descriptor_from_anywhere(md).unwrap_err();
        assert!(err.contains("no skill contract"), "unexpected err: {err}");
    }

    #[test]
    fn extract_frontmatter_contract_finds_inline_json() {
        let md = "---\nname: x\ncontract: {\"a\":1,\"b\":2}\n---\n";
        let c = extract_frontmatter_contract(md).expect("found");
        assert_eq!(c, r#"{"a":1,"b":2}"#);
    }

    #[test]
    fn extract_frontmatter_contract_handles_multiline_json() {
        let md = "---\nname: x\ncontract: {\n  \"a\": 1,\n  \"b\": 2\n}\n---\n";
        let c = extract_frontmatter_contract(md).expect("found");
        assert!(c.starts_with('{') && c.ends_with('}'));
        assert!(c.contains("\"a\""));
        assert!(c.contains("\"b\""));
    }

    #[test]
    fn extract_frontmatter_contract_returns_none_when_absent() {
        let md = "---\nname: x\n---\n\nBody.\n";
        assert!(extract_frontmatter_contract(md).is_none());
    }
}
