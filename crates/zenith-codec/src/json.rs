//! Strict JSON text layer (spec §7.1, §3.1).
//!
//! The parser accepts exactly what the game's strict JSON reader accepts: a leading UTF-8 byte-order mark,
//! duplicate keys (the last value wins, keeping the position of the first), and nothing else outside RFC 8259
//! (no comments, trailing commas, single quotes, unquoted keys, `NaN`, `Infinity`, leading zeros, `1.`, raw control
//! characters in strings, or `\'` escapes). Numbers keep their **original text** so that integer reads can truncate
//! and wrap exactly like the game does (see [`JsonNumber`]).

use std::collections::HashMap;
use std::fmt;

/// Maximum nesting depth the parser accepts before reporting an error.
///
/// The game has no explicit limit (very deep input exhausts its stack, which is a crash and not replicated).
/// Zenith reports a normal parse error instead of overflowing its own stack.
pub const MAX_DEPTH: usize = 1024;

/// Which token rules apply to the document (spec §7.1, right-hand column).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseMode {
    /// Data files: any top-level value, nothing may follow it, an empty file is JSON `null`.
    Data,
    /// `pack.mcmeta` and sidecar files: the root must be an object, reading stops after it (trailing text is
    /// ignored), and an empty file is an error.
    Metadata,
}

/// A JSON parse failure with the byte offset where it was detected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonError {
    /// Human-readable description.
    pub message: String,
    /// Byte offset into the (BOM-stripped) text.
    pub offset: usize,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at offset {}", self.message, self.offset)
    }
}

impl std::error::Error for JsonError {}

/// A JSON number that remembers its decimal text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonNumber {
    text: String,
}

/// A hard (exception-like) failure while reading a number: not a normal decode error (spec §7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberError {
    /// The decimal exponent is outside the supported ±10000 window.
    ExponentOutOfRange,
}

impl fmt::Display for NumberError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExponentOutOfRange => f.write_str("number exponent is out of range"),
        }
    }
}

impl std::error::Error for NumberError {}

/// Largest decimal exponent magnitude an integer read accepts (spec §7.2 / Open question 6).
pub const MAX_INT_EXPONENT: i128 = 10_000;

impl JsonNumber {
    /// Wraps already-validated JSON number text.
    pub fn from_text(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }

    /// Creates a number from an integer.
    pub fn from_i64(value: i64) -> Self {
        Self {
            text: value.to_string(),
        }
    }

    /// The original decimal text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Magnitude of the truncated integer value modulo 2^64, with the sign applied (two's complement).
    fn wrapped_u64(&self) -> Result<u64, NumberError> {
        let t = self.text.as_str();
        let (neg, t) = match t.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, t),
        };
        let (mantissa, exp) = match t.find(['e', 'E']) {
            Some(i) => {
                let e = &t[i + 1..];
                let (eneg, edigits) = match e.strip_prefix('-') {
                    Some(r) => (true, r),
                    None => (false, e.strip_prefix('+').unwrap_or(e)),
                };
                let mut v: i128 = 0;
                for b in edigits.bytes() {
                    v = (v * 10 + i128::from(b - b'0')).min(1 << 100);
                }
                (&t[..i], if eneg { -v } else { v })
            }
            None => (t, 0),
        };
        if exp.abs() > MAX_INT_EXPONENT {
            return Err(NumberError::ExponentOutOfRange);
        }
        let (int_part, frac_part) = match mantissa.find('.') {
            Some(i) => (&mantissa[..i], &mantissa[i + 1..]),
            None => (mantissa, ""),
        };
        let digits: String = [int_part, frac_part].concat();
        let digits = digits.trim_start_matches('0');
        let scale = exp - frac_part.len() as i128;
        let magnitude: u64 = if scale >= 0 {
            let base = wrapping_digits(digits);
            base.wrapping_mul(pow10_wrapping(scale as u64))
        } else {
            let drop = (-scale) as usize;
            if drop >= digits.len() {
                0
            } else {
                wrapping_digits(&digits[..digits.len() - drop])
            }
        };
        Ok(if neg {
            magnitude.wrapping_neg()
        } else {
            magnitude
        })
    }

    /// 64-bit integer read: truncate toward zero, wrap on overflow.
    pub fn to_i64(&self) -> Result<i64, NumberError> {
        self.wrapped_u64().map(|v| v as i64)
    }

    /// 32-bit integer read (low 32 bits of the truncated value).
    pub fn to_i32(&self) -> Result<i32, NumberError> {
        self.wrapped_u64().map(|v| v as i32)
    }

    /// 16-bit integer read.
    pub fn to_i16(&self) -> Result<i16, NumberError> {
        self.wrapped_u64().map(|v| v as i16)
    }

    /// 8-bit integer read.
    pub fn to_i8(&self) -> Result<i8, NumberError> {
        self.wrapped_u64().map(|v| v as i8)
    }

    /// Float read: the decimal text rounds directly to the nearest `f32` (never via `f64`); overflow gives ∞.
    pub fn to_f32(&self) -> f32 {
        self.text.parse::<f32>().unwrap_or(f32::NAN)
    }

    /// Double read: nearest `f64`; overflow gives ∞.
    pub fn to_f64(&self) -> f64 {
        self.text.parse::<f64>().unwrap_or(f64::NAN)
    }
}

