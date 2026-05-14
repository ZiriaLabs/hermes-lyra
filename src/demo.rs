//! Self-contained demo & acceptance vectors.
//!
//! Bakes the three example skill descriptors into the binary so
//! every user can:
//!
//!   * `lyra demo tripwire` — run the self-modification tripwire demo
//!                            on any OS with zero shell dependencies.
//!   * `lyra self-check`    — verify their local binary matches the
//!                            published runtime by re-deriving the
//!                            expected output hashes.
//!
//! ## Why these are in the library, not a shell script
//!
//! The protocol's value is determinism. Shipping the demo as a `.sh`
//! script forks the demo experience by OS (`base64` flags differ, `bash`
//! is not on every system). Embedding the example descriptors and
//! running them through the in-process API gives identical bytes on
//! every machine. The same routine doubles as the acceptance suite a
//! user can run locally to confirm their build is honest — no CI to
//! trust, no upstream pin to fetch.
//!
//! ## Updating the acceptance vectors
//!
//! Vectors are tied to the `LYRA_RUNTIME_IDENT`. When the substrate
//! version changes, hashes change — that is the whole point of
//! `runtime_is_compatible`. Re-derive the vectors with
//! `cargo run --release --bin lyra -- demo tripwire`, copy the printed
//! hashes here, and add the new ident to `COMPATIBLE_RUNTIMES`.

use crate::cli_api::{base64_encode, score};
use crate::computations;
use crate::receipt::Receipt;

// ---------------------------------------------------------------
// Embedded example descriptors (single source of truth on disk).
// ---------------------------------------------------------------

/// Inbox-triage and news-brief now ship as single-file SKILL.md
/// artifacts — the descriptor and its self-verifying proof live in
/// the embedded ```lyra``` block. We extract at runtime so the
/// canonical source on disk is the markdown the user authored.
const INBOX_TRIAGE_MD: &str =
    include_str!("../examples/inbox-triage/SKILL.md");
const NEWS_BRIEF_MD: &str =
    include_str!("../examples/news-brief/SKILL.md");

// `code-review-evolve` is the tripwire demo — its three descriptors
// (v0.1.0 in production + two candidate mutations) are pedagogical
// artifacts, not deployed skills, so they stay as standalone JSON
// descriptors.
pub const CODE_REVIEW_V010: &str =
    include_str!("../examples/code-review-evolve/v0.1.0.lyra.json");
pub const CODE_REVIEW_V011: &str =
    include_str!("../examples/code-review-evolve/v0.1.1.lyra.json");
pub const CODE_REVIEW_V020_BAD: &str =
    include_str!("../examples/code-review-evolve/v0.2.0-bad.lyra.json");

/// Pull the descriptor out of `inbox-triage`'s SKILL.md.
pub fn inbox_triage_descriptor() -> String {
    crate::bridge::descriptor_from_anywhere(INBOX_TRIAGE_MD)
        .expect("inbox-triage SKILL.md must carry a descriptor")
}

/// Pull the descriptor out of `news-brief`'s SKILL.md.
pub fn news_brief_descriptor() -> String {
    crate::bridge::descriptor_from_anywhere(NEWS_BRIEF_MD)
        .expect("news-brief SKILL.md must carry a descriptor")
}

// ---------------------------------------------------------------
// Acceptance vectors — pinned to current LYRA_RUNTIME_IDENT.
// If you change anything that affects canonical bytes (or substrate),
// these must be updated in lockstep. `lyra self-check` exists to catch
// any unintended drift.
// ---------------------------------------------------------------

const EXPECTED_INBOX_TRIAGE_CID: &str =
    "bagaaihra5rqt4axxoz5v7tcl23igs2vmgdqrpd3upaywakr7fdmrcqwyuuma";
const EXPECTED_NEWS_BRIEF_CID: &str =
    "bagaaihrawg3662u3s2o55wmqvz3qizzqxzk32aqf62svh7amkny76q2il6yq";
