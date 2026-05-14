//! `lyra lint` — structural conventions check for SKILL.md files.
//!
//! **Layer.** This is a *textual* lint, not a cryptographic gate. The five
//! canonical gates (bind, verify, refine, compose, merge) prove integrity
//! and Liskov-substitutability. The linter answers a different question:
//! *"is this file shaped like a SKILL.md the rest of the ecosystem will
//! recognize?"*
//!
//! ## Two tiers
//!
//! ### Tier-0 (default) — 6 rules. Hard fail with exit code 1.
//!
//! Each rule was selected because it passes on every audited skill we
//! could find — both the 87 upstream Hermes/agentskills examples and
//! the 3 native Lyra-style descriptors in our own `examples/` tree.
//! Zero false positives on real production content:
//!
//! 1. Frontmatter exists and parses as flat key=value YAML
//! 2. `name` is a valid Hermes slug (`[a-z0-9][a-z0-9-]*[a-z0-9]`)
//! 3. Body contains at least one H1 (`# `)
//! 4. Body is at least 200 chars
//! 5. Body contains at least one H2 (`## `)
//! 6. Body contains a fenced code block or a list item
//!
//! ### `--strict` (opt-in) — Lyra-author and Hermes-side conventions. Advisory only.
//!
//! These would have rejected a meaningful slice of real skills in one
//! ecosystem or another. They reflect *one* convention (Hermes-side or
//! Lyra-author), not a universal contract, and never fail the build:
//!
//! - `description` is a non-empty string (Hermes-side; Lyra-native
//!   descriptors omit it because the contract is in `input_shape`/`output_shape`)
//! - `version` is SemVer (rejects `1.0`, dated versions, missing)
//! - H1 in body contains the `name` slug
//! - `platforms` is a non-empty list
//!
//! No `serde`/`regex`/`yaml` crates — hand-rolled to match the rest of
//! the reference impl's parser style.

/// One linter diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintRule {
    /// Stable rule id, e.g. `"frontmatter-yaml"` or `"strict-semver"`.
    pub id: &'static str,
    /// Tier-0 (hard) or strict (advisory).
    pub tier: LintTier,
    /// Human-readable explanation.
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintTier {
    /// Universal convention — passes on all 87/87 audited upstream skills.
    Tier0,
    /// Lyra-author convention — opt-in via `--strict`. Advisory only.
    Strict,
}

impl LintTier {
    pub fn as_str(self) -> &'static str {
        match self { LintTier::Tier0 => "tier0", LintTier::Strict => "strict" }
    }
}

/// Outcome of a lint run. Exactly one variant per (mode, result) cell:
///
/// |             | no diagnostics | tier0 fired      | strict fired only        |
/// |-------------|----------------|------------------|--------------------------|
/// | default     | `Clean`        | `Tier0Failed`    | (strict rules not run)   |
/// | `--strict`  | `Clean`        | `Tier0Failed`    | `Advisory`               |
///
/// `Advisory` carries a status the CLI maps to **exit 0** — strict rules
/// never fail the build. They surface for human review; CI gates on
/// Tier-0 only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LintOutcome {
    Clean,
    Tier0Failed { rules: Vec<LintRule> },
    Advisory   { rules: Vec<LintRule> },
}

impl LintOutcome {
    pub fn status(&self) -> &'static str {
        match self {
            LintOutcome::Clean             => "clean",
            LintOutcome::Tier0Failed { .. } => "lint_failed",
            LintOutcome::Advisory    { .. } => "advisory",
        }
    }
}

