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

//! Types for UTF-8 code unit sequences and their parts.

use core::fmt::Formatter;
use core::mem::transmute;

/// A type for signaling UTF-8 errors.
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub struct Utf8ByteError;

impl core::fmt::Display for Utf8ByteError {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), core::fmt::Error> {
        write!(f, "byte does not fit the requested position in UTF-8")
    }
}

impl core::error::Error for Utf8ByteError {}

#[repr(align(64))] // Align to cache lines
struct Utf8Data {
    table: [u8; 384],
}

static UTF8_DATA: Utf8Data = Utf8Data {
    table: const {
        // The first 256 entries are for looking up `second`.
        // The rest as for looking up `first` on the assumption
        // that `first` is not ASCII.
        let mut table = [0u8; 384];
        // Lanes 0 and 1 are left unused to allow the tag
        // bits of the third byte in a three-byte sequence
        // to be shifted there for a single-branch comparison.

        // Trails
        let mut i = 0u16;
        while i < 0x100 {
            let second = i as u8;
            // If a `second` is valid given a lane, we leave a lane as zero.
            // We put 1 on the lane if the `second` is invalid given a lane.
            let mut combined = 1 << 2; // invalid lead
            if second < 0x80 || second > 0xBF {
                combined |= 1 << 3; // normal trail
            }
            if second < 0xA0 || second > 0xBF {
                combined |= 1 << 4; // three-byte special lower bound
            }
            if second < 0x80 || second > 0x9F {
                combined |= 1 << 5; // three-byte special upper bound
            }
            if second < 0x90 || second > 0xBF {
                combined |= 1 << 6; // four-byte special lower bound
            }
            if second < 0x80 || second > 0x8F {
                combined |= 1 << 7; // four-byte special upper bound
            }
            table[second as usize] = combined;
            i += 1;
        }

        // Leads
        i = 0x80; // We don't cover ASCII for leads.
        while i < 0x100 {
            let first = i as u8;
            let lane = match first {
                0xC2..=0xDF | 0xE1..=0xEC | 0xEE..=0xEF | 0xF1..=0xF3 => {
                    1 << 3 // normal trail
                }
                0xE0 => {
                    1 << 4 // three-byte special lower bound
                }
                0xED => {
                    1 << 5 // three-byte special upper bound
                }
                0xF0 => {
                    1 << 6 // four-byte special lower bound
                }
                0xF4 => {
                    1 << 7 // four-byte special upper bound
                }
                _ => {
                    // invalid lead
                    1 << 2
                }
            };
            table[0x80 + first as usize] = lane;
            i += 1;
        }

        table
    },
};

/// `true` iff `continuation` is valid as:
/// * The second byte of a two-byte sequence.
/// * The third byte of a three-byte sequence.
/// * The third byte of a four-byte sequence.
/// * The fourth byte of a four-byte sequence.
///
/// Or, alternatively, `true` iff `continuation` is a continuation
/// byte in general. That is, the second byte of a three-byte
/// sequence or a four-byte sequence satisfies this check, but
/// satisfying this check isn't sufficient for the byte to be
/// valid for those positions.
#[inline(always)]
const fn unconstrained_continuation(continuation: u8) -> bool {
    in_inclusive_range8(continuation, 0x80, 0xBF)
}

/// `true` iff `first` is a valid lead byte for a two-byte
/// sequence.
#[inline(always)]
const fn two_byte_lead(first: u8) -> bool {
    in_inclusive_range8(first, 0xC2, 0xDF)
}

/// `true` iff `first` is a valid lead byte for a three-byte
/// sequence.
#[inline(always)]
const fn three_byte_lead(first: u8) -> bool {
    in_inclusive_range8(first, 0xE0, 0xEF)
}

/// `true` iff `first` is a valid lead byte for a four-byte
/// sequence.
#[inline(always)]
const fn four_byte_lead(first: u8) -> bool {
    in_inclusive_range8(first, 0xF0, 0xF4)
}

/// `true` iff `first` and `second` form a valid two-byte UTF-8
/// sequence.
#[inline(always)]
const fn two_byte(first: u8, second: u8) -> bool {
    two_byte_lead(first) && unconstrained_continuation(second)
}

/// `true` iff `first`, `second`, and `third` form a valid three-byte UTF-8
/// sequence.
#[inline(always)]
const fn three_byte(first: u8, second: u8, third: u8) -> bool {
    three_byte_lead(first) && three_byte_prefix(first, second, third)
}

/// `true` iff `first`, `second`, `third`, and `fourth` form a valid
/// four-byte UTF-8 sequence.
#[inline(always)]
const fn four_byte(first: u8, second: u8, third: u8, fourth: u8) -> bool {
    !below_four_byte(first)
        && ((table_lookup(first, second) as u16)
            | (third >> 6) as u16
            | (((fourth & 0xC0) as u16) << 2)
            == 0x202)
}

/// `true` iff `first` is less than the lowest lead byte for a three-byte sequence.
#[inline(always)]
const fn below_three_byte(first: u8) -> bool {
    first < 0xE0
}

/// `true` iff `first` is less than the lowest lead byte for a four-byte sequence.
#[inline(always)]
const fn below_four_byte(first: u8) -> bool {
    first < 0xF0
}

