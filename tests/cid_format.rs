//! CID format pins.
//!
//! These tests pin the v0.3 content-addressing layer at the byte level.
//! They are written BEFORE `src/cid.rs` exists so the type/format design
//! has to satisfy known-good vectors rather than the other way round.
//!
//! Vector source: the multiformats specs at github.com/multiformats:
//!   - multibase: base32 lower with prefix 'b', no padding
//!   - multihash: <hash_code varint> <length varint> <digest bytes>
//!   - CIDv1:     0x01 <codec varint> <multihash>
//!
//! BLAKE3-256 multihash code is 0x1e (draft status in the multicodec table
//! but stable in the ecosystem). 32-byte digest length encodes as 0x20.

use lyra_ref::cid::{Cid, CidParseError, Codec, HashCode};

// ----- known-good blake3 digests --------------------------------------------

/// BLAKE3-256 of the literal byte string b"abc". Spot-checked against
/// the BLAKE3 reference vectors at github.com/BLAKE3-team/BLAKE3/blob/master/test_vectors/test_vectors.json
const BLAKE3_OF_ABC: [u8; 32] = [
    0x64, 0x37, 0xb3, 0xac, 0x38, 0x46, 0x51, 0x33,
    0xff, 0xb6, 0x3b, 0x75, 0x27, 0x3a, 0x8d, 0xb5,
    0x48, 0xc5, 0x58, 0x46, 0x5d, 0x79, 0xdb, 0x03,
    0xfd, 0x35, 0x9c, 0x6c, 0xd5, 0xbd, 0x9d, 0x85,
];

/// BLAKE3-256 of the empty byte string.
const BLAKE3_OF_EMPTY: [u8; 32] = [
    0xaf, 0x13, 0x49, 0xb9, 0xf5, 0xf9, 0xa1, 0xa6,
    0xa0, 0x40, 0x4d, 0xea, 0x36, 0xdc, 0xc9, 0x49,
    0x9b, 0xcb, 0x25, 0xc9, 0xad, 0xc1, 0x12, 0xb7,
    0xcc, 0x9a, 0x93, 0xca, 0xe4, 0x1f, 0x32, 0x62,
];

// ----- binary layout --------------------------------------------------------

#[test]
fn binary_layout_is_exactly_36_bytes_for_json_codec() {
    // json codec (0x0200) is a TWO-byte varint: 0x80 0x04
    //   0x0200 = 0b0000_0010_0000_0000
    //   varint LSB-first 7-bit groups: 0b0000000 (low) | 0b0000100 (high)
    //   first byte: 0b1_0000000 = 0x80 (continuation set)
    //   second byte: 0b0_0000100 = 0x04
    // Total: 1 (version) + 2 (codec) + 1 (hash code) + 1 (length) + 32 = 37 bytes.
    let c = Cid::from_canonical_input("test", b"abc");
    let bin = c.to_binary();
    assert_eq!(bin.len(), 37, "json-codec CID is 37 bytes (2-byte codec varint)");
    assert_eq!(bin[0], 0x01, "CIDv1 prefix");
    assert_eq!(bin[1], 0x80, "codec varint LSB byte (continuation)");
    assert_eq!(bin[2], 0x04, "codec varint MSB byte");
    assert_eq!(bin[3], 0x1e, "blake3 multihash code");
    assert_eq!(bin[4], 0x20, "digest length 32");
    // We don't pin bytes 5.. here — that's the digest, checked elsewhere.
}

#[test]
fn raw_blob_cid_uses_single_byte_codec() {
    // raw codec is 0x55 — single varint byte.
    // Total: 1 + 1 + 1 + 1 + 32 = 36 bytes.
    let c = Cid::from_raw_blob(b"abc");
    let bin = c.to_binary();
    assert_eq!(bin.len(), 36, "raw-codec CID is 36 bytes");
    assert_eq!(bin[0], 0x01);
    assert_eq!(bin[1], 0x55, "raw codec");
    assert_eq!(bin[2], 0x1e);
    assert_eq!(bin[3], 0x20);
    assert_eq!(&bin[4..36], &BLAKE3_OF_ABC, "BLAKE3(\"abc\") pinned");
}