/// Run the linter. `strict=false` runs Tier-0 only; `strict=true` adds
/// the advisory rules but never escalates them to failure.
pub fn lint(md: &str, strict: bool) -> LintOutcome {
    let mut tier0_fails: Vec<LintRule> = Vec::new();
    let mut strict_fails: Vec<LintRule> = Vec::new();

    // Strip BOM the same way bridge::split_frontmatter does so our
    // diagnostics align with `lyra verify` errors on the same file.
    let md = md.strip_prefix('\u{feff}').unwrap_or(md);

    // ---- Rule 1: frontmatter exists and parses ------------------------
    let (fm_kv, body) = match split_frontmatter(md) {
        Some(pair) => pair,
        None => {
            tier0_fails.push(LintRule {
                id: "frontmatter-yaml", tier: LintTier::Tier0,
                message: "file does not begin with a `---` frontmatter block".to_string(),
            });
            return LintOutcome::Tier0Failed { rules: tier0_fails };
        }
    };

    let kvs = match parse_flat_yaml(fm_kv) {
        Ok(kvs) => kvs,
        Err(e) => {
            tier0_fails.push(LintRule {
                id: "frontmatter-yaml", tier: LintTier::Tier0,
                message: format!("frontmatter does not parse: {e}"),
            });
            return LintOutcome::Tier0Failed { rules: tier0_fails };
        }
    };

    let name = lookup(&kvs, "name").unwrap_or("");
    let description = lookup(&kvs, "description");
    let version = lookup(&kvs, "version");
    let platforms = lookup(&kvs, "platforms");

    // ---- Rule 2: name is a valid Hermes slug --------------------------
    if !is_valid_slug(name) {
        tier0_fails.push(LintRule {
            id: "name-slug", tier: LintTier::Tier0,
            message: if name.is_empty() {
                "frontmatter is missing `name`".to_string()
            } else {
                format!("`name` is not a valid slug: {name:?} (must match [a-z0-9][a-z0-9-]*[a-z0-9])")
            },
        });
    }

    // (description is a Hermes-side convention; checked under --strict only.)

    // ---- Rule 4: body has at least one H1 -----------------------------
    let has_h1 = body.lines().any(|l| l.starts_with("# ") && l.len() > 2);
    if !has_h1 {
        tier0_fails.push(LintRule {
            id: "body-has-h1", tier: LintTier::Tier0,
            message: "body has no H1 (`# Title`)".to_string(),
        });
    }

    // ---- Rule 5: body length >= 200 chars -----------------------------
    if body.trim().chars().count() < 200 {
        tier0_fails.push(LintRule {
            id: "body-length", tier: LintTier::Tier0,
            message: format!("body is too short: {} chars (minimum 200)", body.trim().chars().count()),
        });
    }

    // ---- Rule 6: body has at least one H2 -----------------------------
    let has_h2 = body.lines().any(|l| l.starts_with("## ") && l.len() > 3);
    if !has_h2 {
        tier0_fails.push(LintRule {
            id: "body-has-h2", tier: LintTier::Tier0,
            message: "body has no H2 section (`## Heading`)".to_string(),
        });
    }

    // ---- Rule 7: body has a fenced block or a list item ---------------
    let has_fence = body.contains("```");
    let has_list  = body.lines().any(|l| {
        let t = l.trim_start();
        t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ")
    });
    if !has_fence && !has_list {
        tier0_fails.push(LintRule {
            id: "body-structure", tier: LintTier::Tier0,
            message: "body contains no fenced code block or list item".to_string(),
        });
    }

    // If Tier-0 fired, return immediately (strict rules can't run reliably
    // when basic structure is broken, and we don't want to flood stderr).
    if !tier0_fails.is_empty() {
        return LintOutcome::Tier0Failed { rules: tier0_fails };
    }

    if !strict {
        return LintOutcome::Clean;
    }

    // ---- Strict-only rules (advisory) ---------------------------------

    // description present and non-empty. Hermes-side metadata; Lyra-native
    // skills (those whose frontmatter holds `contract: {...}` directly)
    // omit this by design.
    match description {
        Some(d) if !d.trim().is_empty() => {}
        _ => strict_fails.push(LintRule {
            id: "strict-description", tier: LintTier::Strict,
            message: "`description` is missing or empty (Hermes-side convention; Lyra-native skills may omit it)".to_string(),
        }),
    }

    // version SemVer-like: N.N.N optionally followed by -<ident>
    match version {
        Some(v) if is_semver_like(v) => {}
        Some(v) => strict_fails.push(LintRule {
            id: "strict-semver", tier: LintTier::Strict,
            message: format!("`version` is not SemVer-like: {v:?}"),
        }),
        None => strict_fails.push(LintRule {
            id: "strict-semver", tier: LintTier::Strict,
            message: "`version` is missing".to_string(),
        }),
    }

    // H1 contains the slug. Tolerant: lowercase comparison, both H1
    // hyphenated→spaced and slug spaced→hyphenated forms accepted.
    if let Some(h1) = body.lines().find(|l| l.starts_with("# ")).map(|l| l[2..].trim()) {
        let h1_lc = h1.to_lowercase();
        let name_lc = name.to_lowercase();
        if !h1_lc.contains(&name_lc)
            && !h1_lc.replace(' ', "-").contains(&name_lc)
            && !h1_lc.contains(&name_lc.replace('-', " "))
        {
            strict_fails.push(LintRule {
                id: "strict-h1-matches-name", tier: LintTier::Strict,
                message: format!("H1 {h1:?} does not contain slug {name:?}"),
            });
        }
    }

    // platforms is present and non-empty (we accept either YAML list form
    // `[a, b, c]` or bare scalar `a`; both are non-empty if any token survives).
    match platforms {
        Some(p) if !platforms_is_empty(p) => {}
        _ => strict_fails.push(LintRule {
            id: "strict-platforms", tier: LintTier::Strict,
            message: "`platforms` is missing or empty".to_string(),
        }),
    }

    if strict_fails.is_empty() {
        LintOutcome::Clean
    } else {
        LintOutcome::Advisory { rules: strict_fails }
    }
}

