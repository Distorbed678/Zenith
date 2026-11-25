//! Core NBT [`Tag`] and [`TagType`] definitions covering all 13 NBT tag types.

use crate::compound::Compound;
use crate::error::{Error, FormatError};
use crate::list::List;
use crate::mutf8::NbtString;

/// Discriminant for all 13 binary NBT tag types (`0..=12`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TagType {
    /// `0` — End marker (compound terminator, empty list element type, or absent network root).
    End = 0,
    /// `1` — Signed 8-bit integer.
    Byte = 1,
    /// `2` — Signed 16-bit big-endian integer.
    Short = 2,
    /// `3` — Signed 32-bit big-endian integer.
    Int = 3,
    /// `4` — Signed 64-bit big-endian integer.
    Long = 4,
    /// `5` — 32-bit IEEE-754 float.
    Float = 5,
    /// `6` — 64-bit IEEE-754 double.
    Double = 6,
    /// `7` — Length-prefixed array of signed bytes.
    ByteArray = 7,
    /// `8` — Length-prefixed Java modified UTF-8 string.
    String = 8,
    /// `9` — List of tags.
    List = 9,
    /// `10` — Map of string keys to tags.
    Compound = 10,
    /// `11` — Length-prefixed array of 32-bit signed integers.
    IntArray = 11,
    /// `12` — Length-prefixed array of 64-bit signed integers.
    LongArray = 12,
}

impl TagType {
    /// Converts a wire tag type ID byte into a [`TagType`].
    pub const fn from_id(id: u8) -> Result<Self, FormatError> {
        match id {
            0 => Ok(Self::End),
            1 => Ok(Self::Byte),
            2 => Ok(Self::Short),
            3 => Ok(Self::Int),
            4 => Ok(Self::Long),
            5 => Ok(Self::Float),
            6 => Ok(Self::Double),
            7 => Ok(Self::ByteArray),
            8 => Ok(Self::String),
            9 => Ok(Self::List),
            10 => Ok(Self::Compound),
            11 => Ok(Self::IntArray),
            12 => Ok(Self::LongArray),
            other => Err(FormatError::UnknownTagType(other)),
        }
    }

    /// Returns the wire tag type ID byte (`0..=12`).
    #[must_use]
    pub const fn id(self) -> u8 {
        self as u8
    }
}

/// An owned NBT value covering all 13 tag types.
///
/// ## Float / Double equality
///
/// [`PartialEq`] and [`Eq`] compare [`Tag::Float`] and [`Tag::Double`] by their exact IEEE-754
/// bit patterns (`to_bits()`), so `-0.0` is distinct from `+0.0` and NaN payloads compare equal
/// when their bit patterns match. This aligns with binary round-trip fidelity.
#[derive(Debug, Clone)]
pub enum Tag {
    /// Tag ID `0` — End marker.
    End,
    /// Tag ID `1` — Signed 8-bit integer.
    Byte(i8),
    /// Tag ID `2` — Signed 16-bit integer.
    Short(i16),
    /// Tag ID `3` — Signed 32-bit integer.
    Int(i32),
    /// Tag ID `4` — Signed 64-bit integer.
    Long(i64),
    /// Tag ID `5` — 32-bit IEEE-754 float (bit pattern preserved).
    Float(f32),
    /// Tag ID `6` — 64-bit IEEE-754 double (bit pattern preserved).
    Double(f64),
    /// Tag ID `7` — Array of signed 8-bit integers.
    ByteArray(Vec<i8>),
    /// Tag ID `8` — Java modified UTF-8 string.
    String(NbtString),
    /// Tag ID `9` — Ordered (possibly heterogeneous) list of tags.
    List(List),
    /// Tag ID `10` — Key-value map with Java `HashMap` iteration order.
    Compound(Compound),
    /// Tag ID `11` — Array of 32-bit signed integers.
    IntArray(Vec<i32>),
    /// Tag ID `12` — Array of 64-bit signed integers.
    LongArray(Vec<i64>),
}

impl Tag {
    /// Returns the [`TagType`] discriminant of this tag.
    #[must_use]
    pub const fn tag_type(&self) -> TagType {
        match self {
            Self::End => TagType::End,
            Self::Byte(_) => TagType::Byte,
            Self::Short(_) => TagType::Short,
            Self::Int(_) => TagType::Int,
            Self::Long(_) => TagType::Long,
            Self::Float(_) => TagType::Float,
            Self::Double(_) => TagType::Double,
            Self::ByteArray(_) => TagType::ByteArray,
            Self::String(_) => TagType::String,
            Self::List(_) => TagType::List,
            Self::Compound(_) => TagType::Compound,
            Self::IntArray(_) => TagType::IntArray,
            Self::LongArray(_) => TagType::LongArray,
        }
    }

