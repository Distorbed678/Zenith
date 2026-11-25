//! Formatting style constants for the plain single-line SNBT printer.
//!
//! All separator spacing, numeric suffixes, and floating-point formatting live in this module
//! so they can be verified and pinned in one place during the M2 parity phase.

// PARITY: pinned by snbt-format oracle fixtures (M2 parity phase)

use std::fmt::Write;

/// Separator between a compound key and its value (`key:value`).
pub const COMPOUND_KV_SEP: &str = ":";

/// Separator between consecutive entries in a compound (`a:1,b:2`).
pub const COMPOUND_ENTRY_SEP: &str = ",";

/// Separator between consecutive elements in a list (`[1,2]`).
pub const LIST_ELEM_SEP: &str = ",";

/// Prefix for byte arrays (`[B;`).
pub const BYTE_ARRAY_PREFIX: &str = "[B;";

/// Prefix for int arrays (`[I;`).
pub const INT_ARRAY_PREFIX: &str = "[I;";

/// Prefix for long arrays (`[L;`).
pub const LONG_ARRAY_PREFIX: &str = "[L;";

/// Separator between consecutive elements in typed arrays.
pub const ARRAY_ELEM_SEP: &str = ",";

/// Suffix for standalone `Tag::Byte` values.
pub const BYTE_SUFFIX: char = 'b';

/// Suffix for elements inside `Tag::ByteArray`.
pub const BYTE_ARRAY_ELEM_SUFFIX: char = 'B';

/// Suffix for `Tag::Short` values.
pub const SHORT_SUFFIX: char = 's';

/// Suffix for `Tag::Long` values and `Tag::LongArray` elements.
pub const LONG_SUFFIX: char = 'L';

/// Suffix for `Tag::Float` values.
pub const FLOAT_SUFFIX: char = 'f';

/// Suffix for `Tag::Double` values.
pub const DOUBLE_SUFFIX: char = 'd';

/// Whether `\x` hex control-character escapes use lowercase hex digits (`\x00` vs `\x00`).
pub const HEX_ESCAPE_LOWERCASE: bool = true;

/// Formats a `f32` value with its SNBT suffix: Java's `Float.toString` (see [`crate::java_number`]) + `f`.
pub fn write_float(out: &mut String, v: f32) {
    crate::java_number::write_java_float(out, v);
    out.push(FLOAT_SUFFIX);
}

/// Formats an `f64` value with its SNBT suffix: Java's `Double.toString` (see [`crate::java_number`]) + `d`.
pub fn write_double(out: &mut String, v: f64) {
    crate::java_number::write_java_double(out, v);
    out.push(DOUBLE_SUFFIX);
}

/// Writes a `\xXX` control-character escape for `byte < 0x20`.
pub fn write_hex_escape(out: &mut String, byte: u8) {
    if HEX_ESCAPE_LOWERCASE {
        let _ = write!(out, "\\x{byte:02x}");
    } else {
        let _ = write!(out, "\\x{byte:02X}");
    }
}
