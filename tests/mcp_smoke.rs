//! End-to-end MCP smoke test for the **five canonical gates**.
//!
//! Spawns the `lyra mcp serve` binary, drives a real client conversation
//! over stdio, asserts each response against the MCP 2025-06-18 spec.
//!
//! The five gates:
//!   * `skill_bind`   — embed a typed contract + proof in any SKILL.md
//!   * `skill_verify`   — re-derive the embedded proof
//!   * `skill_refine`   — refinement gate (R1–R5)
//!   * `skill_compose`  — composition gate (pair or chain)

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn lyra_bin() -> String { env!("CARGO_BIN_EXE_lyra").to_string() }

fn run_session(requests: &[&str]) -> Vec<String> {
    let mut child = Command::new(lyra_bin())
        .args(["mcp", "serve"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().expect("spawn lyra mcp serve");
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut responses = Vec::with_capacity(requests.len());
    for req in requests {
        writeln!(stdin, "{req}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        let n = reader.read_line(&mut line).unwrap();
        assert!(n > 0, "server closed stdout before responding to: {req}");
        responses.push(line.trim_end().to_string());
    }
    drop(stdin);
    let _ = child.wait();
    responses
}

// ============================================================
// initialize / discovery
// ============================================================

#[test]
fn initialize_returns_protocol_version() {
    let resp = run_session(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{}}}"#,
    ]);
    assert!(resp[0].contains("\"protocolVersion\":\"2025-06-18\""), "{}", resp[0]);
    assert!(resp[0].contains("\"name\":\"lyra\""), "{}", resp[0]);
    assert!(resp[0].contains("\"capabilities\":{\"tools\":{}}"), "{}", resp[0]);
}

#[test]
fn tools_list_advertises_exactly_the_five_gates() {
    let resp = run_session(&[r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#]);
    let r = &resp[0];
    for name in ["skill_bind", "skill_verify", "skill_refine", "skill_compose", "skill_merge"] {
        assert!(r.contains(&format!("\"name\":\"{name}\"")), "missing {name}: {r}");
    }
    // Legacy tools must NOT be present.
    for legacy in ["lyra_score", "lyra_tripwire", "lyra_compose_check",
                   "lyra_chain_check", "lyra_md_extract", "lyra_md_bind", "lyra_md_verify",
                   // Pre-v0.2 names retired in the v0.2 rename:
                   "lyra_certify", "lyra_verify", "lyra_refine", "lyra_compose", "lyra_fuse"] {
        assert!(!r.contains(&format!("\"name\":\"{legacy}\"")),
            "legacy tool {legacy} still advertised: {r}");
    }
}

#[test]
fn protocol_version_negotiation_echoes_supported_version() {
    let resp = run_session(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{}}}"#,
    ]);
    assert!(resp[0].contains("\"protocolVersion\":\"2024-11-05\""), "{}", resp[0]);
}

#[test]
fn unknown_tool_returns_protocol_error_per_spec() {
    let resp = run_session(&[
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"lyra_nope","arguments":{}}}"#
    ]);
    assert!(resp[0].contains("\"code\":-32602"), "{}", resp[0]);
}

#[test]
fn notifications_get_no_response() {
    let mut child = Command::new(lyra_bin())
        .args(["mcp", "serve"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    writeln!(stdin, r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#).unwrap();
    writeln!(stdin, r#"{{"jsonrpc":"2.0","id":42,"method":"tools/list"}}"#).unwrap();
    stdin.flush().unwrap();
    drop(stdin);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert!(line.contains("\"id\":42"), "{line}");
    let mut next = String::new();
    let _ = reader.read_line(&mut next);
    assert!(next.trim().is_empty(), "unexpected extra response: {next:?}");
    let _ = child.wait();
}

// ============================================================
// skill_bind
// ============================================================

#[test]
fn certify_scaffolds_a_fresh_skill_md_from_descriptor_alone() {
    let descriptor = r#"{"content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","effects":["none"],"input_shape":{"type":"u8","max_bytes":1},"name":"fresh","output_shape":{"type":"u8","max_bytes":1},"references":[],"version":"1.0.0"}"#;
    let req = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"skill_bind","arguments":{{"descriptor":{descriptor}}}}}}}"#
    );
    let resp = run_session(&[&req]);
    let r = &resp[0];
    assert!(r.contains("\"isError\":false"), "{r}");
    assert!(r.contains("\\\"status\\\":\\\"certified\\\""), "{r}");
    assert!(r.contains("\\\"skill_md\\\""), "must return upgraded SKILL.md: {r}");
    assert!(r.contains("\\\"proof\\\""), "must include proof: {r}");
    // The proof must carry the protocol identifier — without it the
    // artifact is not self-describing for a cold-start verifier.
    assert!(r.contains("\\\"protocol\\\":\\\"hermes-lyra/0.2\\\""),
        "proof must carry protocol field: {r}");
}

