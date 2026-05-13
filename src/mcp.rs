//! Minimal MCP (Model Context Protocol) server.
//!
//! Speaks **newline-delimited JSON-RPC 2.0 over stdio** as defined at
//! <https://modelcontextprotocol.io/specification/2025-06-18/>.
//!
//! Exposes the five canonical gates — the entire agent-facing protocol —
//! over any MCP client:
//!
//! * `skill_bind` — bind a typed contract + self-verifying proof to a SKILL.md.
//! * `skill_verify`  — re-derive the embedded proof against the embedded descriptor.
//! * `skill_refine`  — refinement gate (R1–R5): promote or roll back.
//! * `skill_compose` — composition gate (pair or N-skill chain).
//! * `skill_merge`    — atomic skill composition; producer ∘ consumer into a new bound SKILL.md.
//!
//! No extra dependencies. The JSON-RPC envelope is hand-rolled; the heavy
//! lifting (`score`, `verify`, refinement, lineage) is the same typed,
//! UOR-anchored code path the CLI already uses.
//!
//! Notifications (`notifications/initialized`, `notifications/cancelled`,
//! anything else without an `id`) are silently dropped per JSON-RPC 2.0.
//!
//! ## Wire summary
//!
//! ```text
//! → {"jsonrpc":"2.0","id":1,"method":"initialize","params":{...}}
//! ← {"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"...","capabilities":{"tools":{}},"serverInfo":{...}}}
//!
//! → {"jsonrpc":"2.0","id":2,"method":"tools/list"}
//! ← {"jsonrpc":"2.0","id":2,"result":{"tools":[ ... ]}}
//!
//! → {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"lyra_score","arguments":{...}}}
//! ← {"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"..."}],"isError":false}}
//! ```

use std::io::{BufRead, Write};

// The MCP server no longer exposes the low-level `score` / `verify`
// computation primitives — those are CLI/library affordances. The
// agent surface is exactly the five gates: certify, verify, refine,
// compose, fuse. All five are implemented in `crate::bridge`, `crate::fuse`, and
// `crate::tripwire` and reached via their typed result types.

/// Preferred MCP protocol version. We return this when the client
/// doesn't request a specific version or requests one we don't recognize.
const PREFERRED_PROTOCOL_VERSION: &str = "2025-06-18";

/// MCP protocol versions this server can speak. We echo the client's
/// requested version back if it appears in this list (per the spec's
/// negotiation rule); otherwise we return `PREFERRED_PROTOCOL_VERSION`.
const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &[
    "2024-11-05",
    "2025-03-26",
    "2025-06-18",
];

const SERVER_NAME: &str    = "lyra";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Hard cap on a single JSON-RPC request line. A misbehaving or hostile
/// client could otherwise stream an unbounded line and force the server
/// to buffer it. 1 MiB comfortably fits every legitimate Lyra call
/// (SKILL.md bodies are bounded by the 16 MiB capacity rule but the
/// gates accept descriptor JSON, not bodies); anything larger is
/// rejected with JSON-RPC error -32600 and the offending bytes are
/// drained to the next newline so the loop stays in sync.
const MAX_REQUEST_BYTES: usize = 1 << 20;

/// Run the stdio JSON-RPC loop until EOF.
///
/// Blocks the calling thread. Returns `Ok(())` on clean EOF; returns an
/// IO error only on a stdin/stdout failure, never on a malformed
/// request (those produce a JSON-RPC error response instead).
pub fn serve_stdio() -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut input = stdin.lock();
    let mut out = stdout.lock();
    loop {
        match read_capped_line(&mut input, MAX_REQUEST_BYTES)? {
            CappedLine::Eof => return Ok(()),
            CappedLine::Line(s) => {
                let trimmed = s.trim();
                if trimmed.is_empty() { continue; }
                if let Some(resp) = handle_message(trimmed) {
                    writeln!(out, "{resp}")?;
                    out.flush()?;
                }
            }
            CappedLine::TooLarge => {
                // We don't know the id (we never parsed the line), so
                // emit a null-id error per JSON-RPC 2.0 §5.1.
                let resp = error_response(
                    "null",
                    -32600,
                    &format!("request exceeds {MAX_REQUEST_BYTES}-byte cap"),
                );
                writeln!(out, "{resp}")?;
                out.flush()?;
            }
        }
    }
}

