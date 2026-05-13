//! Build script: extract `uor-foundation`'s version from the workspace
//! `Cargo.lock` and expose it as a compile-time env var. The runtime
//! ident must agree byte-for-byte with the substrate that actually
//! got linked — a hardcoded string drifts the moment someone bumps the
//! dependency without remembering to update the constant (audit #4).
//!
//! Output: `cargo:rustc-env=UOR_FOUNDATION_VERSION=<version>`, plus
//! a rerun directive so the build script runs again when `Cargo.lock`
//! changes.

use std::fs;
use std::path::PathBuf;

fn main() {
    let cargo_lock = locate_cargo_lock();
    println!("cargo:rerun-if-changed={}", cargo_lock.display());

    let contents = fs::read_to_string(&cargo_lock).unwrap_or_else(|e| {
        panic!("build.rs: cannot read {}: {e}", cargo_lock.display());
    });

    let version = extract_package_version(&contents, "uor-foundation").unwrap_or_else(|| {
        panic!(
            "build.rs: no [[package]] entry for uor-foundation in {}",
            cargo_lock.display()
        );
    });

    println!("cargo:rustc-env=UOR_FOUNDATION_VERSION={version}");
}

/// Find the `Cargo.lock`. The build script runs from the crate dir;
/// walk up until we find `Cargo.lock` (handles workspace layouts too).
fn locate_cargo_lock() -> PathBuf {
    let mut dir: PathBuf = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR is set by cargo")
        .into();
    loop {
        let candidate = dir.join("Cargo.lock");
        if candidate.exists() {
            return candidate;
        }
        if !dir.pop() {
            panic!("build.rs: no Cargo.lock found from CARGO_MANIFEST_DIR upward");
        }
    }
}

/// Parse `Cargo.lock` looking for the `[[package]]` entry whose
/// `name = "<wanted>"` and return its `version = "..."` string.
///
/// `Cargo.lock` is TOML, but TOML pulls a parser dependency we
/// deliberately avoid. The format is line-oriented and predictable for
/// `[[package]]` blocks; a small hand-walk is sufficient.
fn extract_package_version(contents: &str, wanted: &str) -> Option<String> {
    let mut in_pkg = false;
    let mut current_name: Option<&str> = None;
    let target_name_line = format!("name = \"{wanted}\"");
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed == "[[package]]" {
            in_pkg = true;
            current_name = None;
            continue;
        }
        if !in_pkg {
            continue;
        }
        if trimmed == target_name_line {
            current_name = Some(wanted);
            continue;
        }
        if current_name == Some(wanted) {
            if let Some(rest) = trimmed.strip_prefix("version = \"") {
                if let Some(end) = rest.find('"') {
                    return Some(rest[..end].to_string());
                }
            }
            // Hit a non-version line before version → skip block.
            if trimmed.starts_with("name = \"") || trimmed == "[[package]]" {
                current_name = None;
            }
        }
    }
    None
}