#[test]
fn certify_upgrades_existing_skill_md() {
    let md = "---\\nname: existing\\nversion: 1.0.0\\n---\\n\\n# existing\\nProse.\\n";
    let descriptor = r#"{"content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","effects":["none"],"input_shape":{"type":"u8","max_bytes":1},"name":"existing","output_shape":{"type":"u8","max_bytes":1},"references":[],"version":"1.0.0"}"#;
    let req = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"skill_bind","arguments":{{"skill_md":"{md}","descriptor":{descriptor}}}}}}}"#
    );
    let resp = run_session(&[&req]);
    let r = &resp[0];
    assert!(r.contains("\\\"status\\\":\\\"certified\\\""), "{r}");
}

// ============================================================
// skill_verify
// ============================================================

#[test]
fn verify_returns_valid_for_freshly_certified_skill() {
    // Certify a descriptor, then verify the resulting SKILL.md.
    let skill_md_path = "examples/inbox-triage/SKILL.md";
    let md_raw = std::fs::read_to_string(skill_md_path).unwrap();
    let md_escaped = md_raw.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
    let req = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"skill_verify","arguments":{{"skill_md":"{md_escaped}"}}}}}}"#
    );
    let resp = run_session(&[&req]);
    let r = &resp[0];
    assert!(r.contains("\\\"status\\\":\\\"valid\\\""), "shipped skill must verify: {r}");
    assert!(r.contains("\\\"descriptor\\\""), "valid response must include descriptor: {r}");
}

#[test]
fn verify_returns_no_proof_for_bare_markdown() {
    let md = "---\\nname: x\\n---\\n\\nProse only, no lyra block.\\n";
    let req = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"skill_verify","arguments":{{"skill_md":"{md}"}}}}}}"#
    );
    let resp = run_session(&[&req]);
    let r = &resp[0];
    assert!(r.contains("\\\"status\\\":\\\"no_proof\\\""), "{r}");
}

// ============================================================
// skill_refine
// ============================================================

#[test]
fn refine_promotes_legitimate_refinement() {
    let parent = std::fs::read_to_string("examples/code-review-evolve/v0.1.0.lyra.json")
        .unwrap().replace('\n', "");
    let child = std::fs::read_to_string("examples/code-review-evolve/v0.1.1.lyra.json")
        .unwrap().replace('\n', "");
    let req = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"skill_refine","arguments":{{"parent":{parent},"child":{child}}}}}}}"#
    );
    let resp = run_session(&[&req]);
    let r = &resp[0];
    assert!(r.contains("\\\"status\\\":\\\"promote\\\""), "must promote: {r}");
    assert!(r.contains("\\\"lineage_receipt\\\""), "must include lineage receipt: {r}");
}