enum CappedLine {
    Line(String),
    TooLarge,
    Eof,
}

/// Read one newline-terminated line, but refuse to buffer more than
/// `cap` bytes. On overflow, drain the rest of the line (up to EOF or
/// the next `\n`) so the next call resumes on a fresh record. Invalid
/// UTF-8 is reported as `TooLarge` for simplicity; legitimate JSON-RPC
/// is UTF-8 by spec.
fn read_capped_line<R: BufRead>(r: &mut R, cap: usize) -> std::io::Result<CappedLine> {
    let mut buf: Vec<u8> = Vec::new();
    let mut overflow = false;
    loop {
        let mut byte = [0u8; 1];
        let n = r.read(&mut byte)?;
        if n == 0 {
            if buf.is_empty() && !overflow {
                return Ok(CappedLine::Eof);
            }
            break;
        }
        if byte[0] == b'\n' {
            break;
        }
        if !overflow {
            if buf.len() >= cap {
                overflow = true;
                buf.clear();
            } else {
                buf.push(byte[0]);
            }
        }
    }
    if overflow {
        return Ok(CappedLine::TooLarge);
    }
    match String::from_utf8(buf) {
        Ok(s) => Ok(CappedLine::Line(s)),
        Err(_) => Ok(CappedLine::TooLarge),
    }
}

/// Dispatch a single JSON-RPC request line. Returns `Some(response)` for
/// requests (have an `id`), `None` for notifications.
pub fn handle_message(req: &str) -> Option<String> {
    let map = match crate::computations::parse_json(req) {
        Ok(m) => m,
        Err(e) => return Some(error_response("null", -32700, &format!("parse error: {e}"))),
    };

    // JSON-RPC 2.0 §5: jsonrpc field MUST be the string "2.0". Reject
    // anything else with -32600 (invalid request) — accepting "1.0" or
    // unversioned envelopes silently confuses logs and downstream
    // clients that branch on the version.
    let jsonrpc_ok = match map.get("jsonrpc").map(|s| unquote(s)) {
        Some(Ok(v)) => v == "2.0",
        _ => false,
    };
    if !jsonrpc_ok {
        let id = map.get("id").cloned().unwrap_or_else(|| "null".to_string());
        let id_safe = sanitize_id(&id);
        return Some(error_response(&id_safe, -32600, "jsonrpc must be \"2.0\""));
    }

    // The `id` is spliced verbatim into the response envelope. JSON-RPC
    // 2.0 §4.2 restricts id to string, number, or null — reject objects
    // and arrays before they reach the formatter, so a hostile client
    // cannot inject structural JSON into our response.
    let id_raw = map.get("id").cloned();
    if let Some(ref raw) = id_raw {
        if !is_valid_id_token(raw) {
            return Some(error_response(
                "null",
                -32600,
                "id must be string, number, or null",
            ));
        }
    }

    let method = match map.get("method").map(|s| unquote(s)) {
        Some(Ok(m)) => m,
        Some(Err(e)) => return id_raw.map(|i| error_response(&i, -32600, &format!("bad method: {e}"))),
        None => return id_raw.map(|i| error_response(&i, -32600, "missing method")),
    };
    let params_raw = map.get("params").cloned().unwrap_or_else(|| "{}".to_string());

    // No id → notification → drop silently per JSON-RPC 2.0.
    let id = id_raw.as_deref()?;

    Some(match method.as_str() {
        "initialize"   => handle_initialize(id, &params_raw),
        "tools/list"   => handle_tools_list(id),
        "tools/call"   => handle_tools_call(id, &params_raw),
        "ping"         => format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{{}}}}"#),
        other          => error_response(id, -32601, &format!("method not found: {other}")),
    })
}

