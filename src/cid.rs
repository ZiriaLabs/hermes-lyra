//! Content-Addressed Identifiers (CIDv1) — the v0.3+ addressing primitive.
//!
//! A `Cid` is the multihash-wrapped BLAKE3-256 digest of a byte sequence,
//! encoded per the [multiformats](https://github.com/multiformats) specs.
//! The same bytes always produce the same CID across runtime versions,
//! across implementations, forever.
//!
//! # Design
//!
//! * Closed enums (`Codec`, `HashCode`). Adding a codec is a deliberate
//!   protocol-level change, not a runtime decision. There is no
//!   `Other(u64)` escape hatch.
//! * Private fields on `Cid`. The only constructors are
//!   [`Cid::from_canonical_input`], [`Cid::from_raw_blob`],
//!   [`Cid::parse`], and [`Cid::from_binary`]. A `Cid` value seen
//!   anywhere in the crate is structurally well-formed by construction.
//! * Fixed-width digest (`[u8; 32]`, not `Vec<u8>`). The 32-byte length
//!   is in the type, not runtime-checked.
//! * Two codecs only: `Json` (0x0200) for receipt / descriptor / registry
//!   content; `Raw` (0x55) for opaque blobs. `dag-json` is deliberately
//!   omitted — IPLD link semantics are out of scope for v0.3.
//! * One hash code: BLAKE3-256 (0x1e). Adding sha2 / sha3 is a protocol
//!   bump, by design.
//!
//! # Invariants asserted by the type
//!
//! 1. `digest` is exactly 32 bytes (compile-time).
//! 2. `codec` is one of the two declared variants (no unknown values).
//! 3. `hash_code` is BLAKE3-256 (only-known-good).
//! 4. CIDv0 (`Qm…` base58btc sha256) is rejected — we are v1-only.
//!
//! # Wire formats
//!
//! Binary: `0x01 || codec_varint || hash_code || 0x20 || digest[32]`.
//! Raw codec produces 36 bytes total; Json codec produces 37 bytes.
//!
//! String: multibase prefix `b` + base32-lower of the binary, no padding.
//! Raw codec → 59 chars; Json codec → 61 chars.

use crate::LYRA_PROTOCOL_ID_PREFIX;

// ----- Closed enums ---------------------------------------------------------

/// Multicodec codec (closed at the protocol level).
///
/// Adding a variant is a v0.x protocol bump, not a runtime decision.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Hash)]
pub enum Codec {
    /// Opaque bytes. Used for full-file blob CIDs (e.g., a SKILL.md
    /// addressed as a flat file).
    Raw,
    /// Canonical JSON. Used for descriptor / receipt / registry CIDs
    /// where the content is structured JSON parsed by hermes-lyra.
    Json,
}

impl Codec {
    /// Returns the multicodec varint bytes (1 byte for Raw, 2 bytes for Json).
    pub(crate) fn as_varint(self) -> &'static [u8] {
        match self {
            // 0x55 — single-byte varint (high bit clear)
            Codec::Raw => &[0x55],
            // 0x0200 in unsigned-varint encoding:
            //   value = 0b 0000_0010 0000_0000 = 0x200 = 512
            //   7-bit groups LSB-first: 0000000 (low 7 bits), 0000100 (next 7 bits)
            //   first byte 0x80 | 0b0000000 = 0x80 (continuation set)
            //   second byte 0x00 | 0b0000100 = 0x04
            Codec::Json => &[0x80, 0x04],
        }
    }

    /// Decode a leading varint into a Codec, returning `(codec, bytes_consumed)`.
    pub(crate) fn from_varint(bytes: &[u8]) -> Result<(Codec, usize), CidParseError> {
        if bytes.is_empty() {
            return Err(CidParseError::Truncated);
        }
        if bytes[0] == 0x55 {
            return Ok((Codec::Raw, 1));
        }
        if bytes.len() >= 2 && bytes[0] == 0x80 && bytes[1] == 0x04 {
            return Ok((Codec::Json, 2));
        }
        // We could decode a generic varint and report the value, but we
        // want a CLOSED parser: anything not in the known set is rejected
        // immediately. This matches the type-level closure of the enum.
        Err(CidParseError::UnsupportedCodec(bytes[0]))
    }
}

/// Multihash hash code (closed at the protocol level).
#[derive(Copy, Clone, Eq, PartialEq, Debug, Hash)]
pub enum HashCode {
    /// BLAKE3-256, multihash code 0x1e. The only hash function emitted
    /// or accepted by v0.3 implementations.
    Blake3_256,
}

impl HashCode {
    pub(crate) fn as_byte(self) -> u8 {
        match self {
            HashCode::Blake3_256 => 0x1e,
        }
    }

    pub(crate) fn from_byte(b: u8) -> Result<HashCode, CidParseError> {
        match b {
            0x1e => Ok(HashCode::Blake3_256),
            other => Err(CidParseError::UnsupportedHashCode(other)),
        }
    }
}

// ----- Cid ------------------------------------------------------------------