#[test]
fn raw_blob_empty_input_matches_known_blake3() {
    let c = Cid::from_raw_blob(b"");
    let bin = c.to_binary();
    assert_eq!(&bin[4..36], &BLAKE3_OF_EMPTY);
}

// ----- string form ----------------------------------------------------------

#[test]
fn string_form_starts_with_multibase_b() {
    // base32-lower multibase prefix is 'b'.
    let c = Cid::from_raw_blob(b"abc");
    let s = c.to_string();
    assert!(s.starts_with('b'), "multibase prefix 'b' for base32-lower, got {s}");
}

#[test]
fn string_form_uses_only_base32_lower_alphabet() {
    let c = Cid::from_raw_blob(b"abc");
    let s = c.to_string();
    // multibase 'b' alphabet: a-z + 2-7 (RFC4648 base32 lower, no padding).
    for (i, ch) in s.chars().enumerate() {
        if i == 0 {
            continue; // multibase prefix
        }
        assert!(
            matches!(ch, 'a'..='z' | '2'..='7'),
            "non-base32-lower char {ch:?} at {i} in {s}"
        );
    }
}

#[test]
fn string_length_is_stable_for_raw_codec() {
    // 36 binary bytes → 36*8 = 288 bits → ceil(288/5) = 58 base32 chars → +1 prefix = 59
    let c = Cid::from_raw_blob(b"abc");
    let s = c.to_string();
    assert_eq!(s.len(), 59, "raw CID string is exactly 59 chars, got {} for {s}", s.len());
}

#[test]
fn string_length_is_stable_for_json_codec() {
    // 37 binary bytes → 37*8 = 296 bits → ceil(296/5) = 60 base32 chars → +1 prefix = 61
    let c = Cid::from_canonical_input("x", b"y");
    let s = c.to_string();
    assert_eq!(s.len(), 61, "json CID string is exactly 61 chars, got {} for {s}", s.len());
}

// ----- determinism ----------------------------------------------------------

#[test]
fn same_input_produces_byte_identical_cid() {
    let a = Cid::from_canonical_input("compose_interfaces", b"COMPATIBLE");
    let b = Cid::from_canonical_input("compose_interfaces", b"COMPATIBLE");
    assert_eq!(a.to_binary(), b.to_binary());
    assert_eq!(a.to_string(), b.to_string());
}

#[test]
fn different_label_produces_different_cid() {
    // The whole point of the label argument: domain-separate computations.
    let a = Cid::from_canonical_input("compose_interfaces", b"X");
    let b = Cid::from_canonical_input("merkle_manifest",    b"X");
    assert_ne!(a.to_binary(), b.to_binary(), "different labels must produce different CIDs");
}

#[test]
fn different_bytes_produce_different_cid() {
    let a = Cid::from_canonical_input("x", b"foo");
    let b = Cid::from_canonical_input("x", b"bar");
    assert_ne!(a.to_binary(), b.to_binary());
}

#[test]
fn canonical_input_includes_protocol_version_prefix() {
    // The internal canonical input is:
    //   "hermes-lyra/0.3" || 0x00 || label || 0x00 || bytes
    // We can't see the internals, but we CAN verify that the digest
    // matches an externally-computed BLAKE3 over those exact bytes.
    let label = "skill_interface_hash";
    let bytes = b"hello";
    let c = Cid::from_canonical_input(label, bytes);

    let mut framed = Vec::new();
    framed.extend_from_slice(b"hermes-lyra/0.3");
    framed.push(0x00);
    framed.extend_from_slice(label.as_bytes());
    framed.push(0x00);
    framed.extend_from_slice(bytes);
    let expected_digest = blake3::hash(&framed);

    let bin = c.to_binary();
    let digest_in_cid = &bin[bin.len() - 32..];
    assert_eq!(
        digest_in_cid,
        expected_digest.as_bytes(),
        "CID digest must equal BLAKE3 over '{}' || 0x00 || label || 0x00 || bytes",
        "hermes-lyra/0.3"
    );
}