// -------------------- initialize / tools/list --------------------

/// Per the MCP spec's version negotiation rule:
///   * If the client requests a version we support, echo it back.
///   * Otherwise return our preferred version. The client is then free
///     to disconnect if it cannot speak it.
fn negotiate_protocol_version(params: &str) -> &'static str {
    let map = match crate::computations::parse_json(params) {
        Ok(m) => m,
        Err(_) => return PREFERRED_PROTOCOL_VERSION,
    };
    let requested = match map.get("protocolVersion").map(|s| unquote(s)) {
        Some(Ok(v)) => v,
        _ => return PREFERRED_PROTOCOL_VERSION,
    };
    for &v in SUPPORTED_PROTOCOL_VERSIONS {
        if v == requested {
            return v;
        }
    }
    PREFERRED_PROTOCOL_VERSION
}

fn handle_initialize(id: &str, params: &str) -> String {
    let version = negotiate_protocol_version(params);
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"result":{{"protocolVersion":"{version}","capabilities":{{"tools":{{}}}},"serverInfo":{{"name":"{SERVER_NAME}","version":"{SERVER_VERSION}"}}}}}}"#,
    )
}

// The tools manifest. **Four canonical gates** — the entire agent
// surface for Lyra. Each gate is one specific operation on any
// SKILL.md (or descriptor) artifact:
//
//   * skill_bind   — embed a typed contract + self-verifying proof
//   * skill_verify   — re-derive the embedded proof; return descriptor
//   * skill_refine   — refinement gate (R1–R5): promote or roll back
//   * skill_compose  — composition gate: pair OR N-skill chain
//
// Lower-level computation primitives (`score`, `verify(receipt)`) are
// not exposed via MCP — they are CLI/library affordances for power
// users. Agents call the five gates above.
const TOOLS_JSON: &str = r##"[{"name":"skill_bind","description":"Bind a typed contract and a self-verifying proof (BLAKE3 output_hash + runtime ident) into a SKILL.md. With skill_md, upgrades the existing file's `lyra` fenced block. Without skill_md, scaffolds a fresh SKILL.md from the descriptor. Idempotent: same descriptor produces the same proof. Returns the upgraded SKILL.md plus the proof.","inputSchema":{"type":"object","required":["descriptor"],"properties":{"skill_md":{"type":"string","description":"Existing SKILL.md content. Optional."},"descriptor":{"type":"object","description":"Typed Lyra contract for the skill."}}}},{"name":"skill_verify","description":"Re-derive the proof embedded in a SKILL.md against its embedded descriptor. Local, no network. Returns status=valid (with the descriptor), status=mismatch (descriptor or proof tampered), or status=no_proof (SKILL.md carries no embedded contract).","inputSchema":{"type":"object","required":["skill_md"],"properties":{"skill_md":{"type":"string","description":"Full SKILL.md content."}}}},{"name":"skill_refine","description":"Refinement gate. Verifies the Liskov-substitutability rules R1–R5 between a parent skill and a proposed child: R1 name unchanged, R2 version strictly increased, R3 input widens, R4 output narrows, R5 effects ⊆ parent. Returns status=promote with a lineage receipt, or status=rollback with the named rule that fired. Parent and child accept SKILL.md text or descriptor objects.","inputSchema":{"type":"object","required":["parent","child"],"properties":{"parent":{"description":"Parent skill: SKILL.md string or descriptor object."},"child":{"description":"Proposed child skill: SKILL.md string or descriptor object."}}}},{"name":"skill_compose","description":"Composition gate. Verifies that every adjacent pair in a skill list composes at the type level — producer's output_shape is type-substitutable for consumer's input_shape. Length 2 checks a pair; length ≥ 3 checks a chain. Returns status=compatible, or status=incompatible with at_step at the first break. at_step is the **transition index**: at_step=i means the edge skills[i] → skills[i+1] failed (producer at array index i, consumer at i+1). For malformed_descriptor, at_step is the array index of the bad element itself. Each element accepts SKILL.md text or descriptor object.","inputSchema":{"type":"object","required":["skills"],"properties":{"skills":{"type":"array","description":"Skills in pipeline order; SKILL.md text or descriptor objects.","minItems":2}}}},{"name":"skill_merge","description":"Atomic skill composition. Fuses a producer and consumer into a single new SKILL.md whose contract is the categorical composition of the parents: input_shape from producer, output_shape from consumer, effects = union, references pin both parents by content_hash. Type-checks via the Liskov composition rule before fusing; on incompatibility returns status=incompatible. Deterministic: same parents → byte-identical fused SKILL.md. Note: a fused skill is NOT a refinement of either parent (effects union widens). Use skill_refine for parent→child; use skill_merge for sibling→composite. Returns the upgraded SKILL.md, the fused descriptor, and the proof.","inputSchema":{"type":"object","required":["producer","consumer"],"properties":{"producer":{"description":"Head skill: SKILL.md string or descriptor object."},"consumer":{"description":"Tail skill: SKILL.md string or descriptor object."},"name":{"type":"string","description":"Optional name for the fused skill. Auto-derived if omitted."},"skill_md":{"type":"string","description":"Optional existing SKILL.md prose to upgrade in place. Scaffolded if omitted."}}}}]"##;

