//! Java modified UTF-8 codec and [`NbtString`] representation.
//!
//! NBT strings encode Java `String` values (sequences of UTF-16 code units, which may include
//! U+0000 and unpaired surrogates) using Java's modified UTF-8 format with an unsigned 16-bit
//! encoded-byte length prefix (max 65,535 bytes).

use std::borrow::Cow;
use std::fmt;
use std::hash::{Hash, Hasher};

use crate::error::{Error, FormatError};

/// Maximum encoded byte length of an NBT string (unsigned 16-bit prefix limit).
pub const MAX_ENCODED_LEN: usize = u16::MAX as usize;

/// An owned NBT string capable of lossless round-tripping of any Java `String`,
/// including embedded NUL (`U+0000`), astral characters, and unpaired UTF-16 surrogates.
///
/// Internally stores a standard Rust [`String`] whenever the content is valid UTF-8, and
/// falls back to UTF-16 code units only when unpaired surrogates are present.
#[derive(Clone, Eq)]
pub struct NbtString {
    repr: Repr,
}

#[derive(Clone, PartialEq, Eq)]
enum Repr {
    /// Valid UTF-8 string (contains no unpaired surrogates).
    Utf8(String),
    /// Sequence of UTF-16 code units guaranteed to contain at least one unpaired surrogate.
    Wtf16(Vec<u16>),
}

impl NbtString {
    /// Creates a new empty `NbtString`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            repr: Repr::Utf8(String::new()),
        }
    }

    /// Creates an `NbtString` from UTF-16 code units.
    ///
    /// Normalizes to the UTF-8 representation if all surrogates are validly paired.
    #[must_use]
    pub fn from_utf16(units: impl Into<Vec<u16>>) -> Self {
        let units = units.into();
        match String::from_utf16(&units) {
            Ok(s) => Self {
                repr: Repr::Utf8(s),
            },
            Err(_) => Self {
                repr: Repr::Wtf16(units),
            },
        }
    }

    /// Returns the underlying `&str` if the string contains no unpaired surrogates.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match &self.repr {
            Repr::Utf8(s) => Some(s.as_str()),
            Repr::Wtf16(_) => None,
        }
    }

    /// Converts the string into a Rust `String` if it contains no unpaired surrogates.
    pub fn into_string(self) -> Result<String, Self> {
        match self.repr {
            Repr::Utf8(s) => Ok(s),
            Repr::Wtf16(u) => Err(Self {
                repr: Repr::Wtf16(u),
            }),
        }
    }

    /// Returns a UTF-8 view, replacing any unpaired surrogates with U+FFFD.
    #[must_use]
    pub fn to_string_lossy(&self) -> Cow<'_, str> {
        match &self.repr {
            Repr::Utf8(s) => Cow::Borrowed(s.as_str()),
            Repr::Wtf16(u) => Cow::Owned(String::from_utf16_lossy(u)),
        }
    }

    /// Returns `true` if the string is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match &self.repr {
            Repr::Utf8(s) => s.is_empty(),
            Repr::Wtf16(u) => u.is_empty(),
        }
    }

    /// Returns the length in UTF-16 code units, matching Java's `String.length()`.
    #[must_use]
    pub fn utf16_len(&self) -> usize {
        match &self.repr {
            Repr::Utf8(s) => {
                if s.is_ascii() {
                    s.len()
                } else {
                    s.encode_utf16().count()
                }
            }
            Repr::Wtf16(u) => u.len(),
        }
    }

    /// Returns all UTF-16 code units of this string.
    #[must_use]
    pub fn to_utf16(&self) -> Vec<u16> {
        match &self.repr {
            Repr::Utf8(s) => s.encode_utf16().collect(),
            Repr::Wtf16(u) => u.clone(),
        }
    }

    /// Computes Java's `String.hashCode()` over UTF-16 code units.
    #[must_use]
    pub fn java_hash_code(&self) -> i32 {
        match &self.repr {
            Repr::Utf8(s) => java_hash_str(s),
            Repr::Wtf16(u) => java_hash_utf16(u.iter().copied()),
        }
    }

    /// Computes the exact encoded byte length in Java modified UTF-8.
    #[must_use]
    pub fn mutf8_len(&self) -> usize {
        match &self.repr {
            Repr::Utf8(s) => mutf8_len_utf16(s.encode_utf16()),
            Repr::Wtf16(u) => mutf8_len_utf16(u.iter().copied()),
        }
    }

    /// Encodes this string as Java modified UTF-8 bytes (without the 2-byte length prefix).
    ///
    /// Fails with [`Error::StringTooLong`] if the encoded form exceeds [`MAX_ENCODED_LEN`]
    /// (65,535 bytes).
    pub fn encode_mutf8(&self) -> Result<Vec<u8>, Error> {
        match &self.repr {
            Repr::Utf8(s) => encode_mutf8_utf16(s.encode_utf16()),
            Repr::Wtf16(u) => encode_mutf8_utf16(u.iter().copied()),
        }
    }

    /// Decodes Java modified UTF-8 bytes into an `NbtString`.
    pub fn decode_mutf8(bytes: &[u8]) -> Result<Self, Error> {
        decode_mutf8(bytes)
    }
}

