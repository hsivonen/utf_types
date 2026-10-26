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

use core::fmt::Formatter;
use core::num::NonZeroU16;

/// A type for signaling UTF-16 errors.
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub struct Utf16CodeUnitError;

impl core::fmt::Display for Utf16CodeUnitError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), core::fmt::Error> {
        write!(f, "code unit not fit the requested position in UTF-16")
    }
}

impl core::error::Error for Utf16CodeUnitError {}

/// For making sure that reviewing this file is sufficient for
/// reviewing the implementations of `Utf16CodeUnitSequence`.
trait Seal {}

/// Common output conversions for both UTF-16 code unit sequences.
#[allow(private_bounds)] // allow sealing
pub trait Utf16CodeUnitSequence: Seal + Copy + Clone {
    /// Convert the code unit sequence to `char`
    fn to_char(self) -> char;

    /// View the byte sequence as `&[u16]`
    ///
    /// # Safety-usable invariant
    ///
    /// The caller may rely on the length being 1 or 2.
    fn as_slice(&self) -> &[u16];
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
}

impl Seal for Bmp {}

impl Utf16CodeUnitSequence for Bmp {
    #[inline(always)]
    fn to_char(self) -> char {
        let scalar = self.code_unit as u32;
        debug_assert!(char::from_u32(scalar).is_some());
        // SAFETY: The invariant of `Bmp` guarantees that
        // we don't have a surrogate, so `scalar` is indeed
        // a Unicode scalar value.
        unsafe { char::from_u32_unchecked(scalar) }
    }

    #[inline(always)]
    fn as_slice(&self) -> &[u16] {
        core::slice::from_ref(&self.code_unit)
    }
}

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
                code_unit: if let Some(min) = NonZeroU16::new($max) {
                    min
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

    /// Contructor from high surrogate
    #[inline(always)]
    pub const fn new_with_high(high_surrogate: HighSurrogate) -> Self {
        // INVARIANT UPHELD: The invariant of `HighSurrogate` is stricter
        // that the invariant of `Self`, so this is correct.
        Self {
            code_unit: high_surrogate.code_unit,
        }
    }

    /// Contructor from low surrogate
    #[inline(always)]
    pub const fn new_with_low(low_surrogate: LowSurrogate) -> Self {
        // INVARIANT UPHELD: The invariant of `LowSurrogate` is stricter
        // that the invariant of `Self`, so this is correct.
        Self {
            code_unit: low_surrogate.code_unit,
        }
    }
}

/// A high surrogate
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct HighSurrogate {
    // SAFETY-USABLE INVARIANT: A surrogate.
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
        /// Whether `code_unit` is a surrogate.
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
            code_units: [Surrogate::new_with_high(high), Surrogate::new_with_low(low)],
        }
    }
}

impl Seal for SurrogatePair {}

impl Utf16CodeUnitSequence for SurrogatePair {
    #[inline(always)]
    fn to_char(self) -> char {
        let scalar = ((self.code_units[0].to_u16() as u32) << 10)
            + (self.code_units[1].to_u16() as u32)
            - (((0xD800u32 << 10) - 0x10000u32) + 0xDC00u32);
        debug_assert!(char::from_u32(scalar).is_some());
        // SAFETY: The above formulation results in a value in the `char` range if the
        // safety invariant of this type is upheld.
        unsafe { char::from_u32_unchecked(scalar) }
    }

    #[inline(always)]
    fn as_slice(&self) -> &[u16] {
        // SAFETY: Viewing a type that's `repr(transparent)` of `NonZeroU16` as `u16`
        // is safe.
        let slice: &[Surrogate] = &self.code_units;
        unsafe { core::mem::transmute(slice) }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum BmpOrSurrogate {
    Bmp(Bmp),
    Surrogate(Surrogate),
}

impl BmpOrSurrogate {
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