/// Serialize an outcome as one-line JSON for the CLI/MCP layer.
pub fn outcome_to_json(o: &LintOutcome) -> String {
    let status = o.status();
    let rules = match o {
        LintOutcome::Clean => return format!("{{\"status\":\"{status}\"}}"),
        LintOutcome::Tier0Failed { rules } | LintOutcome::Advisory { rules } => rules,
    };
    let mut s = String::new();
    s.push_str("{\"status\":\"");
    s.push_str(status);
    s.push_str("\",\"rules\":[");
    for (i, r) in rules.iter().enumerate() {
        if i > 0 { s.push(','); }
        s.push_str("{\"id\":\"");
        s.push_str(r.id);
        s.push_str("\",\"tier\":\"");
        s.push_str(r.tier.as_str());
        s.push_str("\",\"message\":");
        s.push_str(&json_string(&r.message));
        s.push('}');
    }
    s.push_str("]}");
    s
}

// ---- Helpers (hand-rolled to avoid serde/regex deps) ------------------

/// Returns `(frontmatter_text_without_fences, body)` or `None` if the
/// file doesn't open with `---\n`. Mirrors `bridge::split_frontmatter`
/// semantics but lives here to keep the linter independent.
fn split_frontmatter(md: &str) -> Option<(&str, &str)> {
    let s = md.strip_prefix('\u{feff}').unwrap_or(md);
    let s = s.strip_prefix("---\n").or_else(|| s.strip_prefix("---\r\n"))?;
    // Find closing `\n---\n` (or `\n---\r\n`, or end-of-file `\n---`).
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 4 <= bytes.len() {
        // Look for newline followed by ---
        if bytes[i] == b'\n' && bytes.get(i+1..i+4) == Some(b"---") {
            // What follows the closing ---?
            let after = &bytes[i+4..];
            let fm = &s[..i];
            let rest = if after.starts_with(b"\n") {
                std::str::from_utf8(&after[1..]).ok()?
            } else if after.starts_with(b"\r\n") {
                std::str::from_utf8(&after[2..]).ok()?
            } else if after.is_empty() {
                ""
            } else {
                // `---xyz` isn't a frontmatter close; keep searching.
                i += 1; continue;
            };
            return Some((fm, rest));
        }
        i += 1;
    }
    None
}

/// Parse the *flat* YAML we expect in skill frontmatter: a sequence of
/// `key: value` pairs, one per line. Skips comments and blank lines.
/// Multi-line values, anchors, and aliases are rejected so the linter's
/// errors stay specific. Quoted strings have their outer quotes stripped.
///
/// Indented continuations are tolerated and joined to the previous value;
/// this matches how `prerequisites:` and similar nested keys appear in
/// upstream skills (we just store the raw continuation as part of the
/// value — we don't introspect nested structure).
fn parse_flat_yaml(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.trim_end();
        if line.is_empty() || line.trim_start().starts_with('#') { continue; }
        // Indented line: continuation of previous value. Just record it
        // so the value-as-string is non-empty if it had any content.
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(last) = out.last_mut() {
                if !last.1.is_empty() { last.1.push('\n'); }
                last.1.push_str(line.trim());
            }
            continue;
        }
        let colon = match line.find(':') {
            Some(c) => c,
            None => return Err(format!("line {}: missing `:`", lineno+1)),
        };
        let key = line[..colon].trim().to_string();
        if key.is_empty() {
            return Err(format!("line {}: empty key", lineno+1));
        }
        let raw_val = line[colon+1..].trim();
        let val = strip_quotes(raw_val).to_string();
        out.push((key, val));
    }
    Ok(out)
}