// ----- round-trip -----------------------------------------------------------

#[test]
fn string_roundtrip() {
    let original = Cid::from_canonical_input("L", b"DATA");
    let s = original.to_string();
    let parsed = Cid::parse(&s).expect("parse must succeed");
    assert_eq!(original.to_binary(), parsed.to_binary());
    assert_eq!(original.to_string(), parsed.to_string());
}

#[test]
fn binary_roundtrip() {
    let original = Cid::from_raw_blob(b"some-blob");
    let bin = original.to_binary();
    let parsed = Cid::from_binary(&bin).expect("from_binary must succeed");
    assert_eq!(bin, parsed.to_binary());
}

// ----- parser rejects malformed input ---------------------------------------

#[test]
fn parse_rejects_wrong_multibase_prefix() {
    let valid = Cid::from_raw_blob(b"x").to_string();
    let mut bad = valid.clone();
    bad.replace_range(0..1, "z"); // 'z' is base58btc — also a valid multibase but not what we accept
    assert!(matches!(Cid::parse(&bad), Err(CidParseError::UnsupportedMultibase(_))));
}

#[test]
fn parse_rejects_v0_cids() {
    // CIDv0 starts with "Qm" (base58btc fixed prefix for sha256). We reject it.
    let cidv0 = "QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG";
    assert!(matches!(Cid::parse(cidv0), Err(CidParseError::UnsupportedMultibase(_)) | Err(CidParseError::UnsupportedVersion(_))));
}

#[test]
fn parse_rejects_unknown_codec() {
    // Build a malformed binary: CIDv1 + codec 0x99 (unassigned in our enum) + blake3 + 32 + zeros.
    let mut bad = Vec::new();
    bad.push(0x01);
    bad.push(0x99); // unknown codec
    bad.push(0x1e);
    bad.push(0x20);
    bad.extend_from_slice(&[0u8; 32]);
    assert!(matches!(Cid::from_binary(&bad), Err(CidParseError::UnsupportedCodec(_))));
}

#[test]
fn parse_rejects_unknown_hash_code() {
    // CIDv1 + raw codec + 0x12 (sha2-256) — valid in IPFS but not in our v0.3 protocol
    let mut bad = Vec::new();
    bad.push(0x01);
    bad.push(0x55);
    bad.push(0x12); // sha2-256
    bad.push(0x20);
    bad.extend_from_slice(&[0u8; 32]);
    assert!(matches!(Cid::from_binary(&bad), Err(CidParseError::UnsupportedHashCode(_))));
}

#[test]
fn parse_rejects_wrong_digest_length() {
    // CIDv1 + raw codec + blake3 + length 16 (not 32)
    let mut bad = Vec::new();
    bad.push(0x01);
    bad.push(0x55);
    bad.push(0x1e);
    bad.push(0x10); // length 16
    bad.extend_from_slice(&[0u8; 16]);
    assert!(matches!(Cid::from_binary(&bad), Err(CidParseError::WrongDigestLength(_))));
}

#[test]
fn parse_rejects_truncated_input() {
    let bad = [0x01u8, 0x55, 0x1e, 0x20, 0xaa]; // header complete, digest cut off
    assert!(matches!(Cid::from_binary(&bad), Err(CidParseError::Truncated)));
}

#[test]
fn parse_rejects_extra_trailing_bytes() {
    let mut bad = Cid::from_raw_blob(b"x").to_binary().to_vec();
    bad.push(0xff);
    assert!(matches!(Cid::from_binary(&bad), Err(CidParseError::TrailingBytes)));
}

// ----- accessor sanity ------------------------------------------------------

#[test]
fn accessors_return_constructed_values() {
    let c = Cid::from_raw_blob(b"abc");
    assert_eq!(c.codec(), Codec::Raw);
    assert_eq!(c.hash_code(), HashCode::Blake3_256);
    assert_eq!(c.digest(), &BLAKE3_OF_ABC);
}

#[test]
fn json_codec_marker() {
    let c = Cid::from_canonical_input("x", b"y");
    assert_eq!(c.codec(), Codec::Json);
}
