//! End-to-end coverage for `lyra lint` / `skill_lint`. The unit tests in
//! `src/linter.rs` exercise the rule predicates; this file covers the
//! integration surface — CLI exit codes, JSON contract, MCP wire shape,
//! and the empirical invariant that motivates Tier-0 (all bundled
//! examples pass at the default tier).

use std::path::PathBuf;

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples")
}

/// Every shipped example MUST pass Tier-0. This is the load-bearing
/// claim of the v0.4 lint design: the default rule set never rejects
/// a real, working skill.
#[test]
fn all_bundled_examples_pass_tier0() {
    let dir = examples_dir();
    let mut tested = 0;
    for entry in std::fs::read_dir(&dir).expect("examples dir") {
        let entry = entry.expect("read_dir entry");
        if !entry.file_type().expect("file type").is_dir() { continue; }
        let skill_md = entry.path().join("SKILL.md");
        if !skill_md.exists() { continue; }
        let md = std::fs::read_to_string(&skill_md)
            .unwrap_or_else(|e| panic!("read {}: {e}", skill_md.display()));
        let outcome = lyra_ref::linter::lint(&md, false);
        assert!(
            matches!(outcome, lyra_ref::linter::LintOutcome::Clean),
            "Tier-0 should pass on {}, but got: {}",
            skill_md.display(),
            lyra_ref::linter::outcome_to_json(&outcome),
        );
        tested += 1;
    }
    assert!(tested >= 3, "expected at least 3 bundled examples, found {tested}");
}

/// Advisory mode surfaces real convention mismatches in our own corpus
/// (some examples are Hermes-style; some are Lyra-native), confirming
/// the --strict tier is doing useful work and isn't a no-op.
#[test]
fn strict_mode_surfaces_at_least_one_advisory_across_corpus() {
    let dir = examples_dir();
    let mut any_advisory = false;
    for entry in std::fs::read_dir(&dir).expect("examples dir") {
        let entry = entry.expect("read_dir entry");
        let skill_md = entry.path().join("SKILL.md");
        if !skill_md.exists() { continue; }
        let md = std::fs::read_to_string(&skill_md).expect("read");
        if let lyra_ref::linter::LintOutcome::Advisory { .. } =
            lyra_ref::linter::lint(&md, true)
        {
            any_advisory = true;
            break;
        }
    }
    assert!(any_advisory,
        "advisory tier should fire on at least one bundled example \
         (corpus mixes Lyra-native and Hermes-style — both have \
          intentional strict-rule deltas)");
}

/// The CLI's JSON contract is the public surface. Clean output is
/// exactly `{"status":"clean"}` — no trailing whitespace, no fields,
/// no version markers. Tools that parse this must keep working.
#[test]
fn clean_json_is_compact_and_stable() {
    let md = "---\nname: ok\ndescription: hi\n---\n# OK\n## S\n- x\n\nLorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore et dolore magna aliqua ut enim ad minim veniam quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat duis aute irure dolor in reprehenderit voluptate velit.";
    let outcome = lyra_ref::linter::lint(md, false);
    assert_eq!(
        lyra_ref::linter::outcome_to_json(&outcome),
        r#"{"status":"clean"}"#,
    );
}

/// Tier-0 failure carries a non-empty `rules` array. Each rule has
/// `id`, `tier`, `message`. The wire shape is the contract.
#[test]
fn tier0_failure_emits_rules_with_required_fields() {
    let md = "# Just a heading\n## sub\n- x\n";
    let outcome = lyra_ref::linter::lint(md, false);
    let json = lyra_ref::linter::outcome_to_json(&outcome);
    assert!(json.contains(r#""status":"lint_failed""#));
    assert!(json.contains(r#""rules":["#));
    assert!(json.contains(r#""id":"frontmatter-yaml""#));
    assert!(json.contains(r#""tier":"tier0""#));
    assert!(json.contains(r#""message":"#));
}

/// Advisory mode must NEVER produce `status:lint_failed`. Strict
/// diagnostics escalate to `advisory`, not failure.
#[test]
fn strict_diagnostics_never_escalate_to_lint_failed() {
    // Lyra-native shape: tier-0 clean, advisory under --strict.
    let md = "---\nname: lyra-native\ncontract: {\"x\":1}\n---\n# Lyra-Native Skill\n## Body\n- step\n\nLorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore et dolore magna aliqua ut enim ad minim veniam quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat duis aute irure dolor in reprehenderit voluptate velit.";
    let json = lyra_ref::linter::outcome_to_json(&lyra_ref::linter::lint(md, true));
    assert!(!json.contains("lint_failed"),
        "strict-only diagnostics must not produce lint_failed; got: {json}");
    assert!(json.contains(r#""status":"advisory""#),
        "expected status=advisory in: {json}");
}
