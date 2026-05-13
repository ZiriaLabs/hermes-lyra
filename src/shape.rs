//! The eight Lyra leaf shapes, declared as sealed types.
//!
//! Each shape is an atomic building block for a `SkillDescriptor`'s
//! `input_shape` / `output_shape`. Sealing prevents user code from
//! constructing shape values outside the sanctioned API.

use uor_foundation::pipeline::ConstraintRef;
use uor_foundation_sdk::output_shape;

// -- Integer shapes --
// For integer types SITE_COUNT = byte width, CYCLE_SIZE = 1 (small types).
// IntoBindingValue::MAX_BYTES is set to SITE_COUNT by the macro's default
// (matching CYCLE_SIZE or SITE_COUNT, whichever is appropriate).

output_shape! {
    pub struct LyraU8;
    impl ConstrainedTypeShape for LyraU8 {
        const IRI: &'static str = "https://lyra-protocol.org/shapes/v0.1/u8";
        const SITE_COUNT: usize = 1;
        const CONSTRAINTS: &'static [ConstraintRef] = &[
            ConstraintRef::Site { position: 0 }
        ];
        const CYCLE_SIZE: u64 = 1;
    }
}

output_shape! {
    pub struct LyraU16;
    impl ConstrainedTypeShape for LyraU16 {
        const IRI: &'static str = "https://lyra-protocol.org/shapes/v0.1/u16";
        const SITE_COUNT: usize = 2;
        const CONSTRAINTS: &'static [ConstraintRef] = &[
            ConstraintRef::Site { position: 0 },
            ConstraintRef::Site { position: 1 }
        ];
        const CYCLE_SIZE: u64 = 1;
    }
}

output_shape! {
    pub struct LyraU32;
    impl ConstrainedTypeShape for LyraU32 {
        const IRI: &'static str = "https://lyra-protocol.org/shapes/v0.1/u32";
        const SITE_COUNT: usize = 4;
        const CONSTRAINTS: &'static [ConstraintRef] = &[
            ConstraintRef::Site { position: 0 },
            ConstraintRef::Site { position: 1 },
            ConstraintRef::Site { position: 2 },
            ConstraintRef::Site { position: 3 }
        ];
        const CYCLE_SIZE: u64 = 1;
    }
}

output_shape! {
    pub struct LyraU64;
    impl ConstrainedTypeShape for LyraU64 {
        const IRI: &'static str = "https://lyra-protocol.org/shapes/v0.1/u64";
        const SITE_COUNT: usize = 8;
        const CONSTRAINTS: &'static [ConstraintRef] = &[
            ConstraintRef::Site { position: 0 },
            ConstraintRef::Site { position: 1 },
            ConstraintRef::Site { position: 2 },
            ConstraintRef::Site { position: 3 },
            ConstraintRef::Site { position: 4 },
            ConstraintRef::Site { position: 5 },
            ConstraintRef::Site { position: 6 },
            ConstraintRef::Site { position: 7 }
        ];
        const CYCLE_SIZE: u64 = 1;
    }
}

// -- string / bytes --
// 16 MiB = the universal capacity bound for string/bytes types.

output_shape! {
    pub struct LyraString;
    impl ConstrainedTypeShape for LyraString {
        const IRI: &'static str = "https://lyra-protocol.org/shapes/v0.1/string";
        const SITE_COUNT: usize = 1;
        const CONSTRAINTS: &'static [ConstraintRef] = &[
            ConstraintRef::Site { position: 0 }
        ];
        const CYCLE_SIZE: u64 = 16_777_216;
    }
}

output_shape! {
    pub struct LyraBytes;
    impl ConstrainedTypeShape for LyraBytes {
        const IRI: &'static str = "https://lyra-protocol.org/shapes/v0.1/bytes";
        const SITE_COUNT: usize = 1;
        const CONSTRAINTS: &'static [ConstraintRef] = &[
            ConstraintRef::Site { position: 0 }
        ];
        const CYCLE_SIZE: u64 = 16_777_216;
    }
}

// -- tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use uor_foundation::enforcement::GroundedShape;
    use uor_foundation::pipeline::{ConstrainedTypeShape, IntoBindingValue};

    fn assert_into_binding<T: IntoBindingValue>() {}
    fn assert_grounded<T: GroundedShape>() {}

    #[test]
    fn u8_is_into_binding() {
        assert_into_binding::<LyraU8>();
        assert_grounded::<LyraU8>();
        assert_eq!(<LyraU8 as ConstrainedTypeShape>::SITE_COUNT, 1);
    }

    #[test]
    fn u64_is_into_binding() {
        assert_into_binding::<LyraU64>();
        assert_grounded::<LyraU64>();
        assert_eq!(<LyraU64 as ConstrainedTypeShape>::SITE_COUNT, 8);
    }

    #[test]
    fn string_max_bytes() {
        assert_eq!(<LyraString as ConstrainedTypeShape>::CYCLE_SIZE, 16_777_216);
    }

    #[test]
    fn all_shapes_have_lyra_iris() {
        assert!(<LyraU8 as ConstrainedTypeShape>::IRI.contains("lyra-protocol.org"));
        assert!(<LyraString as ConstrainedTypeShape>::IRI.contains("lyra-protocol.org"));
    }
}