fn handle_tools_list(id: &str) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{{"tools":{TOOLS_JSON}}}}}"#)
}

// -------------------- tools/call --------------------

fn handle_tools_call(id: &str, params: &str) -> String {
    let p = match crate::computations::parse_json(params) {
        Ok(p) => p,
        Err(e) => return error_response(id, -32602, &format!("bad params: {e}")),
    };
    let name = match p.get("name").map(|s| unquote(s)) {
        Some(Ok(n)) => n,
        _ => return error_response(id, -32602, "missing tool name"),
    };
    let args_raw = p.get("arguments").cloned().unwrap_or_else(|| "{}".to_string());
    let args = match crate::computations::parse_json(&args_raw) {
        Ok(a) => a,
        Err(e) => return error_response(id, -32602, &format!("bad arguments: {e}")),
    };
    match name.as_str() {
        "skill_bind"  => tool_certify(id, &args),
        "skill_verify"  => tool_verify(id, &args),
        "skill_refine"  => tool_evolve(id, &args),
        "skill_compose" => tool_compose(id, &args),
        "skill_merge"    => tool_fuse(id, &args),
        // Per the MCP spec, "Unknown tools" is a PROTOCOL error.
        other          => error_response(id, -32602, &format!("unknown tool: {other}")),
    }
}

// ============================================================
// The five canonical gates
// ============================================================

/// `skill_bind`. Embed a typed contract + self-verifying proof in a
/// SKILL.md. If `skill_md` is provided, the existing markdown is upgraded;
/// if absent, a fresh scaffold is generated from the descriptor alone.
fn tool_certify(id: &str, args: &std::collections::HashMap<String, String>) -> String {
    let descriptor_raw = match args.get("descriptor") {
        Some(v) => v.as_str(),
        None => return tool_err(id, "missing descriptor"),
    };
    // Decide whether to upgrade an existing SKILL.md or scaffold a fresh one.
    let md = if let Some(raw_md) = args.get("skill_md") {
        match unquote(raw_md) {
            Ok(s) => s,
            Err(e) => return tool_err(id, &format!("skill_md: {e}")),
        }
    } else {
        // Generate a scaffold so the bind path is uniform.
        match crate::bridge::scaffold_md_from_descriptor(descriptor_raw) {
            Ok(s) => s,
            Err(e) => return tool_err(id, &format!("scaffold: {e}")),
        }
    };
    match crate::bridge::bind_descriptor_to_md(&md, descriptor_raw) {
        Ok((upgraded, proof)) => tool_ok(
            id,
            &format!(
                r#"{{"status":"certified","skill_md":"{}","proof":{{"protocol":"{}","spec_uri":"{}","output_hash":"{}","runtime":"{}"}}}}"#,
                json_escape(&upgraded),
                proof.protocol,
                json_escape(&proof.spec_uri),
                proof.output_hash,
                json_escape(&proof.runtime),
            ),
        ),
        Err(e) => tool_err(id, &e),
    }
}