fn wrapping_digits(digits: &str) -> u64 {
    digits.bytes().fold(0u64, |acc, b| {
        acc.wrapping_mul(10).wrapping_add(u64::from(b - b'0'))
    })
}

fn pow10_wrapping(mut exp: u64) -> u64 {
    let mut base = 10u64;
    let mut acc = 1u64;
    while exp > 0 {
        if exp & 1 == 1 {
            acc = acc.wrapping_mul(base);
        }
        base = base.wrapping_mul(base);
        exp >>= 1;
    }
    acc
}

/// An insertion-ordered JSON object. Duplicate keys keep the **last** value in the position of the first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JsonObject {
    entries: Vec<(String, Json)>,
    index: HashMap<String, usize>,
}

impl JsonObject {
    /// An empty object.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts or replaces a value (a replaced key keeps its original position).
    pub fn insert(&mut self, key: impl Into<String>, value: Json) {
        let key = key.into();
        match self.index.get(&key) {
            Some(&i) => self.entries[i].1 = value,
            None => {
                self.index.insert(key.clone(), self.entries.len());
                self.entries.push((key, value));
            }
        }
    }

    /// Looks a key up.
    pub fn get(&self, key: &str) -> Option<&Json> {
        self.index.get(key).map(|&i| &self.entries[i].1)
    }

    /// Whether the key is present (even with a `null` value).
    pub fn contains_key(&self, key: &str) -> bool {
        self.index.contains_key(key)
    }

    /// Entries in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Json)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Keys in insertion order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(k, _)| k.as_str())
    }

    /// Number of distinct keys.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// A JSON value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Json {
    /// `null`
    Null,
    /// `true` / `false`
    Bool(bool),
    /// A number (original text kept).
    Number(JsonNumber),
    /// A string.
    String(String),
    /// An array.
    Array(Vec<Json>),
    /// An object (insertion-ordered).
    Object(JsonObject),
}

impl Json {
    /// Short type name for messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "boolean",
            Self::Number(_) => "number",
            Self::String(_) => "string",
            Self::Array(_) => "array",
            Self::Object(_) => "object",
        }
    }

    /// The object, if this is one.
    pub fn as_object(&self) -> Option<&JsonObject> {
        match self {
            Self::Object(o) => Some(o),
            _ => None,
        }
    }

    /// The array, if this is one.
    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Self::Array(a) => Some(a),
            _ => None,
        }
    }

    /// The string, if this is one.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    /// The number, if this is one.
    pub fn as_number(&self) -> Option<&JsonNumber> {
        match self {
            Self::Number(n) => Some(n),
            _ => None,
        }
    }

    /// The boolean, if this is one.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Object member lookup (`None` for non-objects).
    pub fn get(&self, key: &str) -> Option<&Json> {
        self.as_object().and_then(|o| o.get(key))
    }

    /// Whether this is `null`.
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Compact serialization (strings escaped per RFC 8259; numbers keep their text).
    pub fn to_json_string(&self) -> String {
        let mut out = String::new();
        self.write_to(&mut out);
        out
    }

    fn write_to(&self, out: &mut String) {
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Self::Number(n) => out.push_str(n.text()),
            Self::String(s) => write_json_string(s, out),
            Self::Array(a) => {
                out.push('[');
                for (i, v) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write_to(out);
                }
                out.push(']');
            }
            Self::Object(o) => {
                out.push('{');
                for (i, (k, v)) in o.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_json_string(k, out);
                    out.push(':');
                    v.write_to(out);
                }
                out.push('}');
            }
        }
    }
}

