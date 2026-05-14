//! `lyra` — reference CLI for the hermes-lyra skill-contract protocol.
//!
//! Five canonical checks (bind, verify, refine, compose, merge) plus
//! tooling (MCP server, demos, self-check, codec primitives).

use std::env;
use std::process;

use lyra_ref::cli_api::{base64_decode, base64_encode, score, verify, VerifyOutcome};
use lyra_ref::receipt::Receipt;

const USAGE: &str = r#"Usage:

The five skill-contract checks (each accepts SKILL.md or descriptor JSON):
  lyra bind     <SKILL.md> <descriptor.json>   embed contract + proof
  lyra bind     <descriptor.json>              scaffold a fresh SKILL.md
  lyra verify   <SKILL.md>                     re-derive embedded proof
  lyra refine   <parent>  <child>              refinement check (R1–R5)
  lyra compose  <s1> <s2> [<s3> ...]           composition check (pair or chain)
  lyra merge    <producer> <consumer>          atomic merge of two skills

Tooling:
  lyra install                                 register hermes-lyra in ~/.hermes/config.yaml (mcp_servers.lyra)
  lyra install --uninstall                     remove the registration
  lyra cid      <SKILL.md or descriptor.json>  print content CID (multibase CIDv1)
  lyra publish  <SKILL.md>                     emit [cid, bytes] for IPFS pinning
                                               (stderr=header, stdout=canonical JSON)
  lyra mcp serve                               MCP server over stdio
  lyra demo refine                             built-in self-modification demo
  lyra self-check                              decentralized acceptance suite
  lyra b64-encode <text>                       text codec (replaces GNU base64)
  lyra b64-decode <base64>

Low-level primitives (CLI / library only — not exposed via MCP):
  lyra score    <computation> <input-json> <receipt-out>
  lyra verify-receipt <computation> <input-json> <receipt-in>

Exit codes for the five checks:
  0  safe / verified path  (bind success / verify valid / pass / compatible)
  1  unsafe / rejected     (verify mismatch / fail / incompatible)
  2  i/o or argument error
"#;