/// `skill_verify`. Re-derive the proof embedded inside a SKILL.md and
/// compare. On success the response includes the descriptor so callers
/// don't need a separate extract call.
fn tool_verify(id: &str, args: &std::collections::HashMap<String, String>) -> String {
    let raw_md = match args.get("skill_md") {
        Some(v) => v,
        None => return tool_err(id, "missing skill_md"),
    };
    let md = match unquote(raw_md) {
        Ok(s) => s,
        Err(e) => return tool_err(id, &format!("skill_md: {e}")),
    };
    // Pull the descriptor (if any) regardless of verify outcome.
    let descriptor_json = crate::bridge::descriptor_from_anywhere(&md).ok();

    use crate::bridge::VerifyOutcome;
    match crate::bridge::verify_embedded_proof(&md) {
        Ok(VerifyOutcome::Valid { body }) => {
            // Gate-as-loader: the body is returned ONLY on Valid. Caching
            // is internal; the wire shape does not advertise cache hits.
            let desc = descriptor_json.unwrap_or_else(|| "null".into());
            tool_ok(id, &format!(
                r#"{{"status":"valid","descriptor":{desc},"body":"{}"}}"#,
                json_escape(&body),
            ))
        }
        Ok(VerifyOutcome::Mismatch)               => tool_ok(id, r#"{"status":"mismatch"}"#),
        Ok(VerifyOutcome::NoProof)                => tool_ok(id, r#"{"status":"no_proof"}"#),
        Ok(VerifyOutcome::UnsupportedProtocol { proof, verifier }) => tool_ok(id,
            &format!(r#"{{"status":"unsupported_protocol","proof":"{}","verifier":"{}"}}"#,
                json_escape(&proof), json_escape(&verifier))),
        Ok(VerifyOutcome::SubstrateIncompatible { proof, verifier }) => tool_ok(id,
            &format!(r#"{{"status":"substrate_incompatible","proof":"{}","verifier":"{}"}}"#,
                json_escape(&proof), json_escape(&verifier))),
        Err(e) => tool_err(id, &e),
    }
}

/// `skill_refine`. Refinement gate. Accepts SKILL.md text or descriptor
/// objects for both parent and child (auto-detected).
fn tool_evolve(id: &str, args: &std::collections::HashMap<String, String>) -> String {
    let parent = match args.get("parent") {
        Some(v) => v.as_str(),
        None => return tool_err(id, "missing parent"),
    };
    let child = match args.get("child") {
        Some(v) => v.as_str(),
        None => return tool_err(id, "missing child"),
    };
    // The values may be JSON objects (used as-is) OR JSON string
    // literals carrying SKILL.md text (unquote first). Detect by leading char.
    let parent_text = match prepare_skill_input(parent) {
        Ok(s) => s,
        Err(e) => return tool_err(id, &format!("parent: {e}")),
    };
    let child_text = match prepare_skill_input(child) {
        Ok(s) => s,
        Err(e) => return tool_err(id, &format!("child: {e}")),
    };
    match crate::tripwire::check_refine(&parent_text, &child_text) {
        Ok(result) => tool_ok(id, &result.to_json()),
        Err(e)     => tool_err(id, &e),
    }
}

/// `skill_compose`. Composition gate. Single tool for the pair case
/// (skills.len() == 2) and the chain case (skills.len() >= 3).
fn tool_compose(id: &str, args: &std::collections::HashMap<String, String>) -> String {
    let skills_raw = match args.get("skills") {
        Some(v) => v,
        None => return tool_err(id, "missing skills array"),
    };
    let elements = match split_json_array(skills_raw) {
        Ok(v) => v,
        Err(e) => return tool_err(id, &format!("bad skills array: {e}")),
    };
    let prepared: Vec<String> = match elements
        .iter()
        .map(|s| prepare_skill_input(s))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(v) => v,
        Err(e) => return tool_err(id, &e),
    };
    let refs: Vec<&str> = prepared.iter().map(|s| s.as_str()).collect();
    match crate::tripwire::check_compose(&refs) {
        Ok(result) => tool_ok(id, &result.to_json()),
        Err(e)     => tool_err(id, &e),
    }
}

/// `skill_merge`. Atomic composition gate. Fuses two compose-compatible
/// skills into one new self-contained, certified SKILL.md.
///
/// Required args: `producer`, `consumer` (each a SKILL.md string or
/// descriptor object). Optional: `name` (string, defaults to
/// auto-derived); `skill_md` (existing prose to upgrade in place,
/// defaults to a fresh scaffold).
fn tool_fuse(id: &str, args: &std::collections::HashMap<String, String>) -> String {
    let producer = match args.get("producer") {
        Some(v) => v.as_str(),
        None => return tool_err(id, "missing producer"),
    };
    let consumer = match args.get("consumer") {
        Some(v) => v.as_str(),
        None => return tool_err(id, "missing consumer"),
    };
    // Coerce both via the same helper compose / refine use — accepts
    // descriptor object substring OR JSON-quoted SKILL.md string.
    let producer_text = match prepare_skill_input(producer) {
        Ok(s) => s,
        Err(e) => return tool_err(id, &format!("producer: {e}")),
    };
    let consumer_text = match prepare_skill_input(consumer) {
        Ok(s) => s,
        Err(e) => return tool_err(id, &format!("consumer: {e}")),
    };

    // Optional name override. The hand-rolled parser stores values as
    // their raw substring including quotes; unquote to recover the
    // string literal.
    let name_owned: Option<String> = match args.get("name") {
        Some(raw) => match unquote(raw) {
            Ok(s) => Some(s),
            Err(e) => return tool_err(id, &format!("name: {e}")),
        },
        None => None,
    };

    // Optional existing-md to upgrade.
    let md_owned: Option<String> = match args.get("skill_md") {
        Some(raw) => match unquote(raw) {
            Ok(s) => Some(s),
            Err(e) => return tool_err(id, &format!("skill_md: {e}")),
        },
        None => None,
    };

    let result = crate::fuse::fuse_skills(
        &producer_text,
        &consumer_text,
        name_owned.as_deref(),
        md_owned.as_deref(),
    );
    match result {
        Ok(r)  => tool_ok(id, &r.to_json()),
        Err(e) => tool_err(id, &e),
    }
}

/// Coerce an MCP argument into a Lyra-readable input. The tripwire
/// layer accepts either a SKILL.md text or a descriptor JSON object;
/// this helper turns the wire form (object substring OR quoted string)
/// into the right thing.
fn prepare_skill_input(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim_start();
    if trimmed.starts_with('{') {
        // It's a JSON object — pass through as descriptor JSON.
        Ok(raw.to_string())
    } else if trimmed.starts_with('"') {
        // It's a JSON string — unquote to recover the SKILL.md text.
        unquote(raw)
    } else {
        Err(format!("expected JSON object or quoted SKILL.md string, got {raw:?}"))
    }
}

/// Split a JSON array string into its element substrings, respecting
/// string boundaries and nested brackets. Mirrors the technique in
/// `computations::parse_list_of_maps` but returns raw element strings.
fn split_json_array(s: &str) -> Result<Vec<String>, String> {
    let s = s.trim();
    if !s.starts_with('[') || !s.ends_with(']') {
        return Err(format!("expected JSON array, got {s:?}"));
    }
    let inner = s[1..s.len() - 1].trim();
    if inner.is_empty() { return Ok(Vec::new()); }
    let bytes = inner.as_bytes();
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut start = 0usize;
    let mut i = 0usize;
    let mut out = Vec::new();
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            if c == b'\\' && i + 1 < bytes.len() { i += 2; continue; }
            if c == b'"' { in_string = false; }
        } else {
            match c {
                b'"' => in_string = true,
                b'{' | b'[' => depth += 1,
                b'}' | b']' => depth -= 1,
                b',' if depth == 0 => {
                    out.push(inner[start..i].trim().to_string());
                    start = i + 1;
                }
                _ => {}
            }
        }
        i += 1;
    }
    let last = inner[start..].trim();
    if !last.is_empty() { out.push(last.to_string()); }
    Ok(out)
}

