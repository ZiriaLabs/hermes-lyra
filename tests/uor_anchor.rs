//! Anchor verification: prove that the UOR substrate is wired in via the
//! Rust type system, not just referenced as a runtime dependency.
//!
//! Three claims under test:
//!
//! 1. Every per-leaf cap in the descriptor validator is a trait
//!    projection through `uor_foundation::pipeline::ConstrainedTypeShape`
//!    — change a `SITE_COUNT`/`CYCLE_SIZE` in `crate::shape` and the
//!    validator's bounds move with it at compile time.
//!
//! 2. The `LyraHasher` registered with the substrate is the actual hasher
//!    that mints receipts — it implements `uor_foundation::enforcement::Hasher`
//!    and produces BLAKE3-256 over the same bytes the pipeline canonicalizes.
//!
//! 3. The `runtime` field of every minted receipt is constructed at compile
//!    time from `env!("UOR_FOUNDATION_VERSION")`, so a receipt cannot lie
//!    about which substrate produced it; `runtime_is_compatible` rejects
//!    anything else.

use uor_foundation::enforcement::Hasher;
use uor_foundation::pipeline::ConstrainedTypeShape;

use lyra_ref::descriptor::LYRA_MAX_BYTES;
use lyra_ref::gate::LyraHasher;
use lyra_ref::shape::{LyraBytes, LyraString, LyraU16, LyraU32, LyraU64, LyraU8};
use lyra_ref::{runtime_is_compatible, LYRA_RUNTIME_IDENT};

#[test]
fn caps_are_trait_projections_not_magic_numbers() {
    // The validator's internal U8_MAX..U64_MAX consts are PRIVATE and are
    // literally `<LyraU8..LyraU64 as ConstrainedTypeShape>::SITE_COUNT as u64`.
    // We re-derive the same expressions here. If a future commit changes
    // either the sealed type or the validator without keeping them
    // synchronised, the matching invariant test inside descriptor.rs
    // breaks; this test merely demonstrates the public anchor.
    let u8_max  = <LyraU8  as ConstrainedTypeShape>::SITE_COUNT as u64;
    let u16_max = <LyraU16 as ConstrainedTypeShape>::SITE_COUNT as u64;
    let u32_max = <LyraU32 as ConstrainedTypeShape>::SITE_COUNT as u64;
    let u64_max = <LyraU64 as ConstrainedTypeShape>::SITE_COUNT as u64;
    assert_eq!(u8_max,  1, "u8 site count anchored to substrate trait");
    assert_eq!(u16_max, 2, "u16 site count anchored to substrate trait");
    assert_eq!(u32_max, 4, "u32 site count anchored to substrate trait");
    assert_eq!(u64_max, 8, "u64 site count anchored to substrate trait");

    // Universal 16 MiB cap really is the sealed type's CYCLE_SIZE.
    assert_eq!(LYRA_MAX_BYTES, <LyraString as ConstrainedTypeShape>::CYCLE_SIZE);
    assert_eq!(LYRA_MAX_BYTES, <LyraBytes  as ConstrainedTypeShape>::CYCLE_SIZE);
    assert_eq!(LYRA_MAX_BYTES, 16 * 1024 * 1024);
}

#[test]
fn lyra_hasher_implements_substrate_hasher_trait() {
    // This compiles only because LyraHasher impls
    // `uor_foundation::enforcement::Hasher`. If the substrate ever
    // changes the trait shape, this stops compiling.
    fn assert_hasher<H: Hasher>() {}
    assert_hasher::<LyraHasher>();

    // OUTPUT_BYTES is a substrate trait associated constant.
    assert_eq!(<LyraHasher as Hasher>::OUTPUT_BYTES, 32);

    // And it actually hashes the bytes the substrate streams in.
    let h = <LyraHasher as Hasher>::initial();
    let h = h.fold_bytes(b"hermes-lyra");
    let h = h.fold_byte(0u8);
    let h = h.fold_bytes(b"anchor-test");
    let got = h.finalize();
    let expected = *blake3::hash(b"hermes-lyra\x00anchor-test").as_bytes();
    assert_eq!(got, expected, "LyraHasher must be BLAKE3-256 over the same bytes");
}

#[test]
fn runtime_ident_is_compile_time_pinned_to_substrate_version() {
    // The runtime ident is built from env!() macros in lib.rs:
    //   concat!("hermes-lyra/", CARGO_PKG_VERSION, "+uor-foundation/", UOR_FOUNDATION_VERSION)
    // so the string is fixed at build time and unforgeable inside this
    // binary.
    assert!(LYRA_RUNTIME_IDENT.starts_with("hermes-lyra/"));
    assert!(LYRA_RUNTIME_IDENT.contains("+uor-foundation/"));

    // Only this exact runtime ident verifies. Anything else (including a
    // future substrate bump) is rejected by runtime_is_compatible.
    assert!(runtime_is_compatible(LYRA_RUNTIME_IDENT));
    assert!(!runtime_is_compatible("hermes-lyra/9.9.9+uor-foundation/9.9.9"));
    assert!(!runtime_is_compatible("hermes-lyra/0.2.0+uor-foundation/0.4.99"));
    assert!(!runtime_is_compatible(""));
    assert!(!runtime_is_compatible("lyra/0.1"));
}
