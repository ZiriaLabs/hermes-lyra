//! Receipt: a JSON envelope binding computation inputs, outputs, and a
//! content-addressed proof of execution.
//!
//! The on-disk format is a single-line JSON object with exactly four
//! string fields, in alphabetical key order:
//!
//! ```json
//! {"computation_id":"...","input":"...","output_cid":"bafy...","runtime":"..."}
//! ```
//!
//! `output_cid` is the v0.3+ content anchor — a multibase string-form
//! CIDv1 over the canonical input bytes for `computation_id`. The bytes
//! actually hashed are
//!
//! ```text
//! LYRA_PROTOCOL_ID_PREFIX || 0x00 || computation_id || 0x00 || canonical_input_bytes
//! ```
//!
//! so two implementations that produce the same canonical inputs produce
//! byte-identical `output_cid` values — the addressing is independent of
//! crate version or substrate version.
//!
//! `runtime` identifies the implementation + substrate that produced
//! this receipt (e.g. `"hermes-lyra/0.3.0+uor-foundation/0.4.2"`). It is
//! preserved on the envelope as a compatibility *gate* (a verifier
//! rejects unknown runtime idents with `unsupported_protocol`) but is
//! *not* folded into the hash itself — pure content addressing.
//!
//! **Earlier versions (v0.2 and below)** carried `output_hash` instead
//! (`BLAKE3(runtime || 0x00 || label || 0x00 || bytes)` hex-encoded).
//! That field is no longer accepted; a v0.3 parser rejects any receipt
//! containing `output_hash` so a partial migration cannot silently slip
//! through. v0.2 receipts must be re-bound with `lyra bind`.
//!
//! Strings are escaped with the standard JSON escape set (`\"`, `\\`,
//! `\n`, `\r`, `\t`); no `\uXXXX` escapes are emitted.
//!
//! The parser is hand-rolled (no serde dependency). It accepts only the
//! receipt shape and rejects anything else with a diagnostic.

use std::io;

/// A computation receipt.
#[derive(Debug, Clone)]
pub struct Receipt {
    pub computation_id: String,
    pub input: String,
    /// Canonical string-form CIDv1 over the framed canonical input.
    /// See [`crate::cid::Cid`] for the layout.
    pub output_cid: String,
    /// Identifier of the implementation + substrate that produced this
    /// receipt. Gated at verify time. See [`crate::LYRA_RUNTIME_IDENT`].
    pub runtime: String,
}

impl Receipt {
    /// Encode as a single-line JSON object.
    pub fn to_json(&self) -> String {
        let mut out = String::with_capacity(
            self.computation_id.len()
                + self.input.len()
                + self.output_cid.len()
                + self.runtime.len()
                + 64,
        );
        out.push('{');
        out.push_str("\"computation_id\":");
        write_json_string(&mut out, &self.computation_id);
        out.push(',');
        out.push_str("\"input\":");
        write_json_string(&mut out, &self.input);
        out.push(',');
        out.push_str("\"output_cid\":");
        write_json_string(&mut out, &self.output_cid);
        out.push(',');
        out.push_str("\"runtime\":");
        write_json_string(&mut out, &self.runtime);
        out.push('}');
        out
    }

    /// Parse the receipt from its on-disk JSON form.
    ///
    /// Accepts exactly v0.3 receipts. v0.2 receipts (`output_hash` field)
    /// are rejected with a diagnostic pointing at `lyra bind` so a
    /// caller cannot accidentally treat a v0.2 receipt as v0.3.
    pub fn from_json(s: &str) -> Result<Self, String> {
        let mut p = Parser::new(s);
        p.skip_ws();
        p.expect_char('{')?;

        p.skip_ws();
        let computation_id = p.expect_field("computation_id")?;
        p.skip_ws();
        p.expect_char(',')?;
        p.skip_ws();
        let input = p.expect_field("input")?;
        p.skip_ws();
        p.expect_char(',')?;
        p.skip_ws();
        let output_cid = p.expect_field_v03_output_cid()?;
        p.skip_ws();
        p.expect_char(',')?;
        p.skip_ws();
        let runtime = p.expect_field("runtime")?;
        p.skip_ws();
        p.expect_char('}')?;

        Ok(Self {
            computation_id,
            input,
            output_cid,
            runtime,
        })
    }

    pub fn write_to_file(&self, path: &str) -> io::Result<()> {
        std::fs::write(path, self.to_json().as_bytes())
    }