// -------------------- response builders --------------------

fn tool_ok(id: &str, text: &str) -> String {
    let escaped = json_escape(text);
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"result":{{"content":[{{"type":"text","text":"{escaped}"}}],"isError":false}}}}"#,
    )
}

fn tool_err(id: &str, text: &str) -> String {
    let escaped = json_escape(text);
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"result":{{"content":[{{"type":"text","text":"{escaped}"}}],"isError":true}}}}"#,
    )
}

fn error_response(id: &str, code: i32, msg: &str) -> String {
    let escaped = json_escape(msg);
    let id_safe = sanitize_id(id);
    format!(
        r#"{{"jsonrpc":"2.0","id":{id_safe},"error":{{"code":{code},"message":"{escaped}"}}}}"#,
    )
}

/// JSON-RPC 2.0 §4.2: `id` MUST be a string, number, or null. Objects
/// and arrays are illegal. We splice `id` verbatim into the response,
/// so a hostile client sending `"id": {"$inject": true}` would corrupt
/// the response envelope. This predicate accepts only well-formed
/// scalar id tokens (raw JSON token form, as `parse_json` returns).
fn is_valid_id_token(raw: &str) -> bool {
    let t = raw.trim();
    if t == "null" { return true; }
    let bytes = t.as_bytes();
    if bytes.first() == Some(&b'"') && bytes.last() == Some(&b'"') && bytes.len() >= 2 {
        return crate::computations::unquote_json_string(t).is_ok();
    }
    // Number: optional minus, then digits, optionally with one fraction
    // and/or one exponent. Reject NaN/Infinity (JSON doesn't allow them
    // anyway). A lenient byte-class check is sufficient — the inner
    // parser has already accepted this as a valid JSON value, so
    // pathological shapes don't reach us; we only need to gate out
    // objects (`{`) and arrays (`[`).
    let first = match bytes.first() { Some(b) => *b, None => return false };
    matches!(first, b'-' | b'0'..=b'9')
}