/// A CIDv1 over a 32-byte BLAKE3-256 digest.
///
/// Construct via [`from_canonical_input`], [`from_raw_blob`], [`parse`],
/// or [`from_binary`]. A `Cid` value is always structurally valid.
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct Cid {
    codec: Codec,
    hash_code: HashCode,
    digest: [u8; 32],
}

impl Cid {
    /// Build a CID from a precomputed 32-byte BLAKE3 digest.
    ///
    /// **Crate-private.** Used by `cli_api::score` and `cli_api::verify`
    /// where the digest provenance is known: it was just computed by
    /// `computations::run`, which itself routes through
    /// [`Cid::from_canonical_input`]. The sealing invariant is preserved
    /// at the public API boundary — this is an internal pipe, not a
    /// general escape hatch.
    pub(crate) fn from_blake3_digest_unchecked(digest: [u8; 32], codec: Codec) -> Cid {
        Cid {
            codec,
            hash_code: HashCode::Blake3_256,
            digest,
        }
    }

    /// Build a CID over the **framed** canonical input bytes.
    ///
    /// The actual hash input is `protocol_prefix || 0x00 || label || 0x00 || bytes`
    /// where `protocol_prefix` is [`crate::LYRA_PROTOCOL_ID_PREFIX`].
    ///
    /// This is the constructor every protocol computation should use.
    /// The framing provides domain separation between computations (so
    /// `compose_interfaces` over `b"COMPATIBLE"` does not collide with
    /// any other computation that happens to hash the same payload).
    pub fn from_canonical_input(label: &str, bytes: &[u8]) -> Cid {
        let mut h = blake3::Hasher::new();
        h.update(LYRA_PROTOCOL_ID_PREFIX.as_bytes());
        h.update(&[0x00]);
        h.update(label.as_bytes());
        h.update(&[0x00]);
        h.update(bytes);
        Cid {
            codec: Codec::Json,
            hash_code: HashCode::Blake3_256,
            digest: *h.finalize().as_bytes(),
        }
    }

    /// Build a CID over raw blob bytes (no protocol framing).
    ///
    /// Use this for addressing whole files (e.g., a SKILL.md byte blob
    /// as it sits on disk). Not used by any protocol computation.
    pub fn from_raw_blob(bytes: &[u8]) -> Cid {
        Cid {
            codec: Codec::Raw,
            hash_code: HashCode::Blake3_256,
            digest: *blake3::hash(bytes).as_bytes(),
        }
    }

    /// Returns the codec of this CID.
    pub fn codec(&self) -> Codec {
        self.codec
    }

    /// Returns the multihash code of this CID.
    pub fn hash_code(&self) -> HashCode {
        self.hash_code
    }

    /// Returns the 32-byte digest.
    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }

    /// Serialise to canonical binary form.
    ///
    /// Layout: `0x01 || codec_varint || hash_code || 0x20 || digest`.
    ///
    /// Returns 36 bytes for Raw codec, 37 bytes for Json codec.
    pub fn to_binary(&self) -> Vec<u8> {
        let codec_bytes = self.codec.as_varint();
        let mut out = Vec::with_capacity(1 + codec_bytes.len() + 1 + 1 + 32);
        out.push(0x01); // CIDv1
        out.extend_from_slice(codec_bytes);
        out.push(self.hash_code.as_byte());
        out.push(0x20); // digest length = 32
        out.extend_from_slice(&self.digest);
        out
    }

    /// Parse from canonical binary form. Returns an error on any deviation
    /// — wrong version, unknown codec, unknown hash code, wrong digest
    /// length, trailing bytes, or truncation.
    pub fn from_binary(bytes: &[u8]) -> Result<Cid, CidParseError> {
        if bytes.is_empty() {
            return Err(CidParseError::Truncated);
        }
        if bytes[0] != 0x01 {
            return Err(CidParseError::UnsupportedVersion(bytes[0]));
        }
        let after_version = &bytes[1..];
        let (codec, codec_len) = Codec::from_varint(after_version)?;
        let after_codec = &after_version[codec_len..];
        if after_codec.len() < 2 {
            return Err(CidParseError::Truncated);
        }
        let hash_code = HashCode::from_byte(after_codec[0])?;
        let digest_len = after_codec[1] as usize;
        if digest_len != 32 {
            return Err(CidParseError::WrongDigestLength(digest_len));
        }
        let digest_bytes = &after_codec[2..];
        if digest_bytes.len() < 32 {
            return Err(CidParseError::Truncated);
        }
        if digest_bytes.len() > 32 {
            return Err(CidParseError::TrailingBytes);
        }
        let mut digest = [0u8; 32];
        digest.copy_from_slice(&digest_bytes[..32]);
        Ok(Cid {
            codec,
            hash_code,
            digest,
        })
    }

    /// Serialise to canonical string form: multibase `b` (base32 lower,
    /// no padding) of the binary representation.
    #[allow(clippy::inherent_to_string_shadow_display)]
    pub fn to_string(&self) -> String {
        let mut out = String::with_capacity(64);
        out.push('b');
        base32_lower_no_pad(&self.to_binary(), &mut out);
        out
    }

    /// Parse from canonical string form. Rejects:
    /// * CIDv0 (`Qm…` base58btc) — wrong multibase prefix.
    /// * Other multibase prefixes (`f` hex, `z` base58btc, etc.).
    /// * Any decoded payload that fails [`Self::from_binary`].
    pub fn parse(s: &str) -> Result<Cid, CidParseError> {
        let mut chars = s.chars();
        let prefix = chars.next().ok_or(CidParseError::Truncated)?;
        if prefix != 'b' {
            return Err(CidParseError::UnsupportedMultibase(prefix));
        }
        let payload = &s[1..];
        let bytes = base32_lower_decode(payload)?;
        Cid::from_binary(&bytes)
    }
}