/// **Audit fix**: the previously-uncovered path through MCP. A child
/// that adds a field to a structured output — the natural way an
/// agent evolves a skill — can push the structured product past the
/// 16 MiB cap. The refine gate must route this as a typed rollback
/// with rule_fired=CapacityExceeded, NOT as malformed_descriptor.
#[test]
fn refine_capacity_exceeded_surfaces_as_typed_rollback() {
    let parent = r#"{"content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","effects":["llm"],"input_shape":{"type":"u8","max_bytes":1},"name":"cap-mcp","output_shape":{"type":"structured","fields":[{"name":"findings","shape":{"type":"list","max_items":16,"item":{"type":"structured","fields":[{"name":"severity","shape":{"type":"u8","max_bytes":1}},{"name":"file","shape":{"type":"string","max_bytes":128}},{"name":"message","shape":{"type":"string","max_bytes":512}}]}}}]},"references":[],"version":"0.1.0"}"#;
    let child = r#"{"content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","effects":["llm"],"input_shape":{"type":"u8","max_bytes":1},"name":"cap-mcp","output_shape":{"type":"structured","fields":[{"name":"findings","shape":{"type":"list","max_items":16,"item":{"type":"structured","fields":[{"name":"severity","shape":{"type":"u8","max_bytes":1}},{"name":"file","shape":{"type":"string","max_bytes":128}},{"name":"message","shape":{"type":"string","max_bytes":512}},{"name":"category","shape":{"type":"string","max_bytes":32}}]}}}]},"references":[],"version":"0.1.1"}"#;
    let req = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"skill_refine","arguments":{{"parent":{parent},"child":{child}}}}}}}"#
    );
    let resp = run_session(&[&req]);
    let r = &resp[0];
    assert!(r.contains("\"isError\":false"),
        "MCP must NOT route this as a protocol error: {r}");
    assert!(
        r.contains("\\\"status\\\":\\\"rollback\\\""),
        "capacity overflow during refine must be rollback, not malformed: {r}");
    assert!(
        r.contains("\\\"rule_fired\\\":\\\"CapacityExceeded\\\""),
        "rule_fired must be CapacityExceeded so agents can route distinctly: {r}");
}

#[test]
fn refine_rolls_back_dropped_field_with_named_rule() {
    let parent = std::fs::read_to_string("examples/code-review-evolve/v0.1.0.lyra.json")
        .unwrap().replace('\n', "");
    let child = std::fs::read_to_string("examples/code-review-evolve/v0.2.0-bad.lyra.json")
        .unwrap().replace('\n', "");
    let req = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"skill_refine","arguments":{{"parent":{parent},"child":{child}}}}}}}"#
    );
    let resp = run_session(&[&req]);
    let r = &resp[0];
    assert!(r.contains("\\\"status\\\":\\\"rollback\\\""), "must roll back: {r}");
    assert!(r.contains("\\\"rule_fired\\\":\\\"R4_OutputWidened\\\""), "must name rule: {r}");
}

// ============================================================
// skill_compose
// ============================================================

#[test]
fn compose_reports_compatible_pair() {
    let s = r#"{"content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","effects":["none"],"input_shape":{"type":"u8","max_bytes":1},"name":"s","output_shape":{"type":"u8","max_bytes":1},"references":[],"version":"1.0.0"}"#;
    let skills = format!("[{s},{s}]");
    let req = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"skill_compose","arguments":{{"skills":{skills}}}}}}}"#
    );
    let resp = run_session(&[&req]);
    let r = &resp[0];
    assert!(r.contains("\\\"status\\\":\\\"compatible\\\""), "{r}");
}

#[test]
fn compose_reports_incompatible_pair_with_at_step_and_reason() {
    // Two valid descriptors whose shapes do NOT compose: producer
    // outputs string<1024>, consumer accepts string<256>. Liskov
    // direction: producer too wide for consumer → incompatible at
    // step 0. **Every** failure path must be a typed status, never
    // an isError:true with a bare string.
    let producer = r#"{"content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","effects":["none"],"input_shape":{"type":"u8","max_bytes":1},"name":"p","output_shape":{"type":"string","max_bytes":1024},"references":[],"version":"1.0.0"}"#;
    let consumer = r#"{"content_hash":"a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2","effects":["none"],"input_shape":{"type":"string","max_bytes":256},"name":"c","output_shape":{"type":"u8","max_bytes":1},"references":[],"version":"1.0.0"}"#;
    let skills = format!("[{producer},{consumer}]");
    let req = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"skill_compose","arguments":{{"skills":{skills}}}}}}}"#
    );
    let resp = run_session(&[&req]);
    let r = &resp[0];
    assert!(r.contains("\"isError\":false"),
        "incompatible compose MUST return isError:false (typed status, not bare error): {r}");
    assert!(r.contains("\\\"status\\\":\\\"incompatible\\\""),
        "must surface incompatible status: {r}");
    assert!(r.contains("\\\"at_step\\\":0"),
        "must report at_step pointing at the failing producer: {r}");
    assert!(r.contains("\\\"reason\\\""),
        "must carry a reason string: {r}");
}

