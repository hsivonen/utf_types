// Copyright Mozilla Foundation
//
// Licensed under the Apache License (Version 2.0), or the MIT license,
// (the "Licenses") at your option. You may not use this file except in
// compliance with one of the Licenses. You may obtain copies of the
// Licenses at:
//
//    https://www.apache.org/licenses/LICENSE-2.0
//    https://opensource.org/licenses/MIT
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the Licenses is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the Licenses for the specific language governing permissions and
// limitations under the Licenses.

//! Types for UTF-16 code unit sequences and their parts.

use core::fmt::Formatter;
use core::num::NonZeroU16;

/// View `&[Surrogate]` as `&[u16]
#[inline(always)]
const fn cast_slice(s: &[Surrogate]) -> &[u16] {
    // SAFETY: It's safe to view `repr(transparent)` of
    // `NonZeroU16` as `u16`.
    unsafe { core::slice::from_raw_parts(s.as_ptr().cast(), s.len()) }
}

/// A type for signaling UTF-16 errors.
#[derive(Debug, PartialEq, Clone, Copy, Eq, Hash)]
#[non_exhaustive]
pub struct Utf16CodeUnitError;

impl core::fmt::Display for Utf16CodeUnitError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), core::fmt::Error> {
        write!(f, "code unit does not fit the requested position in UTF-16")
    }
}

impl core::error::Error for Utf16CodeUnitError {}