const EXPECTED_CR_V010_CID: &str =
    "bagaaihradmrqy5zpr2cszqhzkuo4alqmkja7k2bpcr53twr5etrvvygtoecq";
const EXPECTED_CR_V011_CID: &str =
    "bagaaihrandv5kdnwex6wvc6yfr5shv2ondhuca4rnkrbue75ykz2nkqrm7bq";
const EXPECTED_LINEAGE_CID: &str =
    "bagaaihraspjrxx4d4eaofcxid6fljsbvdeqqu34fr5fohhgfj5ogjjbums6a";

// ---------------------------------------------------------------
// Tripwire demo
// ---------------------------------------------------------------

/// Run the self-modification tripwire demo. Prints step-by-step output
/// and returns `Ok(())` if both outcomes (PROMOTE good child, ROLLBACK
/// bad child) match expectation. Non-zero exit on any deviation, so CI
/// can wrap this.
pub fn run_tripwire() -> Result<(), String> {
    let bold = "\x1b[1m";
    let dim  = "\x1b[2m";
    let grn  = "\x1b[32m";
    let red  = "\x1b[31m";
    let amb  = "\x1b[33m";
    let off  = "\x1b[0m";

    println!();
    println!("{bold}{amb}== Lyra self-modification tripwire =={off}");
    println!("{dim}A cron tries to promote two child versions of `code-review-evolve`.{off}");
    println!("{dim}Only the legitimate refinement is allowed to ship.{off}");
    println!();

    // 1. Mint parent receipt.
    println!("{bold}[1/3]{off} mint parent receipt (v0.1.0, in production)");
    let parent_receipt = score("skill_interface_hash", CODE_REVIEW_V010)
        .map_err(|e| format!("parent score: {e}"))?;
    println!(
        "  parent output_hash = {dim}{}...{off}",
        &parent_receipt.output_cid[..16]
    );

    // 2. Good case: v0.1.1.
    println!();
    println!("{bold}[2/3]{off} cron proposes v0.1.1 (adds `category` field)");
    let pr_b64 = base64_encode(parent_receipt.to_json().as_bytes());
    let ng_input_good = format!(
        r#"{{"parent_receipt":"{pr_b64}","child_descriptor":{CODE_REVIEW_V011}}}"#,
    );
    match score("next_generation", &ng_input_good) {
        Ok(r) => println!(
            "  {grn}OK{off} lineage receipt minted = {dim}{}...{off}\n  {grn}PROMOTE{off} v0.1.1 to production.",
            &r.output_cid[..16]
        ),
        Err(e) => return Err(format!("legitimate refinement was rejected: {e}")),
    }

    // 3. Bad case: v0.2.0-bad. Drops `severity`. R4 fails.
    println!();
    println!("{bold}[3/3]{off} cron proposes v0.2.0-bad (drops `severity` to save tokens)");
    let ng_input_bad = format!(
        r#"{{"parent_receipt":"{pr_b64}","child_descriptor":{CODE_REVIEW_V020_BAD}}}"#,
    );
    match score("next_generation", &ng_input_bad) {
        Ok(_) => return Err("regression was promoted; tripwire did not fire".into()),
        Err(e) => {
            let short = e.chars().take(160).collect::<String>();
            println!("  {red}lineage rejected{off}: {short}");
            println!("  {red}ROLLBACK{off}. v0.2.0-bad stays in staging. Operator paged.");
        }
    }

    println!();
    println!("{bold}== done. =={off} both outcomes are deterministic and replayable on any machine");
    println!("{dim}with the same lyra-ref + uor-foundation substrate.{off}");
    println!();
    Ok(())
}

// ---------------------------------------------------------------
// Self-check — decentralized acceptance suite
// ---------------------------------------------------------------