fn main() {
    let args: Vec<String> = env::args().collect();

    // Zero- or one-arg subcommands. These have no IO contract beyond
    // their own output, so they get checked before the score/verify
    // arity guard.
    if args.len() >= 3 && args[1] == "mcp" && args[2] == "serve" {
        if let Err(e) = lyra_ref::mcp::serve_stdio() {
            eprintln!("mcp serve: {e}");
            process::exit(1);
        }
        return;
    }
    // lyra cid <SKILL.md or descriptor.json>
    //   Print the CIDv1 over the file's envelope bytes.
    //
    //   For a bound SKILL.md (one with a `proof:` line in its
    //   frontmatter), this strips the proof line and hashes the rest
    //   with BLAKE3-256, then wraps as CIDv1+raw+blake3. The result
    //   matches the `output_cid` embedded in the proof line — i.e.,
    //   the CID is self-verifying: re-derivable by anyone with the bytes.
    //
    //   For an unbound file (no proof line), the CID is over the raw
    //   bytes verbatim. The same byte sequence → the same CID, every time.
    //
    //   Either way: matches `ipfs add --raw-leaves --cid-version=1
    //   --hash=blake3 <file>` byte-for-byte.
    if args.len() >= 3 && args[1] == "cid" {
        let body = match std::fs::read_to_string(&args[2]) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("read {}: {e}", args[2]);
                process::exit(2);
            }
        };
        // Strip the proof line if present; otherwise hash the whole file.
        let envelope = lyra_ref::bridge::strip_proof_line(&body).unwrap_or(body);
        let cid = lyra_ref::cid::Cid::from_raw_blob(envelope.as_bytes()).to_string();
        println!("{cid}");
        return;
    }

    // lyra publish <SKILL.md>
    //   Print, on stderr: cid + byte length (the receipt-like header).
    //   Print, on stdout: the exact bytes whose CID is on stderr — the
    //   envelope bytes (file minus proof line) for a bound file, or the
    //   whole file for an unbound one.
    //
    //   The user pipes stdout into their pinning tool of choice:
    //     lyra publish s.md > /tmp/blob && ipfs add --raw-leaves --cid-version=1 --hash=blake3 /tmp/blob
    //   We deliberately do NOT call any network. Decentralization means
    //   the user picks the transport.
    if args.len() >= 3 && args[1] == "publish" {
        let body = match std::fs::read_to_string(&args[2]) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("read {}: {e}", args[2]);
                process::exit(2);
            }
        };
        let envelope = lyra_ref::bridge::strip_proof_line(&body).unwrap_or(body);
        let cid = lyra_ref::cid::Cid::from_raw_blob(envelope.as_bytes()).to_string();
        // Header on stderr (stdout = bytes-to-pin).
        eprintln!("cid: {cid}");
        eprintln!("bytes: {}", envelope.len());
        eprintln!("# pipe stdout into: ipfs add --raw-leaves --cid-version=1 --hash=blake3 -");
        print!("{envelope}");
        return;
    }

    if args.len() >= 2 && args[1] == "install" {
        let uninstall = args[2..].iter().any(|a| a == "--uninstall" || a == "--remove");
        match lyra_ref::install::run(uninstall) {
            Ok(outcome) => {
                use lyra_ref::install::InstallOutcome::*;
                match outcome {
                    Inserted { config_path, command } => {
                        println!(
                            "installed: registered {} in {} (mcp_servers.lyra). On your next message Hermes will load the five skill_* tools.",
                            command.display(), config_path.display()
                        );
                    }
                    Updated { config_path, command } => {
                        println!(
                            "updated: pointed mcp_servers.lyra at {} in {}. Hermes will auto-reload on next config save tick.",
                            command.display(), config_path.display()
                        );
                    }
                    Unchanged { config_path, command } => {
                        println!(
                            "unchanged: mcp_servers.lyra in {} already points at {}.",
                            config_path.display(), command.display()
                        );
                    }
                    Removed { config_path } => {
                        println!("uninstalled: removed mcp_servers.lyra from {}.", config_path.display());
                    }
                    NotInstalled { config_path } => {
                        println!("uninstalled: nothing to do — mcp_servers.lyra was not present in {}.", config_path.display());
                    }
                }
                return;
            }
            Err(e) => {
                eprintln!("install: {e}");
                process::exit(2);
            }
        }
    }
    if args.len() >= 3 && args[1] == "demo" && (args[2] == "refine" || args[2] == "tripwire") {
        match lyra_ref::demo::run_tripwire() {
            Ok(()) => return,
            Err(e) => { eprintln!("demo failed: {e}"); process::exit(1); }
        }
    }
    if args.len() >= 2 && args[1] == "self-check" {
        match lyra_ref::demo::run_self_check() {
            Ok(()) => return,
            Err(e) => { eprintln!("self-check FAILED: {e}"); process::exit(1); }
        }
    }
    if args.len() >= 3 && args[1] == "b64-encode" {
        println!("{}", base64_encode(args[2].as_bytes()));
        return;
    }
    if args.len() >= 3 && args[1] == "b64-decode" {
        match base64_decode(&args[2]) {
            Ok(b) => {
                // Try to print as utf-8; otherwise show byte length.
                match std::str::from_utf8(&b) {
                    Ok(s) => println!("{s}"),
                    Err(_) => println!("({} bytes; non-utf8)", b.len()),
                }
                return;
            }
            Err(e) => { eprintln!("b64-decode: {e}"); process::exit(1); }
        }
    }

    // ====================================================================
    // The five canonical gates. Each reads SKILL.md (or descriptor JSON)
    // from files and prints a structured result to stdout. Exit codes:
    //   0  safe / verified
    //   1  unsafe / rejected (rule fired, mismatch, etc.)
    //   2  i/o or argument error
    // ====================================================================

    // lyra bind <SKILL.md> <descriptor.json>  → upgraded SKILL.md to stdout
    // lyra bind <descriptor.json>             → scaffold + bind; SKILL.md to stdout
    // `certify` is the v0.1 alias and stays accepted; the docs lead with `bind`.
    if args.len() >= 3 && (args[1] == "bind" || args[1] == "certify") {
        if args.len() >= 4 {
            let md = match std::fs::read_to_string(&args[2]) {
                Ok(s) => s, Err(e) => { eprintln!("read {}: {e}", args[2]); process::exit(2); }
            };
            let descriptor = match std::fs::read_to_string(&args[3]) {
                Ok(s) => s, Err(e) => { eprintln!("read {}: {e}", args[3]); process::exit(2); }
            };
            match lyra_ref::bridge::bind_descriptor_to_md(&md, &descriptor) {
                Ok((upgraded, _)) => { print!("{upgraded}"); return; }
                Err(e) => { eprintln!("certify: {e}"); process::exit(1); }
            }
        } else {
            let descriptor = match std::fs::read_to_string(&args[2]) {
                Ok(s) => s, Err(e) => { eprintln!("read {}: {e}", args[2]); process::exit(2); }
            };
            let scaffold = match lyra_ref::bridge::scaffold_md_from_descriptor(&descriptor) {
                Ok(s) => s, Err(e) => { eprintln!("certify: {e}"); process::exit(1); }
            };
            match lyra_ref::bridge::bind_descriptor_to_md(&scaffold, &descriptor) {
                Ok((upgraded, _)) => { print!("{upgraded}"); return; }
                Err(e) => { eprintln!("certify: {e}"); process::exit(1); }
            }
        }
    }

    // lyra verify <SKILL.md>
    //   exit 0  valid
    //   exit 1  mismatch (forged descriptor)
    //   exit 2  no_proof / unsupported_protocol / substrate_incompatible / I-O
    if args.len() >= 3 && args[1] == "verify" {
        use lyra_ref::bridge::VerifyOutcome;
        let md = match std::fs::read_to_string(&args[2]) {
            Ok(s) => s, Err(e) => { eprintln!("read {}: {e}", args[2]); process::exit(2); }
        };
        match lyra_ref::bridge::verify_embedded_proof(&md) {
            Ok(outcome) => {
                let status = outcome.status();
                match &outcome {
                    // Valid: print one-line JSON status to stdout. The
                    // verified prose body (already in the SKILL.md the
                    // caller passed in) is exposed via `--body` flag
                    // for tooling that wants the gate-as-loader form;
                    // by default we keep stdout machine-parseable.
                    VerifyOutcome::Valid { .. } =>
                        println!("{{\"status\":\"{status}\"}}"),
                    VerifyOutcome::Mismatch | VerifyOutcome::NoProof =>
                        println!("{{\"status\":\"{status}\"}}"),
                    VerifyOutcome::UnsupportedProtocol { proof, verifier } =>
                        println!("{{\"status\":\"{status}\",\"proof\":\"{proof}\",\"verifier\":\"{verifier}\"}}"),
                    VerifyOutcome::SubstrateIncompatible { proof, verifier } =>
                        println!("{{\"status\":\"{status}\",\"proof\":\"{proof}\",\"verifier\":\"{verifier}\"}}"),
                }
                process::exit(match outcome {
                    VerifyOutcome::Valid { .. } => 0,
                    VerifyOutcome::Mismatch     => 1,
                    _                           => 2,
                });
            }
            Err(e) => { eprintln!("verify: {e}"); process::exit(2); }
        }
    }

    // lyra refine <parent> <child>  → refinement gate; exit 0 promote, 1 rollback
    if args.len() >= 4 && args[1] == "refine" {
        let parent = match std::fs::read_to_string(&args[2]) {
            Ok(s) => s, Err(e) => { eprintln!("read parent: {e}"); process::exit(2); }
        };
        let child = match std::fs::read_to_string(&args[3]) {
            Ok(s) => s, Err(e) => { eprintln!("read child: {e}"); process::exit(2); }
        };
        match lyra_ref::tripwire::check_refine(&parent, &child) {
            Ok(result) => {
                println!("{}", result.to_json());
                let exit = match result {
                    lyra_ref::tripwire::RefineResult::Promote { .. } => 0,
                    lyra_ref::tripwire::RefineResult::Rollback { .. } => 1,
                    lyra_ref::tripwire::RefineResult::MalformedDescriptor { .. } => 2,
                };
                process::exit(exit);
            }
            Err(e) => { eprintln!("refine: {e}"); process::exit(2); }
        }
    }

    // lyra compose <skill1> <skill2> [...] → composition gate; exit 0 compat, 1 incompat
        if args.len() >= 2 && args[1] == "compose" && args.len() < 4 {
        eprintln!("compose: need at least 2 skills (got {})", args.len().saturating_sub(2));
        process::exit(2);
    }
    if args.len() >= 4 && args[1] == "compose" {
        let mut bodies = Vec::with_capacity(args.len() - 2);
        for path in &args[2..] {
            match std::fs::read_to_string(path) {
                Ok(s) => bodies.push(s),
                Err(e) => { eprintln!("read {path}: {e}"); process::exit(2); }
            }
        }
        let refs: Vec<&str> = bodies.iter().map(|s| s.as_str()).collect();
        match lyra_ref::tripwire::check_compose(&refs) {
            Ok(result) => {
                println!("{}", result.to_json());
                let exit = match result {
                    lyra_ref::tripwire::ComposeResult::Compatible { .. } => 0,
                    lyra_ref::tripwire::ComposeResult::Incompatible { .. } => 1,
                    lyra_ref::tripwire::ComposeResult::MalformedDescriptor { .. } => 2,
                };
                process::exit(exit);
            }
            Err(e) => { eprintln!("compose: {e}"); process::exit(2); }
        }
    }

    // lyra merge <producer> <consumer>   → atomic merge; one new SKILL.md to stdout
    // (Legacy alias: `fuse`. v0.1 used `fuse`; v0.2 prefers `merge`.)
    if args.len() >= 4 && (args[1] == "merge" || args[1] == "fuse") {
        let producer = match std::fs::read_to_string(&args[2]) {
            Ok(s) => s,
            Err(e) => { eprintln!("read producer: {e}"); process::exit(2); }
        };
        let consumer = match std::fs::read_to_string(&args[3]) {
            Ok(s) => s,
            Err(e) => { eprintln!("read consumer: {e}"); process::exit(2); }
        };
        let name = if args.len() >= 5 { Some(args[4].as_str()) } else { None };
        match lyra_ref::fuse::fuse_skills(&producer, &consumer, name, None) {
            Ok(result) => {
                // result has a skill_md field; emit just that to stdout so the
                // shell user can pipe `lyra merge a.md b.md > merged.md`.
                // Pull the skill_md substring out of the JSON envelope.
                let json = result.to_json();
                let key = "\"skill_md\":\"";
                if let Some(i) = json.find(key) {
                    let after = &json[i + key.len()..];
                    // Find the closing unescaped quote.
                    let bytes = after.as_bytes();
                    let mut j = 0usize;
                    while j < bytes.len() {
                        if bytes[j] == b'\\' && j + 1 < bytes.len() { j += 2; continue; }
                        if bytes[j] == b'"' { break; }
                        j += 1;
                    }
                    let escaped = &after[..j];
                    // Decode the JSON-escaped string back to text.
                    let mut out = String::with_capacity(escaped.len());
                    let bytes = escaped.as_bytes();
                    let mut k = 0usize;
                    while k < bytes.len() {
                        if bytes[k] == b'\\' && k + 1 < bytes.len() {
                            match bytes[k+1] {
                                b'n'  => out.push('\n'),
                                b'r'  => out.push('\r'),
                                b't'  => out.push('\t'),
                                b'"'  => out.push('"'),
                                b'\\' => out.push('\\'),
                                _     => { out.push(bytes[k] as char); out.push(bytes[k+1] as char); }
                            }
                            k += 2;
                        } else {
                            out.push(bytes[k] as char);
                            k += 1;
                        }
                    }
                    print!("{out}");
                    process::exit(0);
                }
                // Fallback: emit the raw envelope.
                println!("{json}");
                process::exit(0);
            }
            Err(e) => { eprintln!("merge: {e}"); process::exit(1); }
        }
    }

    if args.len() < 5 {
        eprintln!("{USAGE}");
        process::exit(2);
    }

    let cmd = args[1].as_str();
    let computation_id = args[2].as_str();
    let input = args[3].as_str();
    let receipt_path = args[4].as_str();

    match cmd {
        "score" => match score(computation_id, input) {
            Ok(receipt) => {
                if let Err(e) = receipt.write_to_file(receipt_path) {
                    eprintln!("error writing receipt: {e}");
                    process::exit(1);
                }
                println!("Receipt written to {receipt_path}");
                println!("Output CID: {}", receipt.output_cid);
            }
            Err(e) => {
                eprintln!("score failed: {e}");
                process::exit(1);
            }
        },
        "verify-receipt" => {
            let receipt = match Receipt::read_from_file(receipt_path) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("verify failed: read receipt: {e}");
                    process::exit(1);
                }
            };
            match verify(computation_id, input, &receipt) {
                Ok(VerifyOutcome::Ok { output_cid, .. }) => {
                    println!("Receipt valid. Output CID: {output_cid}");
                    println!("VERIFY_OK");
                    process::exit(0);
                }
                Ok(VerifyOutcome::ContentMismatch {
                    expected,
                    actual_in_receipt,
                }) => {
                    println!("Content mismatch: expected={expected}, receipt={actual_in_receipt}");
                    println!("VERIFY_FAIL");
                    process::exit(1);
                }
                Err(e) => {
                    eprintln!("verify failed: {e}");
                    process::exit(1);
                }
            }
        }
        _ => {
            eprintln!("{USAGE}");
            process::exit(2);
        }
    }
}
