//! Minimal SemVer 2.0.0 parser, dependency-free.
//!
//! Replaces the `semver` crate. We use **only** the `(major, minor, patch)`
//! triple for refinement ordering — prerelease and build-metadata are
//! parsed (so we can validate them at build time) but deliberately
//! ignored for ordering, per R2's anti-pollution policy.
//!
//! Grammar (subset of [SemVer 2.0.0]):
//!
//! ```text
//! version      := major "." minor "." patch [ "-" prerelease ] [ "+" build ]
//! major        := numeric
//! minor        := numeric
//! patch        := numeric
//! numeric      := "0" | [1-9][0-9]*           ; no leading zeros
//! prerelease   := ident ( "." ident )*
//! build        := ident ( "." ident )*
//! ident        := [0-9A-Za-z-]+               ; non-empty
//! ```
//!
//! [SemVer 2.0.0]: https://semver.org/spec/v2.0.0.html

/// Parsed SemVer triple. We retain prerelease and build only to validate
/// their grammar at parse time; ordering uses `(major, minor, patch)` only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

/// Parse a SemVer string. Returns the numeric triple on success.
pub fn parse(s: &str) -> Result<Version, String> {
    if s.is_empty() {
        return Err("empty version string".into());
    }

    // Split off build metadata at the FIRST '+' (build cannot contain '+').
    let (core_and_pre, build) = match s.find('+') {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    if let Some(b) = build {
        validate_dotted_idents(b, "build metadata")?;
    }

    // Split off prerelease at the FIRST '-'. The '-' must come AFTER the
    // patch number, so we scan for the first '-' that follows the third '.'.
    let (core, pre) = split_prerelease(core_and_pre)?;
    if let Some(p) = pre {
        validate_dotted_idents(p, "prerelease")?;
    }

    // Core: must be exactly major.minor.patch, each numeric, no leading zero.
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 {
        return Err(format!(
            "expected MAJOR.MINOR.PATCH, got {} component(s)",
            parts.len()
        ));
    }
    let major = parse_numeric(parts[0], "major")?;
    let minor = parse_numeric(parts[1], "minor")?;
    let patch = parse_numeric(parts[2], "patch")?;

    Ok(Version { major, minor, patch })
}

/// Find the first '-' that separates the core triple from the prerelease.
/// Since none of `[0-9]` digits are '-', the first '-' after the second
/// '.' is the separator. Returns `(core, prerelease_or_none)`.
fn split_prerelease(s: &str) -> Result<(&str, Option<&str>), String> {
    let bytes = s.as_bytes();
    let mut dot_count = 0u8;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'.' {
            dot_count += 1;
            continue;
        }
        if b == b'-' && dot_count >= 2 {
            // The '-' must be the FIRST non-numeric byte in the patch slot.
            // (i.e. it must immediately follow a digit.)
            if i == 0 || !bytes[i - 1].is_ascii_digit() {
                return Err(format!("unexpected '-' at byte {i}"));
            }
            return Ok((&s[..i], Some(&s[i + 1..])));
        }
        // Any other non-digit, non-dot byte in the core is invalid.
        if !b.is_ascii_digit() {
            return Err(format!("invalid byte {:?} in version core at {i}", b as char));
        }
    }
    Ok((s, None))
}

fn parse_numeric(s: &str, what: &str) -> Result<u64, String> {
    if s.is_empty() {
        return Err(format!("{what} is empty"));
    }
    let bytes = s.as_bytes();
    if bytes.len() > 1 && bytes[0] == b'0' {
        return Err(format!("{what} has leading zero: {s:?}"));
    }
    for (i, &b) in bytes.iter().enumerate() {
        if !b.is_ascii_digit() {
            return Err(format!(
                "{what}: non-digit {:?} at byte {i}",
                b as char
            ));
        }
    }
    s.parse::<u64>().map_err(|e| format!("{what}: {e}"))
}

fn validate_dotted_idents(s: &str, what: &str) -> Result<(), String> {
    if s.is_empty() {
        return Err(format!("{what} empty"));
    }
    for (i, part) in s.split('.').enumerate() {
        if part.is_empty() {
            return Err(format!("{what} component {i} is empty"));
        }
        for (j, b) in part.bytes().enumerate() {
            if !(b.is_ascii_alphanumeric() || b == b'-') {
                return Err(format!(
                    "{what} component {i} byte {j}: invalid {:?}",
                    b as char
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_triple() {
        let v = parse("1.2.3").unwrap();
        assert_eq!(v, Version { major: 1, minor: 2, patch: 3 });
    }

    #[test]
    fn zeros() {
        assert_eq!(parse("0.0.0").unwrap(), Version { major: 0, minor: 0, patch: 0 });
    }

    #[test]
    fn rejects_leading_zero() {
        assert!(parse("01.2.3").is_err());
        assert!(parse("1.02.3").is_err());
        assert!(parse("1.2.03").is_err());
    }

    #[test]
    fn rejects_non_numeric() {
        assert!(parse("v1.2.3").is_err());
        assert!(parse("1.2.three").is_err());
    }

    #[test]
    fn rejects_wrong_arity() {
        assert!(parse("1.2").is_err());
        assert!(parse("1.2.3.4").is_err());
        assert!(parse("").is_err());
    }

    #[test]
    fn accepts_prerelease_but_ignores_for_triple() {
        let v = parse("1.2.3-alpha.1").unwrap();
        assert_eq!(v, Version { major: 1, minor: 2, patch: 3 });
    }

    #[test]
    fn accepts_build_metadata_but_ignores_for_triple() {
        let v = parse("1.2.3+build.5").unwrap();
        assert_eq!(v, Version { major: 1, minor: 2, patch: 3 });
    }

    #[test]
    fn accepts_pre_and_build_together() {
        let v = parse("1.2.3-rc.1+sha.abc").unwrap();
        assert_eq!(v, Version { major: 1, minor: 2, patch: 3 });
    }

    #[test]
    fn rejects_empty_prerelease() {
        assert!(parse("1.2.3-").is_err());
    }

    #[test]
    fn rejects_empty_build() {
        assert!(parse("1.2.3+").is_err());
    }

    #[test]
    fn rejects_bad_prerelease_char() {
        assert!(parse("1.2.3-alpha_1").is_err());
    }

    #[test]
    fn rejects_dot_in_prerelease_component() {
        assert!(parse("1.2.3-..").is_err());
    }
}