/// Fallback for the formatter when `id` is missing, malformed, or
/// non-scalar: emit `null` so the response envelope stays well-formed.
/// JSON-RPC 2.0 §5 explicitly allows a null id on parse-error replies.
fn sanitize_id(raw: &str) -> String {
    if is_valid_id_token(raw) {
        raw.trim().to_string()
    } else {
        "null".to_string()
    }
}

// -------------------- helpers --------------------

/// Decode a JSON string literal token. Delegates to the canonical
/// implementation in `crate::computations`; one implementation, one
/// audit surface.
fn unquote(token: &str) -> Result<String, String> {
    crate::computations::unquote_json_string(token)
}

/// Escape a string for embedding inside a JSON string literal.
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

// -------------------- tests --------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_returns_protocol_version() {
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        let resp = handle_message(req).expect("response");
        assert!(resp.contains("\"protocolVersion\":\"2025-06-18\""));
        assert!(resp.contains("\"name\":\"lyra\""));
        assert!(resp.contains("\"capabilities\":{\"tools\":{}}"));
    }

    #[test]
    fn tools_list_advertises_all_four_gates() {
        let req = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;
        let resp = handle_message(req).expect("response");
        for name in ["skill_bind", "skill_verify", "skill_refine", "skill_compose", "skill_merge"] {
            assert!(resp.contains(&format!("\"name\":\"{name}\"")), "missing {name}");
        }
    }

    #[test]
    fn notification_returns_none() {
        // No id → JSON-RPC notification. Server must not respond.
        let req = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        assert!(handle_message(req).is_none());
    }

    #[test]
    fn unknown_method_returns_method_not_found() {
        let req = r#"{"jsonrpc":"2.0","id":9,"method":"who/dis"}"#;
        let resp = handle_message(req).expect("response");
        assert!(resp.contains("\"code\":-32601"));
    }

    #[test]
    fn unquote_handles_escapes_and_unicode() {
        assert_eq!(unquote(r#""hello""#).unwrap(), "hello");
        assert_eq!(unquote(r#""he said \"hi\"""#).unwrap(), r#"he said "hi""#);
        assert_eq!(unquote(r#""line\nbreak""#).unwrap(), "line\nbreak");
        assert_eq!(unquote(r#""path\\to""#).unwrap(), "path\\to");
        assert_eq!(unquote(r#""snowé""#).unwrap(), "snowé");
    }

    #[test]
    fn json_escape_roundtrips_through_unquote() {
        let s = "he said \"hi\"\n\tend";
        let escaped = json_escape(s);
        let quoted = format!("\"{escaped}\"");
        assert_eq!(unquote(&quoted).unwrap(), s);
    }

    /// AUDIT #6: `TOOLS_JSON` is hand-edited; any schema change is a
    /// string-edit with no compile-time check. This test parses the
    /// literal through the same JSON parser MCP clients will use, and
    /// asserts the manifest carries every tool's required structural
    /// fields. A typo in the literal that breaks JSON structure or
    /// drops a tool entry fails the build.
    #[test]
    fn tools_json_parses_and_lists_every_canonical_gate() {
        // The full tools/list response is wrapped in
        // {"jsonrpc":...,"result":{"tools":[...]}}. We test by issuing
        // a real tools/list request through handle_message and parsing
        // the response — same path MCP clients exercise.
        let resp = handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
            .expect("response");
        // Top-level JSON must parse.
        let top = crate::computations::parse_json(&resp)
            .expect("tools/list response must be valid JSON");
        assert!(top.contains_key("result"), "must carry a result field");
        // Every advertised tool name must appear; if any is dropped or
        // misspelled in the hand-rolled literal, this test catches it.
        for name in [
            "skill_bind",
            "skill_verify",
            "skill_refine",
            "skill_compose",
            "skill_merge",
        ] {
            let needle = format!("\"name\":\"{name}\"");
            assert!(
                resp.contains(&needle),
                "TOOLS_JSON must advertise {name}: {resp}",
            );
        }
        // Each tool must declare an inputSchema (Object Schema).
        let occurrences = resp.matches("\"inputSchema\"").count();
        assert!(
            occurrences >= 5,
            "every advertised tool must carry inputSchema; got {occurrences} occurrences",
        );
        // No legacy / removed tools may sneak back in.
        for legacy in [
            "lyra_score",
            "lyra_tripwire",
            "lyra_compose_check",
            "lyra_chain_check",
            "lyra_md_extract",
            "lyra_md_bind",
            "lyra_md_verify",
            // Pre-v0.2 names retired in the v0.2 rename:
            "lyra_certify",
            "lyra_verify",
            "lyra_refine",
            "lyra_compose",
            "lyra_fuse",
        ] {
            let needle = format!("\"name\":\"{legacy}\"");
            assert!(
                !resp.contains(&needle),
                "legacy tool {legacy} must not be re-advertised",
            );
        }
    }
}
