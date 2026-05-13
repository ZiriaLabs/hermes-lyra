//! `lyra install` — register the MCP server in one line.
//!
//! Hermes reads MCP servers from `~/.hermes/config.yaml` under the
//! `mcp_servers:` key. Hermes's CLI has a config-file watcher that diffs
//! that section on every save and auto-reloads MCP connections — no
//! restart needed.
//!
//! What this subcommand does:
//!
//! 1. Resolve the agent profile root (`$HERMES_HOME` if set, else `~/.hermes`).
//! 2. Resolve the running binary's absolute path via `current_exe()`.
//! 3. Surgically splice (or replace) `mcp_servers.lyra:` in `config.yaml`,
//!    preserving every other key and as much formatting as practical.
//! 4. Print one line summarizing what we did. Exit 0.
//!
//! `lyra install --uninstall` removes our entry idempotently.
//!
//! Design choices:
//!
//! - **No YAML round-trip.** We never parse-and-emit the whole file
//!   (that would destroy comments, blank lines, and key ordering).
//!   Instead we operate line-by-line on the `mcp_servers:` block.
//! - **Idempotent.** Running `lyra install` twice is a no-op on the
//!   second run; we print `unchanged` and exit 0.
//! - **Self-locating binary path.** The command we register is the
//!   actual binary that ran `install`, so PATH order and shell aliases
//!   can never make Hermes load a stale `lyra`.

use std::fs;
use std::path::{Path, PathBuf};

/// Resolve `~/.hermes` (or `$HERMES_HOME`) on this machine.
pub fn hermes_home() -> Result<PathBuf, String> {
    if let Ok(custom) = std::env::var("HERMES_HOME") {
        let p = PathBuf::from(custom);
        if !p.exists() {
            return Err(format!("HERMES_HOME points to {} which does not exist", p.display()));
        }
        return Ok(p);
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| "neither HOME nor USERPROFILE is set; pass HERMES_HOME=... explicitly".to_string())?;
    let p = PathBuf::from(home).join(".hermes");
    if !p.exists() {
        return Err(format!(
            "agent profile root {} does not exist; set HERMES_HOME=... explicitly",
            p.display()
        ));
    }
    Ok(p)
}

/// Result envelope for the install operation, returned so callers (CLI +
/// unit tests) can see what changed without reading the file again.
#[derive(Debug, PartialEq, Eq)]
pub enum InstallOutcome {
    Inserted { config_path: PathBuf, command: PathBuf },
    Updated  { config_path: PathBuf, command: PathBuf },
    Unchanged { config_path: PathBuf, command: PathBuf },
    Removed  { config_path: PathBuf },
    NotInstalled { config_path: PathBuf },
}

/// Top-level entry called by `main()` for the `install` / `install --uninstall` paths.
pub fn run(uninstall: bool) -> Result<InstallOutcome, String> {
    let home = hermes_home()?;
    let config_path = home.join("config.yaml");

    let original = if config_path.exists() {
        fs::read_to_string(&config_path).map_err(|e| format!("read {}: {e}", config_path.display()))?
    } else {
        // Brand-new install — start with an empty config so we still write a valid file.
        String::new()
    };

    if uninstall {
        let (new_text, removed) = remove_lyra_entry(&original);
        if !removed {
            return Ok(InstallOutcome::NotInstalled { config_path });
        }
        write_atomically(&config_path, &new_text)?;
        return Ok(InstallOutcome::Removed { config_path });
    }

    let bin = std::env::current_exe()
        .map_err(|e| format!("current_exe(): {e}"))?
        .canonicalize()
        .map_err(|e| format!("canonicalize binary path: {e}"))?;
    let bin_str = bin
        .to_str()
        .ok_or_else(|| "binary path is not valid UTF-8".to_string())?
        .to_string();

    let lyra_block = render_lyra_block(&bin_str);
    let (new_text, change) = splice_lyra_entry(&original, &lyra_block);
    if matches!(change, SpliceChange::Unchanged) {
        return Ok(InstallOutcome::Unchanged {
            config_path,
            command: bin,
        });
    }
    write_atomically(&config_path, &new_text)?;
    Ok(match change {
        SpliceChange::Inserted => InstallOutcome::Inserted { config_path, command: bin },
        SpliceChange::Updated => InstallOutcome::Updated { config_path, command: bin },
        SpliceChange::Unchanged => unreachable!(),
    })
}