/// Assuming that `first` is not ASCII, `true` iff
/// `first` and `second` for a two-byte prefix of a valid multibyte
/// UTF-8 sequence.
///
/// # Panics
///
/// With debug assertions enabled panics if `first` is ASCII.
#[inline(always)]
const fn two_byte_prefix(first: u8, second: u8) -> bool {
    table_lookup(first, second) == 0
}

/// Assuming that `first` is neither ASCII nor a two-byte lead, `true` iff
/// `first`, `second`, and `third` form either a valid three-byte UTF-8
/// sequence or a three-byte prefix of a four-byte UTF-8 sequence.
///
/// # Panics
///
/// With debug assertions enabled panics if `first` is either ASCII or a
/// two-byte lead.
#[inline(always)]
const fn three_byte_prefix(first: u8, second: u8, third: u8) -> bool {
    debug_assert!(!two_byte_lead(first));
    table_lookup(first, second) | (third >> 6) == 2
}

/// The table lookup for backing prefix checks.
#[inline(always)]
const fn table_lookup(first: u8, second: u8) -> u8 {
    UTF8_DATA.table[second as usize] & UTF8_DATA.table[first as usize + 0x80]
}

#[inline(always)]
const fn in_inclusive_range8(i: u8, start: u8, end: u8) -> bool {
    i.wrapping_sub(start) <= (end - start)
}

trait Seal {}

#[allow(private_bounds)] // allow sealing
pub trait Utf8ByteSequence: Seal + Copy + Clone {
    fn to_char(self) -> char;
    fn as_str(&self) -> &str;
}

// `Ascii` and `NonAscii` benefit from `pub enum` representation, because it communicates
// integer range information e.g. when they are used for table lookup.
//
// It's unclear if the other types benefit from communicating range information
// to the optimizer, but let's use the same pattern for consistency even if the
// use cases for `pub enum` aren't clear.

