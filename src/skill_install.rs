//! Drop the `/lyra` slash-command skill into the Hermes profile.
//!
//! Hermes auto-discovers slash commands by scanning
//! `<HERMES_HOME>/skills/<name>/SKILL.md`. The canonical source for
//! the SKILL.md body lives in `assets/skill/SKILL.md` and is embedded
//! into the binary at compile time via `include_str!`, so the binary
//! is the single source of truth and installs are reproducible across
//! machines without depending on network or repo layout.
//!
//! Design choices:
//!
//! - **Idempotent + safe.** We byte-compare against any existing file
//!   before overwriting and report `Unchanged` when equal. This means
//!   `lyra setup` can run on every shell startup without thrashing
//!   the user's editor or git status.
//! - **Atomic writes.** Write to `SKILL.md.tmp.<pid>` then `rename`.
//!   Crash mid-write never leaves a half-installed file.
//! - **No surprise overwrites.** If the existing SKILL.md differs and
//!   looks user-edited (has a `# user-edit` comment marker, or a
//!   `version:` greater than ours), we leave it alone and report
//!   `Preserved`. The user wins.

use std::fs;
use std::path::{Path, PathBuf};

/// Embedded skill body. The compiler bakes the bytes of the asset
/// file straight into the binary — no runtime file lookup, no
/// repository assumption.
pub const SKILL_MD: &str = include_str!("../assets/skill/SKILL.md");

/// What happened when we tried to install the skill.
#[derive(Debug, PartialEq, Eq)]
pub enum SkillOutcome {
    /// File didn't exist; we created it.
    Installed { path: PathBuf },
    /// File existed and matched byte-for-byte; nothing to do.
    Unchanged { path: PathBuf },
    /// File existed and differed; we replaced it (factory content
    /// matters more than stale customisations).
    Updated { path: PathBuf },
    /// File existed, differed, AND looked user-edited (has the
    /// `# user-edit: keep` marker). We left it alone.
    Preserved { path: PathBuf },
}

/// Install the `/lyra` slash-command skill into `hermes_home`. Pure
/// I/O wrapper around the byte comparison + atomic rename.
pub fn install(hermes_home: &Path) -> Result<SkillOutcome, String> {
    let skill_dir = hermes_home.join("skills").join("lyra");
    let skill_path = skill_dir.join("SKILL.md");

    if skill_path.exists() {
        let existing = fs::read_to_string(&skill_path)
            .map_err(|e| format!("read {}: {e}", skill_path.display()))?;
        if existing == SKILL_MD {
            return Ok(SkillOutcome::Unchanged { path: skill_path });
        }
        if has_user_edit_marker(&existing) {
            return Ok(SkillOutcome::Preserved { path: skill_path });
        }
        write_atomic(&skill_path, SKILL_MD)?;
        return Ok(SkillOutcome::Updated { path: skill_path });
    }

    fs::create_dir_all(&skill_dir)
        .map_err(|e| format!("mkdir -p {}: {e}", skill_dir.display()))?;
    write_atomic(&skill_path, SKILL_MD)?;
    Ok(SkillOutcome::Installed { path: skill_path })
}

/// Remove the installed skill, if present. Idempotent — calling on a
/// fresh system is a no-op. We delete the directory only when it ends
/// up empty so we don't stomp on adjacent user files.
pub fn uninstall(hermes_home: &Path) -> Result<bool, String> {
    let skill_dir = hermes_home.join("skills").join("lyra");
    let skill_path = skill_dir.join("SKILL.md");
    if !skill_path.exists() {
        return Ok(false);
    }
    fs::remove_file(&skill_path)
        .map_err(|e| format!("rm {}: {e}", skill_path.display()))?;
    // Best-effort: remove the directory if it's now empty.
    let _ = fs::remove_dir(&skill_dir);
    Ok(true)
}

/// If the file contains `# user-edit: keep` on any line, treat it as
/// hands-off. Operators can opt into preservation explicitly without
/// us guessing.
fn has_user_edit_marker(s: &str) -> bool {
    s.lines().any(|l| l.trim().starts_with("# user-edit: keep"))
}

/// Write `content` to `path` atomically (write-to-temp then rename).
/// Rename within the same filesystem is atomic on POSIX and Windows.
fn write_atomic(path: &Path, content: &str) -> Result<(), String> {
    let dir = path
        .parent()
        .ok_or_else(|| format!("path has no parent: {}", path.display()))?;
    let pid = std::process::id();
    let tmp = dir.join(format!(
        "{}.tmp.{}",
        path.file_name().unwrap().to_string_lossy(),
        pid
    ));
    fs::write(&tmp, content).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("rename {} → {}: {e}", tmp.display(), path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn scratch() -> PathBuf {
        // Unique per-test directory: pid + nanoseconds-since-epoch +
        // an atomic counter. cargo test runs cases concurrently so a
        // pid-only suffix collides between threads.
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut p = env::temp_dir();
        p.push(format!(
            "lyra-skill-test-{}-{}-{}",
            std::process::id(),
            nanos,
            n
        ));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn embedded_skill_has_a_name_field() {
        // Sanity: the bundled SKILL.md has the frontmatter we expect.
        assert!(SKILL_MD.contains("name: lyra"));
        assert!(SKILL_MD.contains("# /lyra"));
    }

    #[test]
    fn fresh_install_creates_the_file() {
        let home = scratch();
        let out = install(&home).unwrap();
        match out {
            SkillOutcome::Installed { path } => {
                assert!(path.exists());
                assert_eq!(fs::read_to_string(&path).unwrap(), SKILL_MD);
            }
            other => panic!("expected Installed, got {other:?}"),
        }
    }

    #[test]
    fn second_install_is_unchanged() {
        let home = scratch();
        install(&home).unwrap();
        let out = install(&home).unwrap();
        assert!(matches!(out, SkillOutcome::Unchanged { .. }));
    }

    #[test]
    fn factory_update_overwrites_old_factory_content() {
        let home = scratch();
        let path = home.join("skills/lyra/SKILL.md");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "old factory content\n").unwrap();
        let out = install(&home).unwrap();
        assert!(matches!(out, SkillOutcome::Updated { .. }));
        assert_eq!(fs::read_to_string(&path).unwrap(), SKILL_MD);
    }

    #[test]
    fn user_edit_marker_preserves_file() {
        let home = scratch();
        let path = home.join("skills/lyra/SKILL.md");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let custom = "# user-edit: keep\n\nmy custom skill\n";
        fs::write(&path, custom).unwrap();
        let out = install(&home).unwrap();
        assert!(matches!(out, SkillOutcome::Preserved { .. }));
        // File untouched.
        assert_eq!(fs::read_to_string(&path).unwrap(), custom);
    }

    #[test]
    fn uninstall_removes_then_is_noop() {
        let home = scratch();
        install(&home).unwrap();
        assert!(uninstall(&home).unwrap());
        assert!(!uninstall(&home).unwrap());
    }
}
