//! End-to-end tests for the `lyra` CLI: validate writes a receipt, verify
//! replays it. Each test exercises the binary as an external user would.

use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_lyra"))
}

fn tmp_receipt(name: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("lyra-test-{}-{}.json", name, std::process::id()));
    p
}

fn score(computation_id: &str, input: &str, receipt: &Path) {
    let out = Command::new(binary())
        .args(["score", computation_id, input])
        .arg(receipt)
        .output()
        .expect("spawn score");
    assert!(
        out.status.success(),
        "score failed: stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

fn verify(computation_id: &str, input: &str, receipt: &Path) -> (bool, String, String) {
    let out = Command::new(binary())
        .args(["verify-receipt", computation_id, input])
        .arg(receipt)
        .output()
        .expect("spawn verify-receipt");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn assert_roundtrip(case: &str, computation_id: &str, input: &str) {
    let receipt = tmp_receipt(case);
    score(computation_id, input, &receipt);
    let (ok, stdout, stderr) = verify(computation_id, input, &receipt);
    assert!(
        ok,
        "verify failed for {case}\ninput={input}\nstdout={stdout}\nstderr={stderr}",
    );
    assert!(
        stdout.contains("VERIFY_OK"),
        "expected VERIFY_OK in stdout for {case}, got: {stdout}",
    );
    let _ = std::fs::remove_file(&receipt);
}

// ---- protocol computations roundtrip ----

#[test]
fn roundtrip_skill_interface_hash() {
    let input = r#"{"name":"web-search","version":"1.0.0","input_shape":{"type":"string","max_bytes":256},"output_shape":{"type":"list","item":{"type":"string","max_bytes":4096},"max_items":100},"effects":["web_read"],"references":[],"content_hash":"10b5b44710b5b44710b5b44710b5b44710b5b44710b5b44710b5b44710b5b447"}"#;
    assert_roundtrip("skill_interface_hash", "skill_interface_hash", input);
}

// `compose_interfaces` routes through the typed descriptor builder (audit #2),
// which requires full skill descriptors. Liskov direction (audit #3):
// producer's max_bytes must be <= consumer's. The helper below wraps each
// shape into a complete descriptor skeleton.
fn compose_input(p_out: &str, c_in: &str) -> String {
    let a64 = "a".repeat(64);
    let b64 = "b".repeat(64);
    format!(
        r#"{{"producer":{{"content_hash":"{a64}","effects":["none"],"input_shape":{{"type":"u8","max_bytes":1}},"name":"p","output_shape":{p_out},"references":[],"version":"1.0.0"}},"consumer":{{"content_hash":"{b64}","effects":["none"],"input_shape":{c_in},"name":"c","output_shape":{{"type":"u8","max_bytes":1}},"references":[],"version":"1.0.0"}}}}"#
    )
}

#[test]
fn roundtrip_compose_interfaces_compatible() {
    // Producer outputs up to 256 bytes; consumer accepts up to 1024.
    let input = compose_input(
        r#"{"type":"string","max_bytes":256}"#,
        r#"{"type":"string","max_bytes":1024}"#,
    );
    assert_roundtrip("compose_interfaces_ok", "compose_interfaces", &input);
}

#[test]
fn roundtrip_compose_interfaces_incompatible() {
    // Producer outputs up to 1024 bytes; consumer accepts only 256 — overflow.
    let input = compose_input(
        r#"{"type":"string","max_bytes":1024}"#,
        r#"{"type":"string","max_bytes":256}"#,
    );
    assert_roundtrip("compose_interfaces_bad", "compose_interfaces", &input);
}

#[test]
fn roundtrip_merkle_manifest() {
    let input = r#"[{"path":"skills/web-search","content_hash":"10b5b44710b5b44710b5b44710b5b44710b5b44710b5b44710b5b44710b5b447"},{"path":"skills/http-client","content_hash":"aabbccdd"}]"#;
    assert_roundtrip("merkle_manifest", "merkle_manifest", input);
}

#[test]
fn roundtrip_skill_reference_resolve() {
    // S4: pinned references `<name>@<64-hex>` must match both name and hash.
    let http_hash = "aa".repeat(32);
    let input = format!(
        r#"{{"skill":{{"name":"web-search","version":"1.0.0","input_shape":{{"type":"string","max_bytes":256}},"output_shape":{{"type":"string","max_bytes":4096}},"effects":["web_read"],"references":["http-client@{http_hash}"],"content_hash":"10b5b44710b5b44710b5b44710b5b44710b5b44710b5b44710b5b44710b5b447"}},"manifest":[{{"name":"http-client","content_hash":"{http_hash}"}}]}}"#
    );
    assert_roundtrip("skill_reference_resolve", "skill_reference_resolve", &input);
}

// ---- tamper detection ----

#[test]
fn tamper_output_hash_is_rejected() {
    let input = r#"{"name":"x","version":"1.0.0","input_shape":{"type":"u8","max_bytes":1},"output_shape":{"type":"u8","max_bytes":1},"effects":["none"],"references":[],"content_hash":"10b5b44710b5b44710b5b44710b5b44710b5b44710b5b44710b5b44710b5b447"}"#;
    let receipt = tmp_receipt("tamper");
    score("skill_interface_hash", input, &receipt);

    let raw = std::fs::read_to_string(&receipt).expect("read receipt");
    let key = "\"output_hash\":\"";
    let start = raw.find(key).expect("output_hash field present") + key.len();
    let mut bytes: Vec<u8> = raw.into_bytes();
    bytes[start] ^= 0x01;
    std::fs::write(&receipt, &bytes).expect("write tampered receipt");

    let (ok, stdout, _stderr) = verify("skill_interface_hash", input, &receipt);
    assert!(!ok, "tampered receipt must NOT verify\nstdout={stdout}");
    let _ = std::fs::remove_file(&receipt);
}

#[test]
fn tamper_input_mismatch_is_rejected() {
    let input1 = r#"{"name":"a","version":"1.0.0","input_shape":{"type":"u8","max_bytes":1},"output_shape":{"type":"u8","max_bytes":1},"effects":["none"],"references":[],"content_hash":"10b5b44710b5b44710b5b44710b5b44710b5b44710b5b44710b5b44710b5b447"}"#;
    let input2 = r#"{"name":"b","version":"1.0.0","input_shape":{"type":"u8","max_bytes":1},"output_shape":{"type":"u8","max_bytes":1},"effects":["none"],"references":[],"content_hash":"10b5b44710b5b44710b5b44710b5b44710b5b44710b5b44710b5b44710b5b447"}"#;
    let receipt = tmp_receipt("tamper-input");
    score("skill_interface_hash", input1, &receipt);

    let (ok, _stdout, _stderr) = verify("skill_interface_hash", input2, &receipt);
    assert!(!ok, "verify with wrong input must fail");
    let _ = std::fs::remove_file(&receipt);
}