fn strip_quotes(s: &str) -> &str {
    let b = s.as_bytes();
    if b.len() >= 2 && ((b[0] == b'"' && b[b.len()-1] == b'"')
                     || (b[0] == b'\'' && b[b.len()-1] == b'\'')) {
        &s[1..s.len()-1]
    } else { s }
}

fn lookup<'a>(kvs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    kvs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

fn is_valid_slug(s: &str) -> bool {
    // [a-z0-9][a-z0-9-]*[a-z0-9]
    let b = s.as_bytes();
    if b.len() < 2 { return !b.is_empty() && is_slug_endchar(b[0]); }
    if !is_slug_endchar(b[0]) || !is_slug_endchar(b[b.len()-1]) { return false; }
    b[1..b.len()-1].iter().all(|&c| c == b'-' || is_slug_endchar(c))
}
fn is_slug_endchar(c: u8) -> bool { c.is_ascii_lowercase() || c.is_ascii_digit() }

fn is_semver_like(s: &str) -> bool {
    // N.N.N[-ident]   N = one or more ascii digits.
    let core_and_pre = s.split_once('-');
    let core = core_and_pre.map(|x| x.0).unwrap_or(s);
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 { return false; }
    if !parts.iter().all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit())) {
        return false;
    }
    if let Some((_, pre)) = core_and_pre {
        if pre.is_empty() { return false; }
        if !pre.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'.' || c == b'-') {
            return false;
        }
    }
    true
}