    pub fn read_from_file(path: &str) -> io::Result<Self> {
        let s = std::fs::read_to_string(path)?;
        Self::from_json(&s).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
}

// -- writer --

fn write_json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                // Other control characters as \u00XX.
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

// -- parser --

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            bytes: s.as_bytes(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.pos += 1;
        Some(b)
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if matches!(b, b' ' | b'\t' | b'\n' | b'\r') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn expect_char(&mut self, c: char) -> Result<(), String> {
        match self.bump() {
            Some(b) if b as char == c => Ok(()),
            Some(b) => Err(format!(
                "expected '{}' at byte {}, got '{}'",
                c,
                self.pos.saturating_sub(1),
                b as char
            )),
            None => Err(format!(
                "expected '{}' at byte {}, got end-of-input",
                c, self.pos
            )),
        }
    }

    /// Parse a JSON string literal: `"..."` with standard escapes.
    fn parse_string(&mut self) -> Result<String, String> {
        self.expect_char('"')?;
        let mut out = String::new();
        loop {
            let b = self
                .bump()
                .ok_or_else(|| "unterminated string".to_string())?;
            match b {
                b'"' => return Ok(out),
                b'\\' => {
                    let esc = self
                        .bump()
                        .ok_or_else(|| "trailing backslash".to_string())?;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000C}'),
                        b'u' => {
                            // \uXXXX -> single 16-bit code unit. Surrogate
                            // pairs are out of scope for receipt fields.
                            let mut cp: u32 = 0;
                            for _ in 0..4 {
                                let h = self
                                    .bump()
                                    .ok_or_else(|| "truncated \\u escape".to_string())?;
                                let nib = match h {
                                    b'0'..=b'9' => (h - b'0') as u32,
                                    b'a'..=b'f' => (h - b'a' + 10) as u32,
                                    b'A'..=b'F' => (h - b'A' + 10) as u32,
                                    _ => return Err(format!("bad hex digit in \\u: {}", h as char)),
                                };
                                cp = (cp << 4) | nib;
                            }
                            let c = char::from_u32(cp)
                                .ok_or_else(|| format!("invalid code point U+{cp:04X}"))?;
                            out.push(c);
                        }
                        other => return Err(format!("unknown escape \\{}", other as char)),
                    }
                }
                // Multi-byte UTF-8: copy continuation bytes verbatim.
                b if b >= 0x80 => {
                    // Walk back one byte and copy the full UTF-8 char.
                    self.pos -= 1;
                    let rest = &self.bytes[self.pos..];
                    let s = std::str::from_utf8(rest)
                        .map_err(|e| format!("invalid utf-8 inside string: {e}"))?;
                    let ch = s
                        .chars()
                        .next()
                        .ok_or_else(|| "utf-8 boundary".to_string())?;
                    out.push(ch);
                    self.pos += ch.len_utf8();
                }
                b if b < 0x20 => {
                    return Err(format!("control byte 0x{:02x} in string literal", b))
                }
                b => out.push(b as char),
            }
        }
    }

    /// Parse `"<key>":"<value>"` and require the key matches.
    fn expect_field(&mut self, key: &str) -> Result<String, String> {
        let k = self.parse_string()?;
        if k != key {
            return Err(format!("expected key {:?}, got {:?}", key, k));
        }
        self.skip_ws();
        self.expect_char(':')?;
        self.skip_ws();
        self.parse_string()
    }

    /// v0.3-specific: parse the third field and require it is `output_cid`.
    /// If the receipt is a v0.2 form carrying `output_hash`, emit a
    /// diagnostic pointing at `lyra bind` for migration.
    fn expect_field_v03_output_cid(&mut self) -> Result<String, String> {
        let k = self.parse_string()?;
        if k == "output_hash" {
            return Err(
                "this receipt is v0.2 (carries `output_hash`); v0.3 receipts use \
                 `output_cid`. Re-bind with `lyra bind <SKILL.md> <descriptor.json>`."
                    .into(),
            );
        }
        if k != "output_cid" {
            return Err(format!("expected key \"output_cid\", got {:?}", k));
        }
        self.skip_ws();
        self.expect_char(':')?;
        self.skip_ws();
        self.parse_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt(input: &str) -> Receipt {
        Receipt {
            computation_id: "test".into(),
            input: input.into(),
            output_cid: "bafkreitestplaceholder0000000000000000000000000000000000000000".into(),
            runtime: "hermes-lyra/0.3.0+uor-foundation/0.4.2".into(),
        }
    }

    fn assert_roundtrips(r: &Receipt) {
        let json = r.to_json();
        let r2 = Receipt::from_json(&json).expect("parse");
        assert_eq!(r.computation_id, r2.computation_id);
        assert_eq!(r.input, r2.input);
        assert_eq!(r.output_cid, r2.output_cid);
        assert_eq!(r.runtime, r2.runtime);
    }

    #[test]
    fn simple_input_roundtrips() {
        assert_roundtrips(&rt("hello world"));
    }

    #[test]
    fn json_object_input_roundtrips() {
        assert_roundtrips(&rt(
            r#"{"producer":{"output_shape":{"type":"string","max_bytes":1024}}}"#,
        ));
    }

    #[test]
    fn json_array_input_roundtrips() {
        assert_roundtrips(&rt(r#"[{"a":1},{"b":2}]"#));
    }

    #[test]
    fn embedded_quotes_roundtrip() {
        assert_roundtrips(&rt(r#"he said "hi""#));
    }

    #[test]
    fn embedded_backslash_roundtrips() {
        assert_roundtrips(&rt(r"path\to\file"));
    }

    #[test]
    fn newlines_roundtrip() {
        assert_roundtrips(&rt("line1\nline2\rline3\tend"));
    }

    #[test]
    fn unicode_roundtrips() {
        assert_roundtrips(&rt("héllo 世界 🦀"));
    }

    #[test]
    fn rejects_missing_field() {
        let bad = r#"{"computation_id":"x","input":"y","output_cid":"z"}"#;
        assert!(Receipt::from_json(bad).is_err());
    }

    #[test]
    fn rejects_wrong_field_order() {
        let bad = r#"{"input":"y","computation_id":"x","output_cid":"z","runtime":"w"}"#;
        assert!(Receipt::from_json(bad).is_err());
    }

    #[test]
    fn rejects_unterminated_string() {
        let bad = r#"{"computation_id":"x"#;
        assert!(Receipt::from_json(bad).is_err());
    }

    #[test]
    fn rejects_v02_receipts_with_explicit_message() {
        // A v0.2 receipt body — `output_hash` instead of `output_cid`.
        let v02 = r#"{"computation_id":"skill_interface_hash","input":"{}","output_hash":"deadbeef","runtime":"hermes-lyra/0.2.0+uor-foundation/0.4.2"}"#;
        let err = Receipt::from_json(v02).expect_err("v0.2 receipts must be rejected");
        assert!(
            err.contains("v0.2") && err.contains("lyra bind"),
            "rejection must point users at lyra bind; got: {err}"
        );
    }
}
