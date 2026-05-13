//! Compact little-endian wire format for sealed proofs of execution.
//!
//! Serializer only — `trace_to_bytes` is called from `gate.rs` to fold
//! a trace into the receipt envelope. There is no deserializer because
//! Lyra verification re-runs the pipeline from canonical descriptor
//! bytes rather than replaying a stored trace; a deserializer would
//! be unused code with its own audit surface.
//!
//! Format (version 6, matching `TRACE_REPLAY_FORMAT_VERSION`):
//!   u32  : format version (must be 6)
//!   u16  : event count (len)
//!   u16  : witt_level_bits
//!   u8   : fingerprint width in bytes
//!   [u8; 32] : fingerprint bytes (only first `width` bytes are meaningful)
//!   For each event (0..len):
//!     u32  : step_index
//!     u8   : primitive_op discriminant
//!     u128 : target address (little-endian)

use uor_foundation::enforcement::Trace;
use uor_foundation::enums::PrimitiveOp;

const FORMAT_VERSION: u32 = 6;
const TR_MAX: usize = 256;

/// Serialize a `Trace` into a byte vector.
pub fn trace_to_bytes(trace: &Trace<TR_MAX>) -> Vec<u8> {
    let mut out = Vec::new();

    // Header
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&trace.len().to_le_bytes());
    out.extend_from_slice(&trace.witt_level_bits().to_le_bytes());

    let fp = trace.content_fingerprint();
    let width = fp.width_bytes();
    out.push(width);
    out.extend_from_slice(fp.as_bytes());

    // Events
    for i in 0..trace.len() as usize {
        let ev = trace.event(i).expect("event in range");
        out.extend_from_slice(&ev.step_index().to_le_bytes());
        out.push(op_to_discriminant(ev.op()));
        out.extend_from_slice(&ev.target().as_u128().to_le_bytes());
    }

    out
}

fn op_to_discriminant(op: PrimitiveOp) -> u8 {
    match op {
        PrimitiveOp::Neg => 0,
        PrimitiveOp::Bnot => 1,
        PrimitiveOp::Succ => 2,
        PrimitiveOp::Pred => 3,
        PrimitiveOp::Add => 4,
        PrimitiveOp::Sub => 5,
        PrimitiveOp::Mul => 6,
        PrimitiveOp::Xor => 7,
        PrimitiveOp::And => 8,
        PrimitiveOp::Or => 9,
        PrimitiveOp::Le => 10,
        PrimitiveOp::Lt => 11,
        PrimitiveOp::Ge => 12,
        PrimitiveOp::Gt => 13,
        PrimitiveOp::Concat => 14,
    }
}