    /// Returns `Some(v)` if this tag is [`Tag::Byte`].
    #[must_use]
    pub const fn as_byte(&self) -> Option<i8> {
        match self {
            Self::Byte(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns `Some(v)` if this tag is [`Tag::Short`].
    #[must_use]
    pub const fn as_short(&self) -> Option<i16> {
        match self {
            Self::Short(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns `Some(v)` if this tag is [`Tag::Int`].
    #[must_use]
    pub const fn as_int(&self) -> Option<i32> {
        match self {
            Self::Int(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns `Some(v)` if this tag is [`Tag::Long`].
    #[must_use]
    pub const fn as_long(&self) -> Option<i64> {
        match self {
            Self::Long(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns `Some(v)` if this tag is [`Tag::Float`].
    #[must_use]
    pub const fn as_float(&self) -> Option<f32> {
        match self {
            Self::Float(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns `Some(v)` if this tag is [`Tag::Double`].
    #[must_use]
    pub const fn as_double(&self) -> Option<f64> {
        match self {
            Self::Double(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns `Some(slice)` if this tag is [`Tag::ByteArray`].
    #[must_use]
    pub fn as_byte_array(&self) -> Option<&[i8]> {
        match self {
            Self::ByteArray(v) => Some(v.as_slice()),
            _ => None,
        }
    }

    /// Returns `Some(s)` if this tag is [`Tag::String`] and contains valid UTF-8.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => s.as_str(),
            _ => None,
        }
    }

    /// Returns `Some(s)` if this tag is [`Tag::String`].
    #[must_use]
    pub const fn as_nbt_string(&self) -> Option<&NbtString> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    /// Returns `Some(list)` if this tag is [`Tag::List`].
    #[must_use]
    pub const fn as_list(&self) -> Option<&List> {
        match self {
            Self::List(l) => Some(l),
            _ => None,
        }
    }

    /// Returns `Some(compound)` if this tag is [`Tag::Compound`].
    #[must_use]
    pub const fn as_compound(&self) -> Option<&Compound> {
        match self {
            Self::Compound(c) => Some(c),
            _ => None,
        }
    }

    /// Returns `Some(compound)` mutably if this tag is [`Tag::Compound`].
    #[must_use]
    pub fn as_compound_mut(&mut self) -> Option<&mut Compound> {
        match self {
            Self::Compound(c) => Some(c),
            _ => None,
        }
    }

    /// Returns `Some(slice)` if this tag is [`Tag::IntArray`].
    #[must_use]
    pub fn as_int_array(&self) -> Option<&[i32]> {
        match self {
            Self::IntArray(v) => Some(v.as_slice()),
            _ => None,
        }
    }

    /// Returns `Some(slice)` if this tag is [`Tag::LongArray`].
    #[must_use]
    pub fn as_long_array(&self) -> Option<&[i64]> {
        match self {
            Self::LongArray(v) => Some(v.as_slice()),
            _ => None,
        }
    }

    /// Consumes `self` and returns the inner [`Compound`], or an [`Error::Format`] mismatch.
    pub fn into_compound(self) -> Result<Compound, Error> {
        match self {
            Self::Compound(c) => Ok(c),
            other => Err(FormatError::UnexpectedTagType {
                expected: TagType::Compound,
                found: other.tag_type(),
            }
            .into()),
        }
    }
}

impl PartialEq for Tag {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::End, Self::End) => true,
            (Self::Byte(a), Self::Byte(b)) => a == b,
            (Self::Short(a), Self::Short(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Long(a), Self::Long(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => a.to_bits() == b.to_bits(),
            (Self::Double(a), Self::Double(b)) => a.to_bits() == b.to_bits(),
            (Self::ByteArray(a), Self::ByteArray(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::List(a), Self::List(b)) => a == b,
            (Self::Compound(a), Self::Compound(b)) => a == b,
            (Self::IntArray(a), Self::IntArray(b)) => a == b,
            (Self::LongArray(a), Self::LongArray(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for Tag {}

impl From<bool> for Tag {
    fn from(v: bool) -> Self {
        Self::Byte(i8::from(v))
    }
}

impl From<i8> for Tag {
    fn from(v: i8) -> Self {
        Self::Byte(v)
    }
}

impl From<i16> for Tag {
    fn from(v: i16) -> Self {
        Self::Short(v)
    }
}

impl From<i32> for Tag {
    fn from(v: i32) -> Self {
        Self::Int(v)
    }
}

impl From<i64> for Tag {
    fn from(v: i64) -> Self {
        Self::Long(v)
    }
}

impl From<f32> for Tag {
    fn from(v: f32) -> Self {
        Self::Float(v)
    }
}

impl From<f64> for Tag {
    fn from(v: f64) -> Self {
        Self::Double(v)
    }
}

impl From<String> for Tag {
    fn from(v: String) -> Self {
        Self::String(NbtString::from(v))
    }
}

impl From<&str> for Tag {
    fn from(v: &str) -> Self {
        Self::String(NbtString::from(v))
    }
}

impl From<NbtString> for Tag {
    fn from(v: NbtString) -> Self {
        Self::String(v)
    }
}

impl From<List> for Tag {
    fn from(v: List) -> Self {
        Self::List(v)
    }
}

impl From<Compound> for Tag {
    fn from(v: Compound) -> Self {
        Self::Compound(v)
    }
}