/// The exact YAML lines we splice in (4-space-indented under `mcp_servers:`).
fn render_lyra_block(command: &str) -> String {
    // Quote conservatively. The path can contain spaces (Windows usernames
    // with spaces, anyone?). YAML double-quoted strings need backslash and
    // quote escaping; nothing else inside a path needs escaping.
    let q = yaml_quote(command);
    format!(
        "  lyra:\n    command: {q}\n    args: [\"mcp\", \"serve\"]\n    env: {{}}\n    timeout: 60\n"
    )
}

fn yaml_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

#[derive(Debug, PartialEq, Eq)]
enum SpliceChange {
    Inserted,
    Updated,
    Unchanged,
}

/// Splice the `lyra:` entry into `mcp_servers:`, creating either the
/// section or the key as needed. Returns the new file and a flag for the
/// kind of change.
fn splice_lyra_entry(original: &str, lyra_block: &str) -> (String, SpliceChange) {
    let lines: Vec<&str> = original.lines().collect();

    // Find the `mcp_servers:` section header at top-level (column 0).
    let header_idx = lines.iter().position(|l| is_top_level_key(l, "mcp_servers"));

    if header_idx.is_none() {
        // No section. Append one.
        let mut out = original.to_string();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("mcp_servers:\n");
        out.push_str(lyra_block);
        return (out, SpliceChange::Inserted);
    }

    let header = header_idx.unwrap();

    // Find the existing `lyra:` subkey inside the section, and the
    // section's end (next top-level key or EOF).
    let mut lyra_start: Option<usize> = None;
    let mut lyra_end: Option<usize> = None;
    let mut section_end: usize = lines.len();
    for (i, line) in lines.iter().enumerate().skip(header + 1) {
        let stripped = line.trim_start();
        let indent = line.len() - stripped.len();

        if indent == 0 && !stripped.is_empty() && !stripped.starts_with('#') {
            // Top-level key — section ends here.
            section_end = i;
            if let Some(start) = lyra_start {
                if lyra_end.is_none() {
                    lyra_end = Some(i);
                }
                let _ = start;
            }
            break;
        }

        // Section-internal: detect `lyra:` at 2-space indent. We accept any
        // indent ≥ 2 to be lenient with operator-edited configs.
        if lyra_start.is_none() && indent >= 2 && stripped.starts_with("lyra:") {
            // Strict: only treat it as ours if it's literally `lyra:`
            // followed by EOL or whitespace, not e.g. `lyra-pro:`.
            let after = &stripped["lyra:".len()..];
            if after.is_empty() || after.starts_with(|c: char| c.is_whitespace()) {
                lyra_start = Some(i);
            }
        } else if let Some(start) = lyra_start {
            // We're inside the `lyra:` block; it ends at the next sibling
            // (same indent as the `lyra:` line itself).
            let our_indent = lines[start].len() - lines[start].trim_start().len();
            if indent <= our_indent && !stripped.is_empty() && !stripped.starts_with('#') {
                lyra_end = Some(i);
                // Don't break — still need section_end.
            }
        }
    }
    let lyra_end = lyra_end.unwrap_or(section_end);

    let mut out = String::with_capacity(original.len() + lyra_block.len());

    if let Some(lyra_start) = lyra_start {
        // Replace existing `lyra:` block. If the new bytes equal the old, no-op.
        let existing: String = lines[lyra_start..lyra_end]
            .iter()
            .map(|s| {
                let mut t = s.to_string();
                t.push('\n');
                t
            })
            .collect();
        if existing == lyra_block {
            return (original.to_string(), SpliceChange::Unchanged);
        }

        // Reassemble: lines[..lyra_start] + lyra_block + lines[lyra_end..]
        push_with_newlines(&mut out, &lines[..lyra_start]);
        out.push_str(lyra_block);
        push_with_newlines(&mut out, &lines[lyra_end..]);
        preserve_trailing_newline(&mut out, original);
        (out, SpliceChange::Updated)
    } else {
        // Insert as the LAST entry in `mcp_servers:` (just before section_end).
        push_with_newlines(&mut out, &lines[..section_end]);
        // Ensure a newline boundary before splicing in our block.
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(lyra_block);
        push_with_newlines(&mut out, &lines[section_end..]);
        preserve_trailing_newline(&mut out, original);
        (out, SpliceChange::Inserted)
    }
}