#[test]
fn compose_reports_malformed_descriptor_distinct_from_incompatible() {
    // First skill is valid; second is malformed (u32 max_bytes=999 >
    // LyraU32::SITE_COUNT=4). The gate must distinguish "wrong
    // descriptor" from "wrong composition" by emitting the
    // malformed_descriptor status with at_step pointing at the
    // offending input.
    let good = r#"{"content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","effects":["none"],"input_shape":{"type":"u8","max_bytes":1},"name":"p","output_shape":{"type":"u8","max_bytes":1},"references":[],"version":"1.0.0"}"#;
    let malformed = r#"{"content_hash":"a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2","effects":["none"],"input_shape":{"type":"u32","max_bytes":999},"name":"c","output_shape":{"type":"u8","max_bytes":1},"references":[],"version":"1.0.0"}"#;
    let skills = format!("[{good},{malformed}]");
    let req = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"skill_compose","arguments":{{"skills":{skills}}}}}}}"#
    );
    let resp = run_session(&[&req]);
    let r = &resp[0];
    assert!(r.contains("\"isError\":false"),
        "malformed descriptor MUST return isError:false (typed status): {r}");
    assert!(r.contains("\\\"status\\\":\\\"malformed_descriptor\\\""),
        "must distinguish malformed from incompatible: {r}");
    assert!(r.contains("\\\"at_step\\\":1"),
        "at_step must point at the malformed input: {r}");
}

#[test]
fn compose_reports_intact_chain() {
    let s = r#"{"content_hash":"a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1","effects":["none"],"input_shape":{"type":"u8","max_bytes":1},"name":"s","output_shape":{"type":"u8","max_bytes":1},"references":[],"version":"1.0.0"}"#;
    let skills = format!("[{s},{s},{s},{s}]");
    let req = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"skill_compose","arguments":{{"skills":{skills}}}}}}}"#
    );
    let resp = run_session(&[&req]);
    let r = &resp[0];
    assert!(r.contains("\\\"status\\\":\\\"compatible\\\""), "uniform chain compatible: {r}");
}

// ============================================================
// hardening regressions
// ============================================================

#[test]
fn structured_id_is_rejected_not_spliced() {
    // Audit H-1: an object/array `id` could be spliced verbatim into
    // the response envelope. Reject it before formatting; emit a clean
    // -32600 with `id:null`.
    let resp = run_session(&[
        r#"{"jsonrpc":"2.0","id":{"$inject":true},"method":"ping"}"#,
    ]);
    let r = &resp[0];
    assert!(r.contains("\"id\":null"), "structured id must be sanitized: {r}");
    assert!(r.contains("\"code\":-32600"), "must return Invalid Request: {r}");
    assert!(!r.contains("$inject"), "raw id token must not leak: {r}");
}

#[test]
fn wrong_jsonrpc_version_is_rejected() {
    // Audit M-9: only `"jsonrpc":"2.0"` is dispatched.
    let resp = run_session(&[
        r#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#,
    ]);
    let r = &resp[0];
    assert!(r.contains("\"code\":-32600"), "wrong jsonrpc must be -32600: {r}");
    assert!(r.contains("jsonrpc"), "error must reference the field: {r}");
}

#[test]
fn missing_jsonrpc_field_is_rejected() {
    let resp = run_session(&[
        r#"{"id":1,"method":"ping"}"#,
    ]);
    let r = &resp[0];
    assert!(r.contains("\"code\":-32600"), "missing jsonrpc must be -32600: {r}");
}