impl core::fmt::Display for Cid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.to_string())
    }
}

impl core::fmt::Debug for Cid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Debug shows the canonical string so test output / logs are
        // immediately copy-pasteable. The full struct layout is internal.
        write!(f, "Cid({})", self.to_string())
    }
}

// ----- Errors ---------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CidParseError {
    /// Input had no bytes / no multibase prefix / digest was cut off.
    Truncated,
    /// Extra bytes after the expected end of the CID.
    TrailingBytes,
    /// Multibase prefix was something other than `'b'` (base32 lower).
    UnsupportedMultibase(char),
    /// CID version byte was not `0x01`.
    UnsupportedVersion(u8),
    /// Codec varint did not match `Codec::Raw` or `Codec::Json`.
    UnsupportedCodec(u8),
    /// Multihash code byte was not `0x1e` (BLAKE3-256).
    UnsupportedHashCode(u8),
    /// Multihash digest length was not 32.
    WrongDigestLength(usize),
    /// Multibase base32 alphabet rejected a character.
    InvalidBase32(char),
}

impl core::fmt::Display for CidParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CidParseError::Truncated => write!(f, "CID truncated"),
            CidParseError::TrailingBytes => write!(f, "CID has trailing bytes"),
            CidParseError::UnsupportedMultibase(c) => {
                write!(f, "unsupported multibase prefix {c:?} (expected 'b')")
            }
            CidParseError::UnsupportedVersion(v) => {
                write!(f, "unsupported CID version 0x{v:02x} (expected 0x01)")
            }
            CidParseError::UnsupportedCodec(b) => {
                write!(f, "unsupported codec varint leading byte 0x{b:02x}")
            }
            CidParseError::UnsupportedHashCode(b) => {
                write!(f, "unsupported multihash code 0x{b:02x} (expected 0x1e blake3-256)")
            }
            CidParseError::WrongDigestLength(n) => {
                write!(f, "digest length {n} (expected 32)")
            }
            CidParseError::InvalidBase32(c) => write!(f, "invalid base32 character {c:?}"),
        }
    }
}

impl std::error::Error for CidParseError {}

// ----- base32 lower (RFC4648, no padding, lowercase) -----------------------

/// RFC4648 base32 alphabet, lowercase, no padding. Multibase prefix `b`.
const B32_ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

fn base32_lower_no_pad(input: &[u8], out: &mut String) {
    let mut bit_buf: u32 = 0;
    let mut bits: u32 = 0;
    for &byte in input {
        bit_buf = (bit_buf << 8) | byte as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            let idx = ((bit_buf >> bits) & 0x1f) as usize;
            out.push(B32_ALPHABET[idx] as char);
        }
    }
    if bits > 0 {
        let idx = ((bit_buf << (5 - bits)) & 0x1f) as usize;
        out.push(B32_ALPHABET[idx] as char);
    }
}

fn base32_lower_decode(s: &str) -> Result<Vec<u8>, CidParseError> {
    let mut bit_buf: u32 = 0;
    let mut bits: u32 = 0;
    let mut out = Vec::with_capacity(s.len() * 5 / 8 + 1);
    for c in s.chars() {
        let v = match c {
            'a'..='z' => (c as u8) - b'a',
            '2'..='7' => 26 + ((c as u8) - b'2'),
            _ => return Err(CidParseError::InvalidBase32(c)),
        };
        bit_buf = (bit_buf << 5) | v as u32;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((bit_buf >> bits) & 0xff) as u8);
        }
    }
    // Any leftover bits must be zero padding (RFC4648 no-pad invariant).
    if bits > 0 {
        let leftover_mask = (1u32 << bits) - 1;
        if bit_buf & leftover_mask != 0 {
            return Err(CidParseError::InvalidBase32('?'));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base32_roundtrip_random_lengths() {
        for len in 0..40 {
            let data: Vec<u8> = (0..len as u8).collect();
            let mut s = String::new();
            base32_lower_no_pad(&data, &mut s);
            let back = base32_lower_decode(&s).unwrap();
            assert_eq!(back, data, "len={len}");
        }
    }

    #[test]
    fn json_codec_varint_encodes_to_0x80_0x04() {
        assert_eq!(Codec::Json.as_varint(), &[0x80u8, 0x04u8]);
    }

    #[test]
    fn raw_codec_varint_encodes_to_0x55() {
        assert_eq!(Codec::Raw.as_varint(), &[0x55u8]);
    }
}