impl Default for NbtString {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for NbtString {
    fn eq(&self, other: &Self) -> bool {
        // Because `Wtf16` is strictly normalized (only constructed when at least one
        // lone surrogate is present), `Utf8` and `Wtf16` can never represent the same
        // sequence of UTF-16 code units.
        self.repr == other.repr
    }
}

impl PartialEq<str> for NbtString {
    fn eq(&self, other: &str) -> bool {
        matches!(&self.repr, Repr::Utf8(s) if s == other)
    }
}

impl PartialEq<&str> for NbtString {
    fn eq(&self, other: &&str) -> bool {
        self == *other
    }
}

impl PartialEq<String> for NbtString {
    fn eq(&self, other: &String) -> bool {
        self == other.as_str()
    }
}

impl Hash for NbtString {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match &self.repr {
            Repr::Utf8(s) => {
                0u8.hash(state);
                s.hash(state);
            }
            Repr::Wtf16(u) => {
                1u8.hash(state);
                u.hash(state);
            }
        }
    }
}

impl fmt::Debug for NbtString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.repr {
            Repr::Utf8(s) => fmt::Debug::fmt(s, f),
            Repr::Wtf16(units) => {
                f.write_str("\"")?;
                for decoded in char::decode_utf16(units.iter().copied()) {
                    match decoded {
                        Ok(c) => {
                            for esc in c.escape_debug() {
                                fmt::Write::write_char(f, esc)?;
                            }
                        }
                        Err(surrogate) => {
                            write!(f, "\\u{{{:04X}}}", surrogate.unpaired_surrogate())?;
                        }
                    }
                }
                f.write_str("\"")
            }
        }
    }
}

impl fmt::Display for NbtString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_lossy())
    }
}

impl From<String> for NbtString {
    fn from(s: String) -> Self {
        Self {
            repr: Repr::Utf8(s),
        }
    }
}

impl From<&str> for NbtString {
    fn from(s: &str) -> Self {
        Self {
            repr: Repr::Utf8(s.to_owned()),
        }
    }
}

impl From<Cow<'_, str>> for NbtString {
    fn from(s: Cow<'_, str>) -> Self {
        Self {
            repr: Repr::Utf8(s.into_owned()),
        }
    }
}

/// Borrowed view of either a Rust `&str` or an [`NbtString`], for zero-copy binary writing.
pub trait AsNbtString {
    /// Encodes the string to Java modified UTF-8 bytes, failing if over 65,535 bytes.
    fn encode_mutf8(&self) -> Result<Vec<u8>, Error>;
}

impl AsNbtString for str {
    fn encode_mutf8(&self) -> Result<Vec<u8>, Error> {
        encode_mutf8_utf16(self.encode_utf16())
    }
}

impl AsNbtString for &str {
    fn encode_mutf8(&self) -> Result<Vec<u8>, Error> {
        encode_mutf8_utf16(self.encode_utf16())
    }
}

impl AsNbtString for String {
    fn encode_mutf8(&self) -> Result<Vec<u8>, Error> {
        encode_mutf8_utf16(self.encode_utf16())
    }
}

impl AsNbtString for &String {
    fn encode_mutf8(&self) -> Result<Vec<u8>, Error> {
        encode_mutf8_utf16(self.encode_utf16())
    }
}

impl AsNbtString for NbtString {
    fn encode_mutf8(&self) -> Result<Vec<u8>, Error> {
        NbtString::encode_mutf8(self)
    }
}

impl AsNbtString for &NbtString {
    fn encode_mutf8(&self) -> Result<Vec<u8>, Error> {
        NbtString::encode_mutf8(self)
    }
}

/// Computes Java's `String.hashCode()` for a UTF-8 `&str`.
#[must_use]
pub fn java_hash_str(s: &str) -> i32 {
    if s.is_ascii() {
        let mut h: i32 = 0;
        for &b in s.as_bytes() {
            h = h.wrapping_mul(31).wrapping_add(i32::from(b));
        }
        h
    } else {
        java_hash_utf16(s.encode_utf16())
    }
}

/// Computes Java's `String.hashCode()` for an iterator of UTF-16 code units.
#[must_use]
pub fn java_hash_utf16(units: impl IntoIterator<Item = u16>) -> i32 {
    let mut h: i32 = 0;
    for u in units {
        h = h.wrapping_mul(31).wrapping_add(i32::from(u));
    }
    h
}