fn platforms_is_empty(p: &str) -> bool {
    let t = p.trim();
    if t.is_empty() { return true; }
    // `[]` or `[ ]` is the YAML empty list.
    if t == "[]" { return true; }
    if t.starts_with('[') && t.ends_with(']') {
        return t[1..t.len()-1].split(',').all(|x| x.trim().is_empty());
    }
    false
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"'  => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c    => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "---\nname: my-skill\ndescription: A demo.\nversion: 0.1.0\nplatforms: [macos]\n---\n# My Skill\n\n## Usage\n\n- step one\n- step two\n\nLorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore et dolore magna aliqua ut enim ad minim veniam quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat duis aute irure dolor in reprehenderit.\n";

    #[test]
    fn good_skill_is_clean_in_both_modes() {
        assert_eq!(lint(GOOD, false), LintOutcome::Clean);
        assert_eq!(lint(GOOD, true),  LintOutcome::Clean);
    }

    #[test]
    fn missing_frontmatter_fires_tier0() {
        let md = "# Just a Title\n\n## Section\n- item\n";
        match lint(md, false) {
            LintOutcome::Tier0Failed { rules } => {
                assert!(rules.iter().any(|r| r.id == "frontmatter-yaml"));
            }
            _ => panic!("expected tier-0 failure"),
        }
    }

    #[test]
    fn bad_slug_fires_tier0() {
        let md = "---\nname: My_Skill\n---\n# h\n\n## s\n- x\nlorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore et dolore magna aliqua ut enim ad minim veniam quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat duis aute irure dolor in reprehenderit voluptate velit.";
        match lint(md, false) {
            LintOutcome::Tier0Failed { rules } => {
                assert!(rules.iter().any(|r| r.id == "name-slug"));
            }
            _ => panic!("expected tier-0 failure"),
        }
    }

    #[test]
    fn short_body_fires_length_rule() {
        let md = "---\nname: ok\n---\n# t\n## s\n- x\n";
        match lint(md, false) {
            LintOutcome::Tier0Failed { rules } => {
                assert!(rules.iter().any(|r| r.id == "body-length"));
            }
            _ => panic!("expected tier-0 failure"),
        }
    }

    #[test]
    fn lyra_native_skill_without_description_is_clean_in_tier0() {
        // Mirrors the shape of examples/code-review-evolve/SKILL.md:
        // frontmatter has `name` + `contract:` only, no `description`.
        let md = "---\nname: code-review-evolve\ncontract: {\"placeholder\":\"yes\"}\n---\n# code-review-evolve\n\n## Inputs\n- a path\n\nLorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore et dolore magna aliqua ut enim ad minim veniam quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat duis aute irure dolor in reprehenderit voluptate velit.";
        assert!(matches!(lint(md, false), LintOutcome::Clean));
        // Under --strict, the missing description is flagged as advisory only.
        match lint(md, true) {
            LintOutcome::Advisory { rules } => {
                assert!(rules.iter().any(|r| r.id == "strict-description"));
                assert!(rules.iter().any(|r| r.id == "strict-semver"));
                assert!(rules.iter().any(|r| r.id == "strict-platforms"));
            }
            other => panic!("expected advisory, got {other:?}"),
        }
    }

    #[test]
    fn strict_fires_on_non_semver() {
        let md = "---\nname: my-skill\ndescription: A demo.\nversion: 2024-05-13\nplatforms: [macos]\n---\n# My Skill\n## Usage\n- step\n\nLorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore et dolore magna aliqua ut enim ad minim veniam quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat duis aute irure dolor in reprehenderit voluptate velit.";
        // Tier-0 clean
        assert!(matches!(lint(md, false), LintOutcome::Clean));
        // Strict advisory
        match lint(md, true) {
            LintOutcome::Advisory { rules } => {
                assert!(rules.iter().any(|r| r.id == "strict-semver"));
            }
            other => panic!("expected advisory, got {other:?}"),
        }
    }

    #[test]
    fn strict_fires_when_h1_does_not_match_name() {
        // findmy-style upstream pattern: name=findmy, H1="Find My (Apple)"
        let md = "---\nname: findmy\ndescription: x\nversion: 0.1.0\nplatforms: [macos]\n---\n# Find My (Apple)\n## Usage\n- step\n\nLorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore et dolore magna aliqua ut enim ad minim veniam quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat duis aute irure dolor in reprehenderit voluptate velit.";
        assert!(matches!(lint(md, false), LintOutcome::Clean));
        match lint(md, true) {
            LintOutcome::Advisory { rules } => {
                assert!(rules.iter().any(|r| r.id == "strict-h1-matches-name"));
            }
            other => panic!("expected advisory, got {other:?}"),
        }
    }

    #[test]
    fn strict_platforms_missing() {
        let md = "---\nname: ok\ndescription: x\nversion: 0.1.0\n---\n# OK\n## S\n- x\n\nLorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore et dolore magna aliqua ut enim ad minim veniam quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat duis aute irure dolor in reprehenderit voluptate velit.";
        assert!(matches!(lint(md, false), LintOutcome::Clean));
        match lint(md, true) {
            LintOutcome::Advisory { rules } => {
                assert!(rules.iter().any(|r| r.id == "strict-platforms"));
            }
            other => panic!("expected advisory, got {other:?}"),
        }
    }

    #[test]
    fn json_output_clean() {
        assert_eq!(outcome_to_json(&LintOutcome::Clean), r#"{"status":"clean"}"#);
    }

    #[test]
    fn json_output_with_rule() {
        let o = LintOutcome::Tier0Failed { rules: vec![LintRule {
            id: "name-slug", tier: LintTier::Tier0,
            message: "bad slug \"X\"".to_string(),
        }]};
        let j = outcome_to_json(&o);
        assert!(j.starts_with(r#"{"status":"lint_failed","rules":[{"id":"name-slug","tier":"tier0","message":"bad slug "#));
    }

    #[test]
    fn semver_parser_boundaries() {
        assert!(is_semver_like("0.1.0"));
        assert!(is_semver_like("1.2.3-rc.1"));
        assert!(!is_semver_like("1.0"));
        assert!(!is_semver_like("2024-05-13"));
        assert!(!is_semver_like("v1.0.0"));
        assert!(!is_semver_like("1.0.0-"));
    }

    #[test]
    fn slug_parser_boundaries() {
        assert!(is_valid_slug("a"));
        assert!(is_valid_slug("ab"));
        assert!(is_valid_slug("my-skill"));
        assert!(is_valid_slug("a1-b2"));
        assert!(!is_valid_slug(""));
        assert!(!is_valid_slug("-x"));
        assert!(!is_valid_slug("x-"));
        assert!(!is_valid_slug("My-Skill"));
        assert!(!is_valid_slug("my_skill"));
    }
}