impl fmt::Display for Json {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_json_string())
    }
}

fn write_json_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Decodes bytes as UTF-8, replacing malformed sequences with U+FFFD.
pub fn decode_utf8_lossy(bytes: &[u8]) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(bytes)
}

/// Parses JSON text in the given mode.
pub fn parse(text: &str, mode: ParseMode) -> Result<Json, JsonError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut p = Parser {
        src: text.as_bytes(),
        text,
        pos: 0,
    };
    p.skip_ws();
    match mode {
        ParseMode::Data => {
            if p.pos >= p.src.len() {
                return Ok(Json::Null);
            }
            let value = p.parse_value(0)?;
            p.skip_ws();
            if p.pos < p.src.len() {
                return Err(p.err("trailing content after the root value"));
            }
            Ok(value)
        }
        ParseMode::Metadata => {
            if p.pos >= p.src.len() {
                return Err(p.err("empty document"));
            }
            if p.src[p.pos] != b'{' {
                return Err(p.err("the root value must be an object"));
            }
            p.parse_value(0)
        }
    }
}

/// Parses bytes (lossy UTF-8) in the given mode.
pub fn parse_bytes(bytes: &[u8], mode: ParseMode) -> Result<Json, JsonError> {
    parse(&decode_utf8_lossy(bytes), mode)
}

struct Parser<'a> {
    src: &'a [u8],
    text: &'a str,
    pos: usize,
}

