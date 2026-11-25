//! Plain single-line SNBT printer following `docs/specs/nbt/snbt.md`.

use std::fmt::Write;

use crate::compound::Compound;
use crate::error::Error;
use crate::list::List;
use crate::mutf8::NbtString;
use crate::snbt::style;
use crate::tag::Tag;

/// Formats a [`Tag`] as a plain single-line SNBT string.
///
/// Fails with [`Error::UnsupportedTreeifiedBin`] if any nested [`Compound`] has reached
/// an unsupported treeified-bin state.
pub fn to_snbt(tag: &Tag) -> Result<String, Error> {
    let mut out = String::new();
    write_tag(&mut out, tag)?;
    Ok(out)
}

/// Returns `true` if `key` may be printed bare (unquoted) in a compound according to
/// `docs/specs/nbt/snbt.md`:
/// starts with an ASCII letter, `.` or `_`; continues with ASCII letters, digits, `.`, `_`,
/// `+`, `-`; and is not `true` or `false` (case-insensitive).
#[must_use]
pub fn is_bare_key(key: &NbtString) -> bool {
    let Some(s) = key.as_str() else {
        return false;
    };
    let mut bytes = s.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == b'.' || first == b'_') {
        return false;
    }
    if !bytes.all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'+' || b == b'-')
    {
        return false;
    }
    !(s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("false"))
}

/// Formats an [`NbtString`] as a quoted SNBT string literal using the content-driven
/// quote selection and control-character escape rules from `docs/specs/nbt/snbt.md`.
pub fn write_quoted_string(out: &mut String, s: &NbtString) {
    let quote = choose_quote(s);
    out.push(quote);
    if let Some(utf8) = s.as_str() {
        for ch in utf8.chars() {
            write_escaped_char(out, ch, quote);
        }
    } else {
        for decoded in char::decode_utf16(s.to_utf16()) {
            match decoded {
                Ok(ch) => write_escaped_char(out, ch, quote),
                Err(surrogate) => {
                    let _ = write!(out, "\\u{:04x}", surrogate.unpaired_surrogate());
                }
            }
        }
    }
    out.push(quote);
}

fn choose_quote(s: &NbtString) -> char {
    if let Some(utf8) = s.as_str() {
        for ch in utf8.chars() {
            if ch == '"' {
                return '\'';
            }
            if ch == '\'' {
                return '"';
            }
        }
        return '"';
    }
    for u in s.to_utf16() {
        if u == u16::from(b'"') {
            return '\'';
        }
        if u == u16::from(b'\'') {
            return '"';
        }
    }
    '"'
}

fn write_escaped_char(out: &mut String, ch: char, quote: char) {
    match ch {
        '\\' => out.push_str("\\\\"),
        c if c == quote => {
            out.push('\\');
            out.push(quote);
        }
        '\u{0008}' => out.push_str("\\b"),
        '\t' => out.push_str("\\t"),
        '\n' => out.push_str("\\n"),
        '\u{000C}' => out.push_str("\\f"),
        '\r' => out.push_str("\\r"),
        c if (c as u32) < 0x20 => {
            style::write_hex_escape(out, c as u8);
        }
        c => out.push(c),
    }
}

fn write_key(out: &mut String, key: &NbtString) {
    if is_bare_key(key) {
        out.push_str(key.as_str().expect("bare key is always valid UTF-8"));
    } else {
        write_quoted_string(out, key);
    }
}

fn write_compound(out: &mut String, compound: &Compound) -> Result<(), Error> {
    // The text form sorts keys by Java's natural string order (UTF-16 code units), unlike the binary form, which
    // writes hash-map order (spec: snbt.md, reader answer Q1). Sorting by UTF-16 units differs from `str` ordering
    // for characters above U+FFFF.
    let mut entries: Vec<(Vec<u16>, &NbtString, &Tag)> =
        compound.iter().map(|(k, v)| (k.to_utf16(), k, v)).collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    out.push('{');
    for (idx, (_, k, v)) in entries.into_iter().enumerate() {
        if idx > 0 {
            out.push_str(style::COMPOUND_ENTRY_SEP);
        }
        write_key(out, k);
        out.push_str(style::COMPOUND_KV_SEP);
        write_tag(out, v)?;
    }
    out.push('}');
    Ok(())
}

fn write_list(out: &mut String, list: &List) -> Result<(), Error> {
    out.push('[');
    for (idx, elem) in list.iter().enumerate() {
        if idx > 0 {
            out.push_str(style::LIST_ELEM_SEP);
        }
        write_tag(out, elem)?;
    }
    out.push(']');
    Ok(())
}

fn write_tag(out: &mut String, tag: &Tag) -> Result<(), Error> {
    match tag {
        Tag::End => out.push_str("END"),
        Tag::Byte(v) => {
            let _ = write!(out, "{v}{}", style::BYTE_SUFFIX);
        }
        Tag::Short(v) => {
            let _ = write!(out, "{v}{}", style::SHORT_SUFFIX);
        }
        Tag::Int(v) => {
            let _ = write!(out, "{v}");
        }
        Tag::Long(v) => {
            let _ = write!(out, "{v}{}", style::LONG_SUFFIX);
        }
        Tag::Float(v) => style::write_float(out, *v),
        Tag::Double(v) => style::write_double(out, *v),
        Tag::ByteArray(arr) => {
            out.push_str(style::BYTE_ARRAY_PREFIX);
            for (idx, &v) in arr.iter().enumerate() {
                if idx > 0 {
                    out.push_str(style::ARRAY_ELEM_SEP);
                }
                let _ = write!(out, "{v}{}", style::BYTE_ARRAY_ELEM_SUFFIX);
            }
            out.push(']');
        }
        Tag::String(s) => write_quoted_string(out, s),
        Tag::List(list) => write_list(out, list)?,
        Tag::Compound(compound) => write_compound(out, compound)?,
        Tag::IntArray(arr) => {
            out.push_str(style::INT_ARRAY_PREFIX);
            for (idx, &v) in arr.iter().enumerate() {
                if idx > 0 {
                    out.push_str(style::ARRAY_ELEM_SEP);
                }
                let _ = write!(out, "{v}");
            }
            out.push(']');
        }
        Tag::LongArray(arr) => {
            out.push_str(style::LONG_ARRAY_PREFIX);
            for (idx, &v) in arr.iter().enumerate() {
                if idx > 0 {
                    out.push_str(style::ARRAY_ELEM_SEP);
                }
                let _ = write!(out, "{v}{}", style::LONG_SUFFIX);
            }
            out.push(']');
        }
    }
    Ok(())
}