/// Returns the Java modified UTF-8 encoded length for a single UTF-16 code unit.
#[inline]
const fn mutf8_unit_len(u: u16) -> usize {
    if u >= 0x0001 && u <= 0x007F {
        1
    } else if u <= 0x07FF {
        // Includes U+0000 (encoded as 0xC0 0x80) and 0x0080..=0x07FF.
        2
    } else {
        3
    }
}

fn mutf8_len_utf16(units: impl IntoIterator<Item = u16>) -> usize {
    units.into_iter().map(mutf8_unit_len).sum()
}

/// Encodes an iterator of UTF-16 code units into Java modified UTF-8.
///
/// Fails with [`Error::StringTooLong`] if the resulting byte length exceeds [`MAX_ENCODED_LEN`]
/// (65,535 bytes); never truncates.
pub fn encode_mutf8_utf16(units: impl IntoIterator<Item = u16> + Clone) -> Result<Vec<u8>, Error> {
    let encoded_len = mutf8_len_utf16(units.clone());
    if encoded_len > MAX_ENCODED_LEN {
        return Err(Error::StringTooLong { encoded_len });
    }
    let mut out = Vec::with_capacity(encoded_len);
    for u in units {
        if (0x0001..=0x007F).contains(&u) {
            out.push(u as u8);
        } else if u <= 0x07FF {
            out.push((0xC0 | ((u >> 6) & 0x1F)) as u8);
            out.push((0x80 | (u & 0x3F)) as u8);
        } else {
            out.push((0xE0 | ((u >> 12) & 0x0F)) as u8);
            out.push((0x80 | ((u >> 6) & 0x3F)) as u8);
            out.push((0x80 | (u & 0x3F)) as u8);
        }
    }
    debug_assert_eq!(out.len(), encoded_len);
    Ok(out)
}

/// Encodes a Rust `&str` into Java modified UTF-8 bytes.
pub fn encode_mutf8(s: &str) -> Result<Vec<u8>, Error> {
    encode_mutf8_utf16(s.encode_utf16())
}

/// Decodes Java modified UTF-8 bytes according to Java SE `DataInput.readUTF()` rules.
///
/// - `0xxxxxxx` → 1-byte code unit `(b & 0x7F)`
/// - `110xxxxx 10xxxxxx` → 2-byte code unit `((a & 0x1F) << 6) | (b & 0x3F)`
/// - `1110xxxx 10xxxxxx 10xxxxxx` → 3-byte code unit `((a & 0x0F) << 12) | ((b & 0x3F) << 6) | (c & 0x3F)`
/// - `10xxxxxx` or `1111xxxx`, truncated multi-byte sequences, or invalid continuation bytes fail.
pub fn decode_mutf8(bytes: &[u8]) -> Result<NbtString, Error> {
    // Fast path: pure ASCII with no NUL bytes is identical in UTF-8 and modified UTF-8.
    if bytes.iter().all(|&b| (0x01..=0x7F).contains(&b)) {
        let s = std::str::from_utf8(bytes)
            .expect("ASCII bytes are always valid UTF-8")
            .to_owned();
        return Ok(NbtString::from(s));
    }

    let mut units = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b1 = bytes[i];
        match b1 >> 4 {
            0..=7 => {
                // 0xxxxxxx
                units.push(u16::from(b1));
                i += 1;
            }
            12 | 13 => {
                // 110xxxxx 10xxxxxx
                if i + 1 >= bytes.len() {
                    return Err(
                        FormatError::InvalidModifiedUtf8("truncated 2-byte sequence").into(),
                    );
                }
                let b2 = bytes[i + 1];
                if (b2 & 0xC0) != 0x80 {
                    return Err(FormatError::InvalidModifiedUtf8(
                        "invalid continuation byte in 2-byte sequence",
                    )
                    .into());
                }
                let code_unit = (u16::from(b1 & 0x1F) << 6) | u16::from(b2 & 0x3F);
                units.push(code_unit);
                i += 2;
            }
            14 => {
                // 1110xxxx 10xxxxxx 10xxxxxx
                if i + 2 >= bytes.len() {
                    return Err(
                        FormatError::InvalidModifiedUtf8("truncated 3-byte sequence").into(),
                    );
                }
                let b2 = bytes[i + 1];
                let b3 = bytes[i + 2];
                if (b2 & 0xC0) != 0x80 || (b3 & 0xC0) != 0x80 {
                    return Err(FormatError::InvalidModifiedUtf8(
                        "invalid continuation byte in 3-byte sequence",
                    )
                    .into());
                }
                let code_unit = (u16::from(b1 & 0x0F) << 12)
                    | (u16::from(b2 & 0x3F) << 6)
                    | u16::from(b3 & 0x3F);
                units.push(code_unit);
                i += 3;
            }
            _ => {
                // 10xxxxxx (8..=11) or 1111xxxx (15)
                return Err(FormatError::InvalidModifiedUtf8(
                    "invalid leading byte (continuation or 4-byte sequence)",
                )
                .into());
            }
        }
    }

    Ok(NbtString::from_utf16(units))
}