fn remove_lyra_entry(original: &str) -> (String, bool) {
    let lines: Vec<&str> = original.lines().collect();
    let header_idx = lines.iter().position(|l| is_top_level_key(l, "mcp_servers"));
    if header_idx.is_none() {
        return (original.to_string(), false);
    }
    let header = header_idx.unwrap();
    let mut lyra_start: Option<usize> = None;
    let mut lyra_end: Option<usize> = None;
    let mut section_end: usize = lines.len();
    for (i, line) in lines.iter().enumerate().skip(header + 1) {
        let stripped = line.trim_start();
        let indent = line.len() - stripped.len();
        if indent == 0 && !stripped.is_empty() && !stripped.starts_with('#') {
            section_end = i;
            break;
        }
        if lyra_start.is_none() && indent >= 2 && stripped.starts_with("lyra:") {
            let after = &stripped["lyra:".len()..];
            if after.is_empty() || after.starts_with(|c: char| c.is_whitespace()) {
                lyra_start = Some(i);
            }
        } else if let Some(start) = lyra_start {
            let our_indent = lines[start].len() - lines[start].trim_start().len();
            if indent <= our_indent && !stripped.is_empty() && !stripped.starts_with('#') {
                lyra_end = Some(i);
                break;
            }
        }
    }

    let lyra_start = match lyra_start {
        Some(s) => s,
        None => return (original.to_string(), false),
    };
    let lyra_end = lyra_end.unwrap_or(section_end);

    let mut out = String::with_capacity(original.len());
    push_with_newlines(&mut out, &lines[..lyra_start]);
    push_with_newlines(&mut out, &lines[lyra_end..]);
    preserve_trailing_newline(&mut out, original);
    (out, true)
}

fn is_top_level_key(line: &str, key: &str) -> bool {
    if line.starts_with(' ') || line.starts_with('\t') {
        return false;
    }
    let trimmed = line.trim_end();
    trimmed == format!("{key}:") || trimmed.starts_with(&format!("{key}:"))
}

fn push_with_newlines(out: &mut String, lines: &[&str]) {
    for l in lines {
        out.push_str(l);
        out.push('\n');
    }
}

fn preserve_trailing_newline(out: &mut String, original: &str) {
    // If the original had no trailing newline, drop our last one so the
    // output's trailing-newline state matches the input. This avoids
    // churning EOF behaviour on every install.
    if !original.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
}

fn write_atomically(path: &Path, contents: &str) -> Result<(), String> {
    // Best-effort atomicity: write to <path>.tmp.<pid> then rename.
    let dir = path.parent().ok_or_else(|| format!("path {} has no parent", path.display()))?;
    let tmp = dir.join(format!(
        ".{}.lyra-install.tmp.{}",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("config"),
        std::process::id()
    ));
    fs::write(&tmp, contents).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| {
        // Clean up the temp file if rename failed.
        let _ = fs::remove_file(&tmp);
        format!("rename {} -> {}: {e}", tmp.display(), path.display())
    })?;
    Ok(())
}