macro_rules! surrogate_impl {
    ($min:expr,
     $max:expr,
     $(#[$is_meta:meta])*,
     $(#[$unchecked_meta:meta])*,
     $(#[$checked_meta:meta])*,
    ) => {
        /// Minimum possible value.
        pub const MIN: Self = const {
            Self {
                code_unit: if let Some(min) = NonZeroU16::new($min) {
                    min
                } else {
                    panic!();
                },
            }
        };

        /// Maximum possible value.
        pub const MAX: Self = const {
            Self {
                code_unit: if let Some(max) = NonZeroU16::new($max) {
                    max
                } else {
                    panic!();
                },
            }
        };

        $(#[$is_meta])*
        #[inline(always)]
        pub const fn is(code_unit: u16) -> bool {
            code_unit >= Self::MIN.to_u16() && code_unit <= Self::MAX.to_u16()
        }

        $(#[$unchecked_meta])*
        #[inline(always)]
        pub const unsafe fn new_unchecked(code_unit: u16) -> Self {
            debug_assert!(Self::is(code_unit));
            // INVARIANT UPHELD: The range is this function's safety invariant.
            // SAFETY: Surrogateness implies non-zeroness.
            Self { code_unit: unsafe { NonZeroU16::new_unchecked(code_unit) } }
        }

        $(#[$checked_meta])*
        #[inline(always)]
        pub const fn try_new(code_unit: u16) -> Result<Self, Utf16CodeUnitError> {
            if Self::is(code_unit) {
                // SAFETY: We checked the safety invariant immediately above.
                Ok(unsafe { Self::new_unchecked(code_unit) })
            } else {
                Err(Utf16CodeUnitError)
            }
        }

        /// Convert to plain code unit
        #[inline(always)]
        pub const fn to_u16(self) -> u16 {
            self.code_unit.get()
        }
    };
}

macro_rules! to_surrogate {
    () => {
        /// Convert to `Surrogate`
        #[inline(always)]
        pub const fn to_surrogate(self) -> Surrogate {
            // INVARIANT UPHELD: `Self` has a stricter invariant than the target
            // type, so the conversion is safe.
            Surrogate {
                code_unit: self.code_unit,
            }
        }
    };
}

macro_rules! code_unit_impls {
    ($type:ident,
     $type_specific_ident:ident,
    ) => {
        impl TryFrom<u16> for $type {
            type Error = Utf16CodeUnitError;

            fn try_from(code_unit: u16) -> Result<Self, Utf16CodeUnitError> {
                Self::try_new(code_unit)
            }
        }

        impl From<$type> for u16 {
            fn from($type_specific_ident: $type) -> Self {
                $type_specific_ident.to_u16()
            }
        }
    };
}

/// For making sure that reviewing this file is sufficient for
/// reviewing the implementations of `Utf16CodeUnitSequence`.
trait Seal {}

/// Common output conversions for both UTF-16 code unit sequences.
#[allow(private_bounds)] // allow sealing
pub trait Utf16CodeUnitSequence: Seal + Copy + Clone + Into<char> + AsRef<[u16]> {}

macro_rules! sequence_impls {
    ($type:ident,
     $type_specific_ident:ident,
    ) => {
        impl Seal for $type {}

        impl Utf16CodeUnitSequence for $type {}

        impl From<$type> for char {
            fn from($type_specific_ident: $type) -> Self {
                $type_specific_ident.to_char()
            }
        }

        impl AsRef<[u16]> for $type {
            fn as_ref(&self) -> &[u16] {
                self.as_slice()
            }
        }
    };
}

/// A single-code-unit UTF-16 sequence representing a (non-surrogate)
/// character in the Basic Multilingual Plane.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct Bmp {
    // SAFETY-USABLE INVARIANT: Not a surrogate.
    code_unit: u16, // Would be nice to have a niche without hurting perf
}

impl Bmp {
    /// Whether `code_unit` is in range for this type (not a surrogate)
    #[inline(always)]
    pub const fn is(code_unit: u16) -> bool {
        !Surrogate::is(code_unit)
    }

    /// Unchecked constructor
    ///
    /// # Safety
    ///
    /// `code_unit` must _not_ be in the range `0xD800..=0xDFFF`.
    ///
    /// # Panics
    ///
    /// When debug assertions are enabled, panics if the safety invariant
    /// is not upheld.
    #[inline(always)]
    pub const unsafe fn new_unchecked(code_unit: u16) -> Self {
        debug_assert!(Self::is(code_unit));
        // INVARIANT UPHELD: The range is this function's safety invariant.
        Self { code_unit }
    }

    /// Fallible constructor
    ///
    /// `Ok` if _not_ surrogate and `Err` otherwise.
    #[inline(always)]
    pub const fn try_new(code_unit: u16) -> Result<Self, Utf16CodeUnitError> {
        if Self::is(code_unit) {
            // SAFETY: We checked the safety invariant immediately above.
            Ok(unsafe { Self::new_unchecked(code_unit) })
        } else {
            Err(Utf16CodeUnitError)
        }
    }

    /// Convert to plain code unit
    #[inline(always)]
    pub const fn to_u16(self) -> u16 {
        self.code_unit
    }

    /// Convert to `char`
    #[inline(always)]
    pub const fn to_char(self) -> char {
        let scalar = self.code_unit as u32;
        debug_assert!(char::from_u32(scalar).is_some());
        // SAFETY: The invariant of `Bmp` guarantees that
        // we don't have a surrogate, so `scalar` is indeed
        // a Unicode scalar value.
        unsafe { char::from_u32_unchecked(scalar) }
    }

    /// View as `&[u16]`
    #[inline(always)]
    pub const fn as_slice(&self) -> &[u16] {
        core::slice::from_ref(&self.code_unit)
    }
}

code_unit_impls!(Bmp, bmp,);

sequence_impls!(Bmp, bmp,);

/// A single surrogate code unit, which can be a high
/// surrogate or a low surrogate
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct Surrogate {
    // SAFETY-USABLE INVARIANT: A surrogate.
    code_unit: NonZeroU16,
}

impl Surrogate {
    surrogate_impl!(
        0xD800,
        0xDFFF,
        /// Whether `code_unit` is a surrogate.
        ,
        /// Unchecked constructor
        ///
        /// # Safety
        ///
        /// `code_unit` must be in the range `0xD800..=0xDFFF`.
        ///
        /// # Panics
        ///
        /// When debug assertions are enabled, panics if the safety invariant
        /// is not upheld.
        ,
        /// Fallible constructor
        ///
        /// `Ok` if surrogate and `Err` otherwise.
        ,
    );
}

code_unit_impls!(Surrogate, surrogate,);

/// A high surrogate
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct HighSurrogate {
    // SAFETY-USABLE INVARIANT: A high surrogate.
    code_unit: NonZeroU16,
}

impl HighSurrogate {
    surrogate_impl!(
        0xD800,
        0xDBFF,
        /// Whether `code_unit` is a high surrogate.
        ,
        /// Unchecked constructor
        ///
        /// # Safety
        ///
        /// `code_unit` must be in the range `0xD800..=0xDBFF`.
        ///
        /// # Panics
        ///
        /// When debug assertions are enabled, panics if the safety invariant
        /// is not upheld.
        ,
        /// Fallible constructor
        ///
        /// `Ok` if high surrogate and `Err` otherwise.
        ,
    );

    to_surrogate!();

    /// Fallible constructor given a surrogate
    ///
    /// `Ok` if high surrogate and `Err` otherwise.
    #[inline(always)]
    pub const fn try_new_with_surrogate(surrogate: Surrogate) -> Result<Self, Utf16CodeUnitError> {
        let code_unit = surrogate.to_u16();
        if code_unit <= Self::MAX.to_u16() {
            // SAFETY: We checked the safety invariant immediately above.
            Ok(unsafe { Self::new_unchecked(code_unit) })
        } else {
            Err(Utf16CodeUnitError)
        }
    }
}

code_unit_impls!(HighSurrogate, high_surrogate,);

/// A low surrogate
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct LowSurrogate {
    // SAFETY-USABLE INVARIANT: A low surrogate.
    code_unit: NonZeroU16,
}

impl LowSurrogate {
    surrogate_impl!(
        0xDC00,
        0xDFFF,
        /// Whether `code_unit` is a low surrogate.
        ,
        /// Unchecked constructor
        ///
        /// # Safety
        ///
        /// `code_unit` must be in the range `0xDC00..=0xDFFF`.
        ///
        /// # Panics
        ///
        /// When debug assertions are enabled, panics if the safety invariant
        /// is not upheld.
        ,
        /// Fallible constructor
        ///
        /// `Ok` if low surrogate and `Err` otherwise.
        ,
    );

    to_surrogate!();

    /// Fallible constructor given a surrogate
    ///
    /// `Ok` if high surrogate and `Err` otherwise.
    #[inline(always)]
    pub const fn try_new_with_surrogate(surrogate: Surrogate) -> Result<Self, Utf16CodeUnitError> {
        let code_unit = surrogate.to_u16();
        if code_unit >= Self::MIN.to_u16() {
            // SAFETY: We checked the safety invariant immediately above.
            Ok(unsafe { Self::new_unchecked(code_unit) })
        } else {
            Err(Utf16CodeUnitError)
        }
    }
}

code_unit_impls!(LowSurrogate, low_surrogate,);

/// A two-code-unit UTF-16 sequence
///
/// A high surrogate followed by a low surrogate representing
/// a Unicode scalar value outside the Basic Multilingual Plane.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct SurrogatePair {
    /// SAFETY-USABLE INVARIANT: `code_units[0]` meets the requirements
    /// of `HighSurrogate` and `code_units[1]` meets the requirements
    /// of `LowSurrogate`.
    code_units: [Surrogate; 2],
}

impl SurrogatePair {
    /// Constructor from a high surrogate and a low surrogate
    #[inline(always)]
    pub const fn new(high: HighSurrogate, low: LowSurrogate) -> Self {
        // INVARIANT UPHELD: We put the high surrogate in the first position
        // and the low surrogate in the second position.
        Self {
            code_units: [high.to_surrogate(), low.to_surrogate()],
        }
    }

    /// Convert to typed code unit tuple.
    #[inline(always)]
    pub const fn to_typed_code_units(self) -> (HighSurrogate, LowSurrogate) {
        (
            // INVARIANT UPHELD: This is actually a high surrogate per the
            // invariant of `self.code_units`.
            HighSurrogate {
                code_unit: self.code_units[0].code_unit,
            },
            // INVARIANT UPHELD: This is actually a low surrogate per the
            // invariant of `self.code_units`.
            LowSurrogate {
                code_unit: self.code_units[1].code_unit,
            },
        )
    }

    /// Convert to `char`
    #[inline(always)]
    pub const fn to_char(self) -> char {
        let scalar = ((self.code_units[0].to_u16() as u32) << 10)
            + (self.code_units[1].to_u16() as u32)
            - (((0xD800u32 << 10) - 0x10000u32) + 0xDC00u32);
        debug_assert!(char::from_u32(scalar).is_some());
        // SAFETY: The above formulation results in a value in the `char` range if the
        // safety invariant of this type is upheld.
        unsafe { char::from_u32_unchecked(scalar) }
    }

    /// View as `&[u16]`
    #[inline(always)]
    pub const fn as_slice(&self) -> &[u16] {
        cast_slice(&self.code_units)
    }
}

sequence_impls!(SurrogatePair, surrogate_pair,);

/// Type for categorizing code units as either non-surrogates or surrogates.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum BmpOrSurrogate {
    /// Non-surrogate code unit on the Basic Multilingual Plane
    Bmp(Bmp),
    /// Surrogate
    Surrogate(Surrogate),
}

impl BmpOrSurrogate {
    /// Constructor
    #[inline(always)]
    pub const fn new(code_unit: u16) -> Self {
        if Bmp::is(code_unit) {
            // SAFETY: We checked the invariant above.
            Self::Bmp(unsafe { Bmp::new_unchecked(code_unit) })
        } else {
            // SAFETY: We checked the invariant above.
            Self::Surrogate(unsafe { Surrogate::new_unchecked(code_unit) })
        }
    }
}

// TODO: Add 16-bit types that correspond to the value ranges of 1, 2, and 3-byte
// UTF-8 sequences.