macro_rules! byte_methods {
    ($type_specific_ident:ident,
     $check:expr,
     $(#[$unchecked_meta:meta])*,
     $(#[$checked_meta:meta])*,
     $byte:ident,
    ) => {
        /// Whether `byte` is in range for this type.
        #[inline(always)]
        const fn is($byte: u8) -> bool {
            $check
        }

        $(#[$unchecked_meta])*
        #[inline(always)]
        pub const unsafe fn new_unchecked($type_specific_ident: u8) -> Self {
            debug_assert!(Self::is($type_specific_ident));
            // SAFETY: Our `repr` matches and the range is this function's safety invariant.
            unsafe { transmute($type_specific_ident) }
        }

        $(#[$checked_meta])*
        #[inline(always)]
        pub const fn try_new(byte: u8) -> Result<Self, Utf8ByteError> {
            if Self::is(byte) {
                // SAFETY: We checked the safety invariant immediately above.
                Ok(unsafe { Self::new_unchecked(byte) })
            } else {
                Err(Utf8ByteError)
            }
        }

        /// Convert to plain byte
        #[inline(always)]
        pub const fn to_u8(self) -> u8 {
            self as u8
        }
    };
}

/// A byte in the ASCII range (single-byte UTF-8 sequence).
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
#[repr(u8)]
pub enum Ascii {
    B00 = 0x00,
    B01 = 0x01,
    B02 = 0x02,
    B03 = 0x03,
    B04 = 0x04,
    B05 = 0x05,
    B06 = 0x06,
    B07 = 0x07,
    B08 = 0x08,
    B09 = 0x09,
    B0a = 0x0a,
    B0b = 0x0b,
    B0c = 0x0c,
    B0d = 0x0d,
    B0e = 0x0e,
    B0f = 0x0f,
    B10 = 0x10,
    B11 = 0x11,
    B12 = 0x12,
    B13 = 0x13,
    B14 = 0x14,
    B15 = 0x15,
    B16 = 0x16,
    B17 = 0x17,
    B18 = 0x18,
    B19 = 0x19,
    B1a = 0x1a,
    B1b = 0x1b,
    B1c = 0x1c,
    B1d = 0x1d,
    B1e = 0x1e,
    B1f = 0x1f,
    B20 = 0x20,
    B21 = 0x21,
    B22 = 0x22,
    B23 = 0x23,
    B24 = 0x24,
    B25 = 0x25,
    B26 = 0x26,
    B27 = 0x27,
    B28 = 0x28,
    B29 = 0x29,
    B2a = 0x2a,
    B2b = 0x2b,
    B2c = 0x2c,
    B2d = 0x2d,
    B2e = 0x2e,
    B2f = 0x2f,
    B30 = 0x30,
    B31 = 0x31,
    B32 = 0x32,
    B33 = 0x33,
    B34 = 0x34,
    B35 = 0x35,
    B36 = 0x36,
    B37 = 0x37,
    B38 = 0x38,
    B39 = 0x39,
    B3a = 0x3a,
    B3b = 0x3b,
    B3c = 0x3c,
    B3d = 0x3d,
    B3e = 0x3e,
    B3f = 0x3f,
    B40 = 0x40,
    B41 = 0x41,
    B42 = 0x42,
    B43 = 0x43,
    B44 = 0x44,
    B45 = 0x45,
    B46 = 0x46,
    B47 = 0x47,
    B48 = 0x48,
    B49 = 0x49,
    B4a = 0x4a,
    B4b = 0x4b,
    B4c = 0x4c,
    B4d = 0x4d,
    B4e = 0x4e,
    B4f = 0x4f,
    B50 = 0x50,
    B51 = 0x51,
    B52 = 0x52,
    B53 = 0x53,
    B54 = 0x54,
    B55 = 0x55,
    B56 = 0x56,
    B57 = 0x57,
    B58 = 0x58,
    B59 = 0x59,
    B5a = 0x5a,
    B5b = 0x5b,
    B5c = 0x5c,
    B5d = 0x5d,
    B5e = 0x5e,
    B5f = 0x5f,
    B60 = 0x60,
    B61 = 0x61,
    B62 = 0x62,
    B63 = 0x63,
    B64 = 0x64,
    B65 = 0x65,
    B66 = 0x66,
    B67 = 0x67,
    B68 = 0x68,
    B69 = 0x69,
    B6a = 0x6a,
    B6b = 0x6b,
    B6c = 0x6c,
    B6d = 0x6d,
    B6e = 0x6e,
    B6f = 0x6f,
    B70 = 0x70,
    B71 = 0x71,
    B72 = 0x72,
    B73 = 0x73,
    B74 = 0x74,
    B75 = 0x75,
    B76 = 0x76,
    B77 = 0x77,
    B78 = 0x78,
    B79 = 0x79,
    B7a = 0x7a,
    B7b = 0x7b,
    B7c = 0x7c,
    B7d = 0x7d,
    B7e = 0x7e,
    B7f = 0x7f,
}

impl Ascii {
    /// Minimum possible value.
    pub const MIN: Self = Self::B00;

    /// Maximum possible value.
    pub const MAX: Self = Self::B7f;

    byte_methods!(
        ascii,
        byte.is_ascii(),
        /// Unchecked constructor
        ///
        /// # Safety
        ///
        /// `ascii` must be < 0x80.
        ///
        /// # Panics
        ///
        /// When debug assertions are enabled, panics if the safety invariant
        /// is not upheld.
        ,
        /// Fallible constructor
        ///
        /// `Ok` if ASCII and `Err` otherwise.
        ,
        byte,
    );
}

impl TryFrom<u8> for Ascii {
    type Error = Utf8ByteError;

    fn try_from(byte: u8) -> Result<Self, Utf8ByteError> {
        Self::try_new(byte)
    }
}

impl From<Ascii> for u8 {
    fn from(ascii: Ascii) -> Self {
        ascii.to_u8()
    }
}

impl Seal for Ascii {}

impl Utf8ByteSequence for Ascii {
    #[inline(always)]
    fn to_char(self) -> char {
        self as u8 as char
    }

    #[inline(always)]
    fn as_str(&self) -> &str {
        // SAFETY: The construction of this type maintains the
        // invariant that `self` is in the ASCII range
        unsafe {
            core::str::from_utf8_unchecked(core::slice::from_ref(core::mem::transmute(&self)))
        }
    }
}

/// A byte in the non-ASCII range.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
#[repr(u8)]
pub enum NonAscii {
    B80 = 0x80,
    B81 = 0x81,
    B82 = 0x82,
    B83 = 0x83,
    B84 = 0x84,
    B85 = 0x85,
    B86 = 0x86,
    B87 = 0x87,
    B88 = 0x88,
    B89 = 0x89,
    B8a = 0x8a,
    B8b = 0x8b,
    B8c = 0x8c,
    B8d = 0x8d,
    B8e = 0x8e,
    B8f = 0x8f,
    B90 = 0x90,
    B91 = 0x91,
    B92 = 0x92,
    B93 = 0x93,
    B94 = 0x94,
    B95 = 0x95,
    B96 = 0x96,
    B97 = 0x97,
    B98 = 0x98,
    B99 = 0x99,
    B9a = 0x9a,
    B9b = 0x9b,
    B9c = 0x9c,
    B9d = 0x9d,
    B9e = 0x9e,
    B9f = 0x9f,
    Ba0 = 0xa0,
    Ba1 = 0xa1,
    Ba2 = 0xa2,
    Ba3 = 0xa3,
    Ba4 = 0xa4,
    Ba5 = 0xa5,
    Ba6 = 0xa6,
    Ba7 = 0xa7,
    Ba8 = 0xa8,
    Ba9 = 0xa9,
    Baa = 0xaa,
    Bab = 0xab,
    Bac = 0xac,
    Bad = 0xad,
    Bae = 0xae,
    Baf = 0xaf,
    Bb0 = 0xb0,
    Bb1 = 0xb1,
    Bb2 = 0xb2,
    Bb3 = 0xb3,
    Bb4 = 0xb4,
    Bb5 = 0xb5,
    Bb6 = 0xb6,
    Bb7 = 0xb7,
    Bb8 = 0xb8,
    Bb9 = 0xb9,
    Bba = 0xba,
    Bbb = 0xbb,
    Bbc = 0xbc,
    Bbd = 0xbd,
    Bbe = 0xbe,
    Bbf = 0xbf,
    Bc0 = 0xc0,
    Bc1 = 0xc1,
    Bc2 = 0xc2,
    Bc3 = 0xc3,
    Bc4 = 0xc4,
    Bc5 = 0xc5,
    Bc6 = 0xc6,
    Bc7 = 0xc7,
    Bc8 = 0xc8,
    Bc9 = 0xc9,
    Bca = 0xca,
    Bcb = 0xcb,
    Bcc = 0xcc,
    Bcd = 0xcd,
    Bce = 0xce,
    Bcf = 0xcf,
    Bd0 = 0xd0,
    Bd1 = 0xd1,
    Bd2 = 0xd2,
    Bd3 = 0xd3,
    Bd4 = 0xd4,
    Bd5 = 0xd5,
    Bd6 = 0xd6,
    Bd7 = 0xd7,
    Bd8 = 0xd8,
    Bd9 = 0xd9,
    Bda = 0xda,
    Bdb = 0xdb,
    Bdc = 0xdc,
    Bdd = 0xdd,
    Bde = 0xde,
    Bdf = 0xdf,
    Be0 = 0xe0,
    Be1 = 0xe1,
    Be2 = 0xe2,
    Be3 = 0xe3,
    Be4 = 0xe4,
    Be5 = 0xe5,
    Be6 = 0xe6,
    Be7 = 0xe7,
    Be8 = 0xe8,
    Be9 = 0xe9,
    Bea = 0xea,
    Beb = 0xeb,
    Bec = 0xec,
    Bed = 0xed,
    Bee = 0xee,
    Bef = 0xef,
    Bf0 = 0xf0,
    Bf1 = 0xf1,
    Bf2 = 0xf2,
    Bf3 = 0xf3,
    Bf4 = 0xf4,
    Bf5 = 0xf5,
    Bf6 = 0xf6,
    Bf7 = 0xf7,
    Bf8 = 0xf8,
    Bf9 = 0xf9,
    Bfa = 0xfa,
    Bfb = 0xfb,
    Bfc = 0xfc,
    Bfd = 0xfd,
    Bfe = 0xfe,
    Bff = 0xff,
}

impl NonAscii {
    /// Minimum possible value.
    pub const MIN: Self = Self::B80;

    /// Maximum possible value.
    pub const MAX: Self = Self::Bff;

    byte_methods!(
        non_ascii,
        !byte.is_ascii(),
        /// Unchecked constructor
        ///
        /// # Safety
        ///
        /// `non_ascii` must be >= 0x80.
        ///
        /// # Panics
        ///
        /// When debug assertions are enabled, panics if the safety invariant
        /// is not upheld.
        ,
        /// Fallible constructor
        ///
        /// `Ok` if non-ASCII and `Err` otherwise.
        ,
        byte,
    );
}

impl TryFrom<u8> for NonAscii {
    type Error = Utf8ByteError;

    fn try_from(byte: u8) -> Result<Self, Utf8ByteError> {
        Self::try_new(byte)
    }
}

impl From<NonAscii> for u8 {
    fn from(non_ascii: NonAscii) -> Self {
        non_ascii.to_u8()
    }
}

/// An unconstrained UTF-8 continuation byte (0x80 to 0xBF, inclusive)
///
/// Any continuation byte is valid as this type, but this type is not
/// sufficiently constrained to guarantee validity as the second byte
/// of a three-byte or four-byte sequence whose lead byte is 0xE0, 0xED,
/// 0xF0, or 0xF4.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
#[repr(u8)]
pub enum UnconstrainedContinuation {
    B80 = 0x80,
    B81 = 0x81,
    B82 = 0x82,
    B83 = 0x83,
    B84 = 0x84,
    B85 = 0x85,
    B86 = 0x86,
    B87 = 0x87,
    B88 = 0x88,
    B89 = 0x89,
    B8a = 0x8a,
    B8b = 0x8b,
    B8c = 0x8c,
    B8d = 0x8d,
    B8e = 0x8e,
    B8f = 0x8f,
    B90 = 0x90,
    B91 = 0x91,
    B92 = 0x92,
    B93 = 0x93,
    B94 = 0x94,
    B95 = 0x95,
    B96 = 0x96,
    B97 = 0x97,
    B98 = 0x98,
    B99 = 0x99,
    B9a = 0x9a,
    B9b = 0x9b,
    B9c = 0x9c,
    B9d = 0x9d,
    B9e = 0x9e,
    B9f = 0x9f,
    Ba0 = 0xa0,
    Ba1 = 0xa1,
    Ba2 = 0xa2,
    Ba3 = 0xa3,
    Ba4 = 0xa4,
    Ba5 = 0xa5,
    Ba6 = 0xa6,
    Ba7 = 0xa7,
    Ba8 = 0xa8,
    Ba9 = 0xa9,
    Baa = 0xaa,
    Bab = 0xab,
    Bac = 0xac,
    Bad = 0xad,
    Bae = 0xae,
    Baf = 0xaf,
    Bb0 = 0xb0,
    Bb1 = 0xb1,
    Bb2 = 0xb2,
    Bb3 = 0xb3,
    Bb4 = 0xb4,
    Bb5 = 0xb5,
    Bb6 = 0xb6,
    Bb7 = 0xb7,
    Bb8 = 0xb8,
    Bb9 = 0xb9,
    Bba = 0xba,
    Bbb = 0xbb,
    Bbc = 0xbc,
    Bbd = 0xbd,
    Bbe = 0xbe,
    Bbf = 0xbf,
}

impl UnconstrainedContinuation {
    /// Minimum possible value.
    pub const MIN: Self = Self::B80;

    /// Maximum possible value.
    pub const MAX: Self = Self::Bbf;

    byte_methods!(
        continuation,
        (byte >> 6) == 0b10,
        /// Unchecked constructor
        ///
        /// # Safety
        ///
        /// `continuation` must be in the range 0x80 to 0xBF, inclusive.
        ///
        /// # Panics
        ///
        /// When debug assertions are enabled, panics if the safety invariant
        /// is not upheld.
        ,
        /// Fallible constructor
        ///
        /// `Ok` if in the range 0x80 to 0xBF, inclusive, and `Err` otherwise.
        ,
        byte,
    );

    /// Obtain the least-significant six bits (i.e. the non-tag bits).
    ///
    /// # Safety-usable invariant
    ///
    /// Only the least-significant six bits can have non-zero values.
    #[inline(always)]
    pub const fn low_six(self) -> u8 {
        self.to_u8() & 0b111_111
    }

    /// Obtain the least-significant six bits (i.e. the non-tag bits) as `u32`.
    ///
    /// # Safety-usable invariant
    ///
    /// Only the least-significant six bits can have non-zero values.
    #[inline(always)]
    pub const fn low_six_u32(self) -> u32 {
        self.low_six() as u32
    }
}

/// A lead byte for a multibyte sequence (two-byte, three-byte, or
/// four-byte).
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
#[repr(u8)]
pub enum LeadForMultiByte {
    Bc2 = 0xc2,
    Bc3 = 0xc3,
    Bc4 = 0xc4,
    Bc5 = 0xc5,
    Bc6 = 0xc6,
    Bc7 = 0xc7,
    Bc8 = 0xc8,
    Bc9 = 0xc9,
    Bca = 0xca,
    Bcb = 0xcb,
    Bcc = 0xcc,
    Bcd = 0xcd,
    Bce = 0xce,
    Bcf = 0xcf,
    Bd0 = 0xd0,
    Bd1 = 0xd1,
    Bd2 = 0xd2,
    Bd3 = 0xd3,
    Bd4 = 0xd4,
    Bd5 = 0xd5,
    Bd6 = 0xd6,
    Bd7 = 0xd7,
    Bd8 = 0xd8,
    Bd9 = 0xd9,
    Bda = 0xda,
    Bdb = 0xdb,
    Bdc = 0xdc,
    Bdd = 0xdd,
    Bde = 0xde,
    Bdf = 0xdf,
    Be0 = 0xe0,
    Be1 = 0xe1,
    Be2 = 0xe2,
    Be3 = 0xe3,
    Be4 = 0xe4,
    Be5 = 0xe5,
    Be6 = 0xe6,
    Be7 = 0xe7,
    Be8 = 0xe8,
    Be9 = 0xe9,
    Bea = 0xea,
    Beb = 0xeb,
    Bec = 0xec,
    Bed = 0xed,
    Bee = 0xee,
    Bef = 0xef,
    Bf0 = 0xf0,
    Bf1 = 0xf1,
    Bf2 = 0xf2,
    Bf3 = 0xf3,
    Bf4 = 0xf4,
}

impl LeadForMultiByte {
    /// Minimum possible value.
    pub const MIN: Self = Self::Bc2;

    /// Maximum possible value.
    pub const MAX: Self = Self::Bf4;

    byte_methods!(
        lead,
        in_inclusive_range8(byte, 0xC2, 0xF4),
        /// Unchecked constructor
        ///
        /// # Safety
        ///
        /// `lead` must be in the range 0xC2 to 0xF4, inclusive.
        ///
        /// # Panics
        ///
        /// When debug assertions are enabled, panics if the safety invariant
        /// is not upheld.
        ,
        /// Fallible constructor
        ///
        /// `Ok` if in the range 0xC2 to 0xF4, inclusive, and `Err` otherwise.
        ,
        byte,
    );
}

/// A lead byte for a two-byte UTF-8 sequence (0xC2 to 0xDF, inclusive)
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
#[repr(u8)]
pub enum LeadForTwoByte {
    Bc2 = 0xc2,
    Bc3 = 0xc3,
    Bc4 = 0xc4,
    Bc5 = 0xc5,
    Bc6 = 0xc6,
    Bc7 = 0xc7,
    Bc8 = 0xc8,
    Bc9 = 0xc9,
    Bca = 0xca,
    Bcb = 0xcb,
    Bcc = 0xcc,
    Bcd = 0xcd,
    Bce = 0xce,
    Bcf = 0xcf,
    Bd0 = 0xd0,
    Bd1 = 0xd1,
    Bd2 = 0xd2,
    Bd3 = 0xd3,
    Bd4 = 0xd4,
    Bd5 = 0xd5,
    Bd6 = 0xd6,
    Bd7 = 0xd7,
    Bd8 = 0xd8,
    Bd9 = 0xd9,
    Bda = 0xda,
    Bdb = 0xdb,
    Bdc = 0xdc,
    Bdd = 0xdd,
    Bde = 0xde,
    Bdf = 0xdf,
}

impl LeadForTwoByte {
    /// Minimum possible value.
    pub const MIN: Self = Self::Bc2;

    /// Maximum possible value.
    pub const MAX: Self = Self::Bdf;

    byte_methods!(
        lead,
        in_inclusive_range8(byte, 0xC2, 0xDF),
        /// Unchecked constructor
        ///
        /// # Safety
        ///
        /// `lead` must be in the range 0xC2 to 0xDF, inclusive.
        ///
        /// # Panics
        ///
        /// When debug assertions are enabled, panics if the safety invariant
        /// is not upheld.
        ,
        /// Fallible constructor
        ///
        /// `Ok` if in the range 0xC2 to 0xDF, inclusive, and `Err` otherwise.
        ,
        byte,
    );

    /// Obtain the least-significant five bits (i.e. the non-tag bits).
    ///
    /// # Safety-usable invariant
    ///
    /// Only the least-significant five bits can have non-zero values.
    #[inline(always)]
    pub const fn low_five(self) -> u8 {
        self.to_u8() & 0b11_111
    }

    /// Obtain the least-significant five bits (i.e. the non-tag bits) as `u32`.
    ///
    /// # Safety-usable invariant
    ///
    /// Only the least-significant five bits can have non-zero values.
    #[inline(always)]
    pub const fn low_five_u32(self) -> u32 {
        self.low_five() as u32
    }
}

/// A lead byte for a three-byte UTF-8 sequence (0xE0 to 0xEF, inclusive)
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
#[repr(u8)]
pub enum LeadForThreeByte {
    Be0 = 0xe0,
    Be1 = 0xe1,
    Be2 = 0xe2,
    Be3 = 0xe3,
    Be4 = 0xe4,
    Be5 = 0xe5,
    Be6 = 0xe6,
    Be7 = 0xe7,
    Be8 = 0xe8,
    Be9 = 0xe9,
    Bea = 0xea,
    Beb = 0xeb,
    Bec = 0xec,
    Bed = 0xed,
    Bee = 0xee,
    Bef = 0xef,
}

impl LeadForThreeByte {
    /// Minimum possible value.
    pub const MIN: Self = Self::Be0;

    /// Maximum possible value.
    pub const MAX: Self = Self::Bef;

    byte_methods!(
        lead,
        in_inclusive_range8(byte, 0xE0, 0xEF),
        /// Unchecked constructor
        ///
        /// # Safety
        ///
        /// `lead` must be in the range 0xE0 to 0xEF, inclusive.
        ///
        /// # Panics
        ///
        /// When debug assertions are enabled, panics if the safety invariant
        /// is not upheld.
        ,
        /// Fallible constructor
        ///
        /// `Ok` if in the range 0xE0 to 0xEF, inclusive, and `Err` otherwise.
        ,
        byte,
    );

    /// Obtain the least-significant four bits (i.e. the non-tag bits).
    ///
    /// # Safety-usable invariant
    ///
    /// Only the least-significant four bits can have non-zero values.
    #[inline(always)]
    pub const fn low_four(self) -> u8 {
        self.to_u8() & 0b1111
    }

    /// Obtain the least-significant four bits (i.e. the non-tag bits) as `u32`.
    ///
    /// # Safety-usable invariant
    ///
    /// Only the least-significant four bits can have non-zero values.
    #[inline(always)]
    pub const fn low_four_u32(self) -> u32 {
        self.low_four() as u32
    }
}

/// A lead byte for a four-byte UTF-8 sequence (0xF0 to 0xF4, inclusive)
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
#[repr(u8)]
pub enum LeadForFourByte {
    Bf0 = 0xf0,
    Bf1 = 0xf1,
    Bf2 = 0xf2,
    Bf3 = 0xf3,
    Bf4 = 0xf4,
}

impl LeadForFourByte {
    /// Minimum possible value.
    pub const MIN: Self = Self::Bf0;

    /// Maximum possible value.
    pub const MAX: Self = Self::Bf4;

    byte_methods!(
        lead,
        in_inclusive_range8(byte, 0xF0, 0xF4),
        /// Unchecked constructor
        ///
        /// # Safety
        ///
        /// `lead` must be in the range 0xF0 to 0xF4, inclusive.
        ///
        /// # Panics
        ///
        /// When debug assertions are enabled, panics if the safety invariant
        /// is not upheld.
        ,
        /// Fallible constructor
        ///
        /// `Ok` if in the range 0xF0 to 0xF4, inclusive, and `Err` otherwise.
        ,
        byte,
    );

    /// Obtain the least-significant three bits (i.e. the non-tag bits).
    ///
    /// # Safety-usable invariant
    ///
    /// Only the least-significant three bits can have non-zero values.
    #[inline(always)]
    pub const fn low_three(self) -> u8 {
        self.to_u8() & 0b111
    }

    /// Obtain the least-significant three bits (i.e. the non-tag bits) as `u32`.
    ///
    /// # Safety-usable invariant
    ///
    /// Only the least-significant three bits can have non-zero values.
    #[inline(always)]
    pub const fn low_three_u32(self) -> u32 {
        self.low_three() as u32
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct TwoByteSequence {
    /// # Safety Usable Invariant
    ///
    /// The value is a well-formed two-byte UTF-8 sequence.
    bytes: [u8; 2],
}

impl TwoByteSequence {
    /// Checks whether the bytes are valid for this kind of sequence.
    #[inline(always)]
    pub const fn is(first: u8, second: u8) -> bool {
        LeadForTwoByte::is(first) && UnconstrainedContinuation::is(second)
    }

    /// Unchecked constructor
    ///
    /// # Safety
    ///
    /// `first` and `second` must form a two-byte UTF-8 sequence.
    ///
    /// # Panics
    ///
    /// When debug assertions are enabled, panics if the safety invariant
    /// is not upheld.
    #[inline(always)]
    pub const unsafe fn new_unchecked(first: u8, second: u8) -> Self {
        debug_assert!(Self::is(first, second));
        Self {
            bytes: [first, second],
        }
    }

    /// Fallible constructor
    ///
    /// `Ok` if `first` and `second` form a two-byte UTF-8 sequence and `Err` otherwise.
    #[inline(always)]
    pub const fn try_new(first: u8, second: u8) -> Result<Self, Utf8ByteError> {
        if Self::is(first, second) {
            // SAFETY: We checked the safety invariant immediately above.
            Ok(unsafe { Self::new_unchecked(first, second) })
        } else {
            Err(Utf8ByteError)
        }
    }

    /// Convert to typed byte tuple.
    #[inline(always)]
    pub const fn to_typed_bytes(self) -> (LeadForTwoByte, UnconstrainedContinuation) {
        // SAFETY: Re-interpretation is valid due to the invariant of `bytes`.
        unsafe {
            (
                LeadForTwoByte::new_unchecked(self.bytes[0]),
                UnconstrainedContinuation::new_unchecked(self.bytes[1]),
            )
        }
    }
}

impl Seal for TwoByteSequence {}

impl Utf8ByteSequence for TwoByteSequence {
    #[inline(always)]
    fn to_char(self) -> char {
        let (first, second) = self.to_typed_bytes();
        let scalar = (first.low_five_u32() << 6) | second.low_six_u32();
        debug_assert!(char::from_u32(scalar).is_some());
        // SAFETY: We distribute the bits in a way that forms
        // a valid scalar on the assumption that the bytes represent
        // a well-formed three-byte UTF-8 byte sequence.
        unsafe { char::from_u32_unchecked(scalar) }
    }

    #[inline(always)]
    fn as_str(&self) -> &str {
        // SAFETY: The construction of this type maintains the
        // invariant that `bytes` is a well-formed UTF-8 byte
        // sequence.
        unsafe { core::str::from_utf8_unchecked(&self.bytes) }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct ThreeByteSequence {
    /// # Safety Usable Invariant
    ///
    /// The value is a well-formed three-byte UTF-8 sequence.
    bytes: [u8; 3],
}

impl ThreeByteSequence {
    /// Unchecked constructor
    ///
    /// # Safety
    ///
    /// `first`, `second`, and `third` must form a three-byte UTF-8 sequence.
    ///
    /// # Panics
    ///
    /// When debug assertions are enabled, panics if the safety invariant
    /// is not upheld.
    #[inline(always)]
    pub const unsafe fn new_unchecked(first: u8, second: u8, third: u8) -> Self {
        debug_assert!(three_byte(first, second, third));
        Self {
            bytes: [first, second, third],
        }
    }

    /// Fallible constructor
    ///
    /// `Ok` if `first`, `second`, and `third` form a three-byte UTF-8 sequence and `Err` otherwise.
    #[inline(always)]
    pub const fn try_new(first: u8, second: u8, third: u8) -> Result<Self, Utf8ByteError> {
        if three_byte(first, second, third) {
            // SAFETY: We checked the safety invariant immediately above.
            Ok(unsafe { Self::new_unchecked(first, second, third) })
        } else {
            Err(Utf8ByteError)
        }
    }

    /// Convert to typed byte tuple.
    #[inline(always)]
    pub const fn to_typed_bytes(
        self,
    ) -> (
        LeadForThreeByte,
        UnconstrainedContinuation,
        UnconstrainedContinuation,
    ) {
        // SAFETY: Re-interpretation is valid due to the invariant of `bytes`.
        unsafe {
            (
                LeadForThreeByte::new_unchecked(self.bytes[0]),
                UnconstrainedContinuation::new_unchecked(self.bytes[1]),
                UnconstrainedContinuation::new_unchecked(self.bytes[2]),
            )
        }
    }
}

impl Seal for ThreeByteSequence {}

impl Utf8ByteSequence for ThreeByteSequence {
    #[inline(always)]
    fn to_char(self) -> char {
        let (first, second, third) = self.to_typed_bytes();
        let scalar =
            (first.low_four_u32() << 12) | (second.low_six_u32() << 6) | third.low_six_u32();
        debug_assert!(char::from_u32(scalar).is_some());
        // SAFETY: We distribute the bits in a way that forms
        // a valid scalar on the assumption that the bytes represent
        // a well-formed three-byte UTF-8 byte sequence.
        unsafe { char::from_u32_unchecked(scalar) }
    }

    #[inline(always)]
    fn as_str(&self) -> &str {
        // SAFETY: The construction of this type maintains the
        // invariant that `bytes` is a well-formed UTF-8 byte
        // sequence.
        unsafe { core::str::from_utf8_unchecked(&self.bytes) }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub struct FourByteSequence {
    /// # Safety Usable Invariant
    ///
    /// The value is a well-formed four-byte UTF-8 sequence.
    bytes: [u8; 4],
}

impl FourByteSequence {
    /// Unchecked constructor
    ///
    /// # Safety
    ///
    /// `first`, `second`, `third`, and `fourth` must form a four-byte UTF-8 sequence.
    ///
    /// # Panics
    ///
    /// When debug assertions are enabled, panics if the safety invariant
    /// is not upheld.
    #[inline(always)]
    pub const unsafe fn new_unchecked(first: u8, second: u8, third: u8, fourth: u8) -> Self {
        debug_assert!(four_byte(first, second, third, fourth));
        Self {
            bytes: [first, second, third, fourth],
        }
    }

    /// Fallible constructor
    ///
    /// `Ok` if `first`, `second`, `third`, and `fourth` form a four-byte UTF-8 sequence and `Err` otherwise.
    #[inline(always)]
    pub const fn try_new(
        first: u8,
        second: u8,
        third: u8,
        fourth: u8,
    ) -> Result<Self, Utf8ByteError> {
        if four_byte(first, second, third, fourth) {
            // SAFETY: We checked the safety invariant immediately above.
            Ok(unsafe { Self::new_unchecked(first, second, third, fourth) })
        } else {
            Err(Utf8ByteError)
        }
    }

    /// Convert to typed byte tuple.
    #[inline(always)]
    pub const fn to_typed_bytes(
        self,
    ) -> (
        LeadForFourByte,
        UnconstrainedContinuation,
        UnconstrainedContinuation,
        UnconstrainedContinuation,
    ) {
        // SAFETY: Re-interpretation is valid due to the invariant of `bytes`.
        unsafe {
            (
                LeadForFourByte::new_unchecked(self.bytes[0]),
                UnconstrainedContinuation::new_unchecked(self.bytes[1]),
                UnconstrainedContinuation::new_unchecked(self.bytes[2]),
                UnconstrainedContinuation::new_unchecked(self.bytes[3]),
            )
        }
    }
}

impl Seal for FourByteSequence {}

impl Utf8ByteSequence for FourByteSequence {
    #[inline(always)]
    fn to_char(self) -> char {
        let (first, second, third, fourth) = self.to_typed_bytes();
        let scalar = (first.low_three_u32() << 18)
            | (second.low_six_u32() << 12)
            | (third.low_six_u32() << 6)
            | fourth.low_six_u32();
        debug_assert!(char::from_u32(scalar).is_some());
        // SAFETY: We distribute the bits in a way that forms
        // a valid scalar on the assumption that the bytes represent
        // a well-formed four-byte UTF-8 byte sequence.
        unsafe { char::from_u32_unchecked(scalar) }
    }

    #[inline(always)]
    fn as_str(&self) -> &str {
        // SAFETY: The construction of this type maintains the
        // invariant that `bytes` is a well-formed UTF-8 byte
        // sequence.
        unsafe { core::str::from_utf8_unchecked(&self.bytes) }
    }
}

#[cfg(test)]
mod tests {

    fn two_byte_prefix_reference(first: u8, second: u8) -> bool {
        if !super::LeadForMultiByte::is(first) {
            return false;
        }
        let (lower_bound, upper_bound) = match first {
            0xE0 => (0xA0, 0xBF),
            0xED => (0x80, 0x9F),
            0xF0 => (0x90, 0xBF),
            0xF4 => (0x80, 0x8F),
            _ => (0x80, 0xBF),
        };
        super::in_inclusive_range8(second, lower_bound, upper_bound)
    }

    #[test]
    fn test_two_byte_prefix() {
        for first in 0x80..=0xFF {
            for second in 0..0xFF {
                assert_eq!(
                    super::two_byte_prefix(first, second),
                    two_byte_prefix_reference(first, second)
                );
            }
        }
    }
}

/*

#[unsafe(no_mangle)]
pub fn two_byte(first: u8, second: u8) -> bool {
    two_byte_lead(first) && unconstrained_continuation(second)
}

#[unsafe(no_mangle)]
pub fn three_byte(first: u8, second: u8, third: u8) -> bool {
    // Shifting masked `first` by 4 instead of 8 results
    // in a number to compare with that fits in an immediate
    // on aarch64. This is one instruction shorter on aarch64
    // that shifting the other part left the way it's shifted
    // in the four-byte case.
    ((first as u16 & 0b1111_0000) << 4) |
    ((table_lookup(first, second) | (third >> 6)) as u16) == 0b1110_0000_0010
}

#[unsafe(no_mangle)]
pub fn four_byte(first: u8, second: u8, third: u8, fourth: u8) -> bool {
    // We shift the combination of table lookup and third left, instead
    // of the other way round to make the comparison constant fit in
    // an immediate on aarch64.
    (fourth >> 6) as u16 | (first & 0b1111_1000) as u16 |
    (((table_lookup(first, second) | (third >> 6)) as u16) << 8) == 0b10_1111_0010
}
*/