impl Parser<'_> {
    fn err(&self, message: &str) -> JsonError {
        JsonError {
            message: message.to_owned(),
            offset: self.pos,
        }
    }

    fn skip_ws(&mut self) {
        while let Some(&b) = self.src.get(self.pos) {
            if matches!(b, b' ' | b'\t' | b'\n' | b'\r') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn parse_value(&mut self, depth: usize) -> Result<Json, JsonError> {
        if depth > MAX_DEPTH {
            return Err(self.err("nesting too deep"));
        }
        let Some(&b) = self.src.get(self.pos) else {
            return Err(self.err("unexpected end of input"));
        };
        match b {
            b'{' => self.parse_object(depth),
            b'[' => self.parse_array(depth),
            b'"' => self.parse_string().map(Json::String),
            b'-' | b'0'..=b'9' => self.parse_number(),
            b't' => self.parse_literal("true", Json::Bool(true)),
            b'f' => self.parse_literal("false", Json::Bool(false)),
            b'n' => self.parse_literal("null", Json::Null),
            _ => Err(self.err("unexpected character")),
        }
    }

    fn at_delimiter(&self) -> bool {
        match self.src.get(self.pos) {
            None => true,
            Some(b) => matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b',' | b']' | b'}'),
        }
    }

    fn parse_literal(&mut self, word: &str, value: Json) -> Result<Json, JsonError> {
        if self.src[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            if self.at_delimiter() {
                return Ok(value);
            }
        }
        Err(self.err("malformed literal"))
    }

    fn parse_number(&mut self) -> Result<Json, JsonError> {
        let start = self.pos;
        if self.src[self.pos] == b'-' {
            self.pos += 1;
        }
        match self.src.get(self.pos) {
            Some(b'0') => {
                self.pos += 1;
                if matches!(self.src.get(self.pos), Some(b'0'..=b'9')) {
                    return Err(self.err("leading zeros are not allowed"));
                }
            }
            Some(b'1'..=b'9') => self.skip_digits(),
            _ => return Err(self.err("malformed number")),
        }
        if self.src.get(self.pos) == Some(&b'.') {
            self.pos += 1;
            if !matches!(self.src.get(self.pos), Some(b'0'..=b'9')) {
                return Err(self.err("malformed number: digits expected after '.'"));
            }
            self.skip_digits();
        }
        if matches!(self.src.get(self.pos), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.src.get(self.pos), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !matches!(self.src.get(self.pos), Some(b'0'..=b'9')) {
                return Err(self.err("malformed number: digits expected in exponent"));
            }
            self.skip_digits();
        }
        if !self.at_delimiter() {
            return Err(self.err("malformed number"));
        }
        Ok(Json::Number(JsonNumber::from_text(
            &self.text[start..self.pos],
        )))
    }

    fn skip_digits(&mut self) {
        while matches!(self.src.get(self.pos), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
    }

    fn parse_string(&mut self) -> Result<String, JsonError> {
        debug_assert_eq!(self.src[self.pos], b'"');
        self.pos += 1;
        let mut out = String::new();
        let mut run_start = self.pos;
        loop {
            let Some(&b) = self.src.get(self.pos) else {
                return Err(self.err("unterminated string"));
            };
            match b {
                b'"' => {
                    out.push_str(&self.text[run_start..self.pos]);
                    self.pos += 1;
                    return Ok(out);
                }
                b'\\' => {
                    out.push_str(&self.text[run_start..self.pos]);
                    self.pos += 1;
                    self.parse_escape(&mut out)?;
                    run_start = self.pos;
                }
                0..=0x1f => return Err(self.err("unescaped control character in string")),
                _ => self.pos += 1,
            }
        }
    }

    fn parse_escape(&mut self, out: &mut String) -> Result<(), JsonError> {
        let Some(&b) = self.src.get(self.pos) else {
            return Err(self.err("unterminated escape"));
        };
        self.pos += 1;
        match b {
            b'"' => out.push('"'),
            b'\\' => out.push('\\'),
            b'/' => out.push('/'),
            b'b' => out.push('\u{8}'),
            b'f' => out.push('\u{c}'),
            b'n' => out.push('\n'),
            b'r' => out.push('\r'),
            b't' => out.push('\t'),
            b'u' => {
                let hi = self.parse_hex4()?;
                if (0xD800..0xDC00).contains(&hi) {
                    if self.src[self.pos..].starts_with(b"\\u") {
                        let save = self.pos;
                        self.pos += 2;
                        let lo = self.parse_hex4()?;
                        if (0xDC00..0xE000).contains(&lo) {
                            let cp = 0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
                            out.push(char::from_u32(cp).unwrap_or('\u{fffd}'));
                        } else {
                            // Unpaired high surrogate: Rust strings cannot hold it, substitute U+FFFD and
                            // re-read the second escape on its own.
                            out.push('\u{fffd}');
                            self.pos = save;
                        }
                    } else {
                        out.push('\u{fffd}');
                    }
                } else {
                    out.push(char::from_u32(hi).unwrap_or('\u{fffd}'));
                }
            }
            _ => {
                self.pos -= 1;
                return Err(self.err("invalid escape sequence"));
            }
        }
        Ok(())
    }

    fn parse_hex4(&mut self) -> Result<u32, JsonError> {
        let mut v = 0u32;
        for _ in 0..4 {
            let Some(&b) = self.src.get(self.pos) else {
                return Err(self.err("truncated \\u escape"));
            };
            let d = match b {
                b'0'..=b'9' => b - b'0',
                b'a'..=b'f' => b - b'a' + 10,
                b'A'..=b'F' => b - b'A' + 10,
                _ => return Err(self.err("malformed \\u escape")),
            };
            v = v * 16 + u32::from(d);
            self.pos += 1;
        }
        Ok(v)
    }

    fn parse_array(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.pos += 1;
        let mut items = Vec::new();
        self.skip_ws();
        if self.src.get(self.pos) == Some(&b']') {
            self.pos += 1;
            return Ok(Json::Array(items));
        }
        loop {
            self.skip_ws();
            items.push(self.parse_value(depth + 1)?);
            self.skip_ws();
            match self.src.get(self.pos) {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(self.err("expected ',' or ']'")),
            }
        }
    }

    fn parse_object(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.pos += 1;
        let mut obj = JsonObject::new();
        self.skip_ws();
        if self.src.get(self.pos) == Some(&b'}') {
            self.pos += 1;
            return Ok(Json::Object(obj));
        }
        loop {
            self.skip_ws();
            if self.src.get(self.pos) != Some(&b'"') {
                return Err(self.err("expected a double-quoted member name"));
            }
            let key = self.parse_string()?;
            self.skip_ws();
            if self.src.get(self.pos) != Some(&b':') {
                return Err(self.err("expected ':'"));
            }
            self.pos += 1;
            self.skip_ws();
            let value = self.parse_value(depth + 1)?;
            obj.insert(key, value);
            self.skip_ws();
            match self.src.get(self.pos) {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Object(obj));
                }
                _ => return Err(self.err("expected ',' or '}'")),
            }
        }
    }
}
