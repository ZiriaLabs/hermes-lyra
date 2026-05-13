//! JSON-string CLI API used by the `lyra` binary.
//!
//! Two operations:
//! - [`score`]: run a protocol computation and assemble a [`Receipt`].
//! - [`verify`]: re-run the computation on `(computation_id, input)` and
//!   compare the result to the receipt's `output_hash`.
//!
//! The wire `Receipt` is intentionally minimal: `{computation_id, input,
//! output_hash, runtime}`. Forgery resistance comes from BLAKE3 over the
//! canonical input plus the deterministic computation — not from a
//! separate "seal" field, which an earlier version carried but which
//! provided no input-dependent binding under the current UOR pipeline
//! configuration. In-process Rust callers who want a sealed UOR witness
//! still get one via [`crate::gate::validate_skill`] →
//! [`crate::gate::Attestation`].

use crate::computations;
use crate::receipt::Receipt;

/// Run a protocol computation and assemble a receipt.
///
/// The receipt is **not** written to disk by this function — callers
/// (the CLI, library consumers, integration tests) decide whether to
/// persist it.
///
/// # Errors
///
/// Returns an error if `computation_id` is unknown or `input` is
/// rejected by the computation.
pub fn score(computation_id: &str, input: &str) -> Result<Receipt, String> {
    let output = computations::run(computation_id, input)?;
    let output_hash = hex_encode(&output);
    Ok(Receipt {
        computation_id: computation_id.to_string(),
        input: input.to_string(),
        output_hash,
        runtime: crate::LYRA_RUNTIME_IDENT.to_string(),
    })
}

/// Outcome of a verify call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyOutcome {
    /// `output_hash` matches a fresh re-execution.
    Ok { output_hash: String },
    /// The receipt's `output_hash` did not match a fresh re-execution.
    /// Returned as a value (rather than `Err`) so callers can
    /// distinguish forged content from I/O or parse failures.
    ContentMismatch {
        expected: String,
        actual_in_receipt: String,
    },
}

/// Verify a receipt: re-run the computation and compare the output hash.
///
/// `computation_id` and `input` must match the values stored in the
/// receipt; a receipt cannot be re-bound to a different pair.
pub fn verify(
    computation_id: &str,
    input: &str,
    receipt: &Receipt,
) -> Result<VerifyOutcome, String> {
    if receipt.computation_id != computation_id {
        return Err(format!(
            "computation_id mismatch: receipt={}, args={}",
            receipt.computation_id, computation_id
        ));
    }
    if receipt.input != input {
        return Err(format!(
            "input mismatch: receipt={}, args={}",
            receipt.input, input
        ));
    }
    if !crate::runtime_is_compatible(&receipt.runtime) {
        return Err(format!(
            "SubstrateVersionMismatch: receipt={}, verifier={} (compat={:?})",
            receipt.runtime,
            crate::LYRA_RUNTIME_IDENT,
            crate::COMPATIBLE_RUNTIMES,
        ));
    }

    // Content integrity: re-run computation, compare hashes.
    let output = computations::run(computation_id, input)?;
    let expected_hash = hex_encode(&output);

    if expected_hash == receipt.output_hash {
        Ok(VerifyOutcome::Ok { output_hash: expected_hash })
    } else {
        Ok(VerifyOutcome::ContentMismatch {
            expected: expected_hash,
            actual_in_receipt: receipt.output_hash.clone(),
        })
    }
}

// ---- minimal hex/base64 utilities (no extra deps) ----

pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

pub fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let b = &bytes[i..i + 3];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | (b[2] as u32);
        out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
        out.push(TABLE[(n & 0x3f) as usize] as char);
        i += 3;
    }
    let rem = bytes.len() - i;
    if rem == 1 {
        let n = (bytes[i] as u32) << 16;
        out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = (bytes[i] as u32) << 16 | (bytes[i + 1] as u32) << 8;
        out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
        out.push('=');
    }
    out
}

pub fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 4 <= bytes.len() {
        let a = val(bytes[i]).ok_or("invalid base64")?;
        let b = val(bytes[i + 1]).ok_or("invalid base64")?;
        let c = val(bytes[i + 2]);
        let d = val(bytes[i + 3]);
        let n = (a << 18) | (b << 12) | (c.unwrap_or(0) << 6) | d.unwrap_or(0);
        out.push((n >> 16) as u8);
        if c.is_some() {
            out.push(((n >> 8) & 0xff) as u8);
        }
        if d.is_some() {
            out.push((n & 0xff) as u8);
        }
        i += 4;
    }
    Ok(out)
}