/// Run the embedded acceptance suite. Each check re-derives a known
/// output hash from a known input and compares it against a vector
/// baked into the binary. Any mismatch is a hard failure.
///
/// This gives anyone with the binary a way to prove it is honest
/// without trusting a CI: same input + same substrate must produce the
/// same output bytes, byte for byte.
pub fn run_self_check() -> Result<(), String> {
    let mut passed = 0u32;
    let cases: &[(&str, &str, &str)] = &[
        ("inbox-triage      / skill_interface_hash", &inbox_triage_descriptor(), EXPECTED_INBOX_TRIAGE_CID),
        ("news-brief        / skill_interface_hash", &news_brief_descriptor(),   EXPECTED_NEWS_BRIEF_CID),
        ("code-review v0.1.0/ skill_interface_hash", CODE_REVIEW_V010, EXPECTED_CR_V010_CID),
        ("code-review v0.1.1/ skill_interface_hash", CODE_REVIEW_V011, EXPECTED_CR_V011_CID),
    ];

    println!("lyra self-check (runtime: {})", crate::LYRA_RUNTIME_IDENT);
    println!("--------------------------------------------------------");
    for (label, descriptor, expected) in cases {
        let r = score("skill_interface_hash", descriptor)
            .map_err(|e| format!("{label}: score: {e}"))?;
        if r.output_cid != *expected {
            return Err(format!(
                "{label}\n  expected: {expected}\n  got:      {}",
                r.output_cid
            ));
        }
        println!("  PASS  {label}");
        passed += 1;
    }

    // Lineage acceptance vector: v0.1.0 -> v0.1.1 must produce the
    // pinned 32-byte output. Exercises descriptor parsing, refinement
    // (R1-R5), and the typed UOR pipeline end-to-end.
    let parent = score("skill_interface_hash", CODE_REVIEW_V010)
        .map_err(|e| format!("lineage parent: {e}"))?;
    let pr_b64 = base64_encode(parent.to_json().as_bytes());
    let ng_input = format!(
        r#"{{"parent_receipt":"{pr_b64}","child_descriptor":{CODE_REVIEW_V011}}}"#,
    );
    let lineage = score("next_generation", &ng_input)
        .map_err(|e| format!("lineage score: {e}"))?;
    if lineage.output_cid != EXPECTED_LINEAGE_CID {
        return Err(format!(
            "lineage v0.1.0->v0.1.1\n  expected: {EXPECTED_LINEAGE_CID}\n  got:      {}",
            lineage.output_cid
        ));
    }
    println!("  PASS  lineage v0.1.0->v0.1.1 / next_generation");
    passed += 1;

    // Negative case: v0.1.0 -> v0.2.0-bad must fail with R4
    // OutputWidened. Acceptance is that the typed builder rejects the
    // refinement (computations::run returns Err).
    let ng_bad = format!(
        r#"{{"parent_receipt":"{pr_b64}","child_descriptor":{CODE_REVIEW_V020_BAD}}}"#,
    );
    match computations::run("next_generation", &ng_bad) {
        Ok(_) => return Err("v0.2.0-bad was accepted; tripwire dead".into()),
        Err(e) if e.contains("OutputWidened") => {
            println!("  PASS  v0.2.0-bad rejected with OutputWidened (tripwire live)");
            passed += 1;
        }
        Err(e) => return Err(format!("v0.2.0-bad: wrong rejection reason: {e}")),
    }

    // Verify roundtrip on one receipt: write, parse, compare.
    let json = parent.to_json();
    let parsed = Receipt::from_json(&json).map_err(|e| format!("receipt roundtrip: {e}"))?;
    if parsed.output_cid != parent.output_cid {
        return Err("receipt roundtrip mismatch".into());
    }
    println!("  PASS  receipt JSON roundtrip");
    passed += 1;

    println!("--------------------------------------------------------");
    println!("self-check: {passed}/{passed} PASS");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tripwire_runs_clean_in_process() {
        run_tripwire().expect("tripwire must succeed in the standard configuration");
    }

    #[test]
    fn self_check_runs_clean_in_process() {
        run_self_check().expect("self-check must pass against the pinned vectors");
    }
}