// ---------------------------------------------------------------
// Tests
// ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn block() -> String {
        render_lyra_block("/usr/local/bin/lyra")
    }

    #[test]
    fn yaml_quote_handles_backslashes_and_quotes() {
        assert_eq!(yaml_quote("/usr/local/bin/lyra"), "\"/usr/local/bin/lyra\"");
        assert_eq!(
            yaml_quote("C:\\Users\\Path With Space\\lyra.exe"),
            "\"C:\\\\Users\\\\Path With Space\\\\lyra.exe\""
        );
        assert_eq!(yaml_quote(r#"weird"path"#), "\"weird\\\"path\"");
    }

    #[test]
    fn render_block_shape_matches_hermes_schema() {
        let b = render_lyra_block("/usr/local/bin/lyra");
        assert!(b.starts_with("  lyra:\n"));
        assert!(b.contains("    command: \"/usr/local/bin/lyra\"\n"));
        assert!(b.contains("    args: [\"mcp\", \"serve\"]\n"));
        assert!(b.contains("    env: {}\n"));
        assert!(b.contains("    timeout: 60\n"));
    }

    #[test]
    fn insert_into_empty_file_creates_section_and_entry() {
        let (out, change) = splice_lyra_entry("", &block());
        assert_eq!(change, SpliceChange::Inserted);
        assert_eq!(out, format!("mcp_servers:\n{}", block()));
    }

    #[test]
    fn insert_into_config_without_mcp_section_appends_section() {
        let original = "model: gpt-5\nlog_level: info\n";
        let (out, change) = splice_lyra_entry(original, &block());
        assert_eq!(change, SpliceChange::Inserted);
        assert!(out.starts_with("model: gpt-5\nlog_level: info\n"));
        assert!(out.contains("\nmcp_servers:\n"));
        assert!(out.ends_with(&block()));
    }

    #[test]
    fn insert_alongside_other_mcp_entries_preserves_them() {
        let original = "\
model: gpt-5
mcp_servers:
  filesystem:
    command: \"npx\"
    args: [\"-y\", \"@modelcontextprotocol/server-filesystem\", \"/tmp\"]
log_level: info
";
        let (out, change) = splice_lyra_entry(original, &block());
        assert_eq!(change, SpliceChange::Inserted);
        // Filesystem must survive.
        assert!(out.contains("  filesystem:\n"));
        // Lyra must be inserted at the end of the section, BEFORE the next top-level key.
        let lyra_pos = out.find("  lyra:\n").expect("lyra block must be spliced in");
        let log_pos = out.find("log_level:").expect("trailing top-level keys must survive");
        assert!(
            lyra_pos < log_pos,
            "lyra block must precede the next top-level key\n{out}"
        );
        // And after filesystem (last-in-section behaviour).
        let fs_pos = out.find("  filesystem:").unwrap();
        assert!(fs_pos < lyra_pos);
    }

    #[test]
    fn second_install_is_unchanged() {
        let (once, _) = splice_lyra_entry("", &block());
        let (twice, change) = splice_lyra_entry(&once, &block());
        assert_eq!(change, SpliceChange::Unchanged);
        assert_eq!(once, twice);
    }

    #[test]
    fn install_with_different_path_updates_in_place() {
        let original = format!("mcp_servers:\n{}", render_lyra_block("/old/path/lyra"));
        let new_block = render_lyra_block("/new/path/lyra");
        let (out, change) = splice_lyra_entry(&original, &new_block);
        assert_eq!(change, SpliceChange::Updated);
        assert!(out.contains("/new/path/lyra"));
        assert!(!out.contains("/old/path/lyra"));
    }

    #[test]
    fn uninstall_removes_entry_only() {
        let original = format!(
            "model: gpt-5\nmcp_servers:\n  filesystem:\n    command: \"npx\"\n{}log_level: info\n",
            render_lyra_block("/usr/local/bin/lyra")
        );
        let (out, removed) = remove_lyra_entry(&original);
        assert!(removed);
        assert!(!out.contains("lyra:"));
        assert!(out.contains("  filesystem:\n"));
        assert!(out.contains("model: gpt-5\n"));
        assert!(out.contains("log_level: info\n"));
    }

    #[test]
    fn uninstall_when_not_installed_is_noop() {
        let original = "model: gpt-5\nmcp_servers:\n  filesystem:\n    command: \"npx\"\n";
        let (out, removed) = remove_lyra_entry(original);
        assert!(!removed);
        assert_eq!(out, original);
    }

    #[test]
    fn lyra_pro_is_not_matched_as_lyra() {
        // Regression: `lyra-pro:` (a hypothetical sibling) must not be
        // matched by the `lyra:` detector.
        let original = format!(
            "mcp_servers:\n  lyra-pro:\n    command: \"x\"\n    args: []\n"
        );
        let (out, change) = splice_lyra_entry(&original, &block());
        assert_eq!(change, SpliceChange::Inserted);
        assert!(out.contains("  lyra-pro:\n"));
        assert!(out.contains("  lyra:\n"));
    }

    #[test]
    fn preserves_trailing_newline_when_no_eof_insertion() {
        // When the splice point is mid-file (existing mcp_servers section),
        // we must preserve the input's trailing-newline state.
        let with_nl = "mcp_servers:\n  filesystem:\n    command: \"npx\"\nlog_level: info\n";
        let (out_with, _) = splice_lyra_entry(with_nl, &block());
        assert!(out_with.ends_with('\n'));

        let no_nl = "mcp_servers:\n  filesystem:\n    command: \"npx\"\nlog_level: info";
        let (out_no, _) = splice_lyra_entry(no_nl, &block());
        assert!(!out_no.ends_with('\n'),
            "should preserve missing trailing newline; got:\n{out_no:?}");
    }
}
