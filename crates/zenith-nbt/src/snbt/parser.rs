//! 1.21.5+ SNBT recursive-descent parser.
//!
//! Supports:
//! - Compounds (`{k: v, ...}` with optional trailing comma)
//! - Heterogeneous lists (`[a, b, ...]` with optional trailing comma)
//! - Typed arrays (`[B; ...]`, `[I; ...]`, `[L; ...]` with optional trailing comma)
//! - Single- and double-quoted strings with escapes (`\\`, `\'`, `\"`, `\b`, `\s`, `\t`,
//!   `\n`, `\f`, `\r`, `\xHH`, `\uHHHH`, `\UHHHHHHHH`, `\N{NAME}`)
//! - Unquoted strings (`[A-Za-z._][A-Za-z0-9._+-]*`)
//! - Booleans (`true` → `1b`, `false` → `0b`)
//! - Integers: decimal, hexadecimal (`0x`/`0X`), binary (`0b`/`0B`), underscores between digits,
//!   signed/unsigned prefixes (`s`/`u`), type suffixes (`b`/`s`/`i`/`l`)
//! - Floats/doubles: partial decimals (`.5`, `5.`), underscores, scientific notation (`e`/`E`),
//!   suffixes (`f`/`d`), rejecting non-finite overflow
//! - Built-in operations: `bool(expr)` and `uuid(string)`

use crate::accounter::DEFAULT_MAX_DEPTH;
use crate::compound::Compound;
use crate::error::{Error, SnbtError};
use crate::list::List;
use crate::mutf8::NbtString;
use crate::tag::Tag;

/// Parses a single SNBT value, requiring the entire input (aside from surrounding whitespace)
/// to be consumed.
pub fn parse_snbt(input: &str) -> Result<Tag, Error> {
    let mut parser = Parser::new(input);
    parser.skip_ws();
    let tag = parser.parse_value()?;
    parser.skip_ws();
    if !parser.is_eof() {
        return Err(parser
            .err_here(format!(
                "unexpected trailing input: {:?}",
                parser.rest_preview()
            ))
            .into());
    }
    Ok(tag)
}

/// Parses an SNBT compound (`{ ... }`), returning an error if the root value is not a compound.
pub fn parse_snbt_compound(input: &str) -> Result<Compound, Error> {
    let tag = parse_snbt(input)?;
    tag.into_compound()
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    const fn new(src: &'a str) -> Self {
        Self {
            src,
            pos: 0,
            depth: 0,
        }
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.src.len()
    }

    fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    fn rest_preview(&self) -> &'a str {
        let rest = self.rest();
        match rest.char_indices().nth(16) {
            Some((idx, _)) => &rest[..idx],
            None => rest,
        }
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn peek_byte(&self) -> Option<u8> {
        self.src.as_bytes().get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn skip_ws(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
    }

    fn err(&self, offset: usize, message: impl Into<String>) -> SnbtError {
        SnbtError::new(offset, message)
    }

    fn err_here(&self, message: impl Into<String>) -> SnbtError {
        SnbtError::new(self.pos, message)
    }

    fn push_depth(&mut self) -> Result<(), SnbtError> {
        self.depth += 1;
        if self.depth > DEFAULT_MAX_DEPTH {
            return Err(self.err_here(format!(
                "SNBT nesting depth exceeded maximum of {DEFAULT_MAX_DEPTH}"
            )));
        }
        Ok(())
    }

    fn pop_depth(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    fn parse_value(&mut self) -> Result<Tag, SnbtError> {
        self.skip_ws();
        let Some(ch) = self.peek() else {
            return Err(self.err_here("unexpected end of input"));
        };
        match ch {
            '{' => self.parse_compound().map(Tag::Compound),
            '[' => self.parse_list_or_array(),
            '"' | '\'' => self
                .parse_quoted_string()
                .map(|s| Tag::String(NbtString::from(s))),
            _ => self.parse_atom_or_op(),
        }
    }

    fn parse_compound(&mut self) -> Result<Compound, SnbtError> {
        self.push_depth()?;
        let res = (|| {
            self.bump(); // '{'
            let mut map = Compound::new();
            loop {
                self.skip_ws();
                if self.peek() == Some('}') {
                    self.bump();
                    return Ok(map);
                }
                let key = self.parse_key()?;
                self.skip_ws();
                if self.peek() != Some(':') {
                    return Err(self.err_here("expected ':' after compound key"));
                }
                self.bump(); // ':'
                let value = self.parse_value()?;
                map.insert(key, value);
                self.skip_ws();
                match self.peek() {
                    Some(',') => {
                        self.bump();
                    }
                    Some('}') => {
                        self.bump();
                        return Ok(map);
                    }
                    _ => return Err(self.err_here("expected ',' or '}' in compound")),
                }
            }
        })();
        self.pop_depth();
        res
    }

    fn parse_key(&mut self) -> Result<String, SnbtError> {
        self.skip_ws();
        let start = self.pos;
        match self.peek() {
            Some('"' | '\'') => self.parse_quoted_string(),
            Some(c) if is_unquoted_start(c) => {
                let s = self.take_unquoted_token();
                // PARITY: unquoted boolean words as keys are settled by the oracle differential fuzzer (snbt.md Q5-Q7).
                if s.eq_ignore_ascii_case("true") || s.eq_ignore_ascii_case("false") {
                    return Err(self.err(
                        start,
                        format!("boolean literal '{s}' must be quoted when used as a key"),
                    ));
                }
                Ok(s.to_owned())
            }
            _ => Err(self.err_here("expected compound key")),
        }
    }

    fn parse_list_or_array(&mut self) -> Result<Tag, SnbtError> {
        self.push_depth()?;
        let res = (|| {
            self.bump(); // '['
            let save = self.pos;
            self.skip_ws();
            // Check for typed array header: `B;`, `I;`, `L;`
            if let Some(prefix @ ('B' | 'I' | 'L')) = self.peek() {
                let after_prefix = self.pos + 1;
                let rest = &self.src[after_prefix..];
                let ws_len = rest
                    .find(|c: char| !c.is_whitespace())
                    .unwrap_or(rest.len());
                if rest.as_bytes().get(ws_len) == Some(&b';') {
                    self.pos = after_prefix + ws_len + 1;
                    return match prefix {
                        'B' => self.parse_byte_array(),
                        'I' => self.parse_int_array(),
                        'L' => self.parse_long_array(),
                        _ => unreachable!(),
                    };
                }
            }
            self.pos = save;
            self.parse_list().map(Tag::List)
        })();
        self.pop_depth();
        res
    }

    fn parse_byte_array(&mut self) -> Result<Tag, SnbtError> {
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            if self.peek() == Some(']') {
                self.bump();
                return Ok(Tag::ByteArray(items));
            }
            let elem_start = self.pos;
            let val = self.parse_value()?;
            match val {
                Tag::Byte(b) => items.push(b),
                other => {
                    return Err(self.err(
                        elem_start,
                        format!(
                            "expected byte value in byte array, found {:?}",
                            other.tag_type()
                        ),
                    ));
                }
            }
            self.skip_ws();
            match self.peek() {
                Some(',') => {
                    self.bump();
                }
                Some(']') => {
                    self.bump();
                    return Ok(Tag::ByteArray(items));
                }
                _ => return Err(self.err_here("expected ',' or ']' in byte array")),
            }
        }
    }

    fn parse_int_array(&mut self) -> Result<Tag, SnbtError> {
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            if self.peek() == Some(']') {
                self.bump();
                return Ok(Tag::IntArray(items));
            }
            let elem_start = self.pos;
            let val = self.parse_value()?;
            match val {
                Tag::Int(n) => items.push(n),
                other => {
                    return Err(self.err(
                        elem_start,
                        format!(
                            "expected int value in int array, found {:?}",
                            other.tag_type()
                        ),
                    ));
                }
            }
            self.skip_ws();
            match self.peek() {
                Some(',') => {
                    self.bump();
                }
                Some(']') => {
                    self.bump();
                    return Ok(Tag::IntArray(items));
                }
                _ => return Err(self.err_here("expected ',' or ']' in int array")),
            }
        }
    }

    fn parse_long_array(&mut self) -> Result<Tag, SnbtError> {
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            if self.peek() == Some(']') {
                self.bump();
                return Ok(Tag::LongArray(items));
            }
            let elem_start = self.pos;
            let val = self.parse_value()?;
            match val {
                Tag::Long(n) => items.push(n),
                other => {
                    return Err(self.err(
                        elem_start,
                        format!(
                            "expected long value in long array, found {:?}",
                            other.tag_type()
                        ),
                    ));
                }
            }
            self.skip_ws();
            match self.peek() {
                Some(',') => {
                    self.bump();
                }
                Some(']') => {
                    self.bump();
                    return Ok(Tag::LongArray(items));
                }
                _ => return Err(self.err_here("expected ',' or ']' in long array")),
            }
        }
    }

    fn parse_list(&mut self) -> Result<List, SnbtError> {
        let mut list = List::new();
        loop {
            self.skip_ws();
            if self.peek() == Some(']') {
                self.bump();
                return Ok(list);
            }
            list.push(self.parse_value()?);
            self.skip_ws();
            match self.peek() {
                Some(',') => {
                    self.bump();
                }
                Some(']') => {
                    self.bump();
                    return Ok(list);
                }
                _ => return Err(self.err_here("expected ',' or ']' in list")),
            }
        }
    }

    fn parse_quoted_string(&mut self) -> Result<String, SnbtError> {
        let start = self.pos;
        let quote = self
            .bump()
            .ok_or_else(|| self.err(start, "expected opening quote"))?;
        let mut out = String::new();
        while let Some(ch) = self.bump() {
            if ch == quote {
                return Ok(out);
            }
            if ch == '\\' {
                let esc_pos = self.pos;
                let Some(esc) = self.bump() else {
                    return Err(self.err(esc_pos, "unterminated escape sequence"));
                };
                match esc {
                    '\\' => out.push('\\'),
                    '\'' => out.push('\''),
                    '"' => out.push('"'),
                    'b' => out.push('\u{0008}'),
                    's' => out.push(' '),
                    't' => out.push('\t'),
                    'n' => out.push('\n'),
                    'f' => out.push('\u{000C}'),
                    'r' => out.push('\r'),
                    'x' => {
                        let cp = self.read_hex_codepoint(2, esc_pos)?;
                        out.push(
                            char::from_u32(cp)
                                .ok_or_else(|| self.err(esc_pos, "invalid \\x code point"))?,
                        );
                    }
                    'u' => {
                        let cp = self.read_hex_codepoint(4, esc_pos)?;
                        let ch = char::from_u32(cp).ok_or_else(|| {
                            self.err(esc_pos, format!("invalid Unicode scalar \\u{cp:04x}"))
                        })?;
                        out.push(ch);
                    }
                    'U' => {
                        let cp = self.read_hex_codepoint(8, esc_pos)?;
                        let ch = char::from_u32(cp).ok_or_else(|| {
                            self.err(esc_pos, format!("invalid Unicode scalar \\U{cp:08x}"))
                        })?;
                        out.push(ch);
                    }
                    'N' => {
                        if self.peek() != Some('{') {
                            return Err(self.err_here("expected '{' after \\N"));
                        }
                        self.bump();
                        let name_start = self.pos;
                        let Some(close_rel) = self.rest().find('}') else {
                            return Err(self.err(name_start, "unterminated \\N{...} escape"));
                        };
                        let name = &self.src[name_start..name_start + close_rel];
                        self.pos = name_start + close_rel + 1;
                        if name.is_empty() {
                            return Err(self.err(name_start, "empty Unicode character name"));
                        }
                        let ch =
                            unicode_names2::character(&name.to_uppercase()).ok_or_else(|| {
                                self.err(
                                    name_start,
                                    format!("unknown Unicode character name '{name}'"),
                                )
                            })?;
                        out.push(ch);
                    }
                    other => {
                        return Err(
                            self.err(esc_pos, format!("unknown escape sequence '\\{other}'"))
                        );
                    }
                }
            } else {
                out.push(ch);
            }
        }
        Err(self.err(start, "unterminated quoted string"))
    }

    fn read_hex_codepoint(&mut self, digits: usize, err_pos: usize) -> Result<u32, SnbtError> {
        if self.rest().len() < digits {
            return Err(self.err(err_pos, format!("expected {digits} hex digits in escape")));
        }
        let slice = &self.src[self.pos..self.pos + digits];
        if !slice.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(self.err(self.pos, format!("invalid hex escape '{slice}'")));
        }
        self.pos += digits;
        u32::from_str_radix(slice, 16)
            .map_err(|_| self.err(err_pos, format!("invalid hex escape '{slice}'")))
    }

    fn take_unquoted_token(&mut self) -> &'a str {
        let start = self.pos;
        while let Some(b) = self.peek_byte() {
            if is_unquoted_cont_byte(b) {
                self.pos += 1;
            } else {
                break;
            }
        }
        &self.src[start..self.pos]
    }

    fn parse_atom_or_op(&mut self) -> Result<Tag, SnbtError> {
        let start = self.pos;
        let first = self
            .peek_byte()
            .ok_or_else(|| self.err_here("unexpected end of input"))?;

        // Check whether this starts a numeric literal:
        // digit, '+', '-', or '.' immediately followed by an ASCII digit.
        let starts_number = first.is_ascii_digit()
            || first == b'+'
            || first == b'-'
            || (first == b'.'
                && self
                    .src
                    .as_bytes()
                    .get(self.pos + 1)
                    .is_some_and(u8::is_ascii_digit));

        if starts_number {
            let token = self.take_unquoted_token();
            return parse_number_token(token, start);
        }

        if !is_unquoted_start_byte(first) {
            return Err(self.err(
                start,
                format!("unexpected character {:?}", self.peek().unwrap_or('\0')),
            ));
        }

        let ident = self.take_unquoted_token();
        let after_ident = self.pos;
        self.skip_ws();

        // Check for built-in operation call: `ident(...)`
        if self.peek() == Some('(') {
            return self.parse_operation(ident, start);
        }
        self.pos = after_ident;

        // Boolean words are recognised case-insensitively (spec: snbt.md, reader answer Q4).
        if ident.eq_ignore_ascii_case("true") {
            Ok(Tag::Byte(1))
        } else if ident.eq_ignore_ascii_case("false") {
            Ok(Tag::Byte(0))
        } else {
            Ok(Tag::String(NbtString::from(ident)))
        }
    }

    fn parse_operation(&mut self, name: &str, name_pos: usize) -> Result<Tag, SnbtError> {
        self.push_depth()?;
        let res = (|| {
            self.bump(); // '('
            let mut args = Vec::new();
            loop {
                self.skip_ws();
                if self.peek() == Some(')') {
                    self.bump();
                    break;
                }
                args.push(self.parse_value()?);
                self.skip_ws();
                match self.peek() {
                    Some(',') => {
                        self.bump();
                    }
                    Some(')') => {
                        self.bump();
                        break;
                    }
                    _ => return Err(self.err_here("expected ',' or ')' in operation arguments")),
                }
            }
            match name {
                "bool" => {
                    if args.len() != 1 {
                        return Err(self.err(
                            name_pos,
                            format!("bool() expects 1 argument, got {}", args.len()),
                        ));
                    }
                    let truthy = match &args[0] {
                        Tag::Byte(v) => *v != 0,
                        Tag::Short(v) => *v != 0,
                        Tag::Int(v) => *v != 0,
                        Tag::Long(v) => *v != 0,
                        Tag::Float(v) => *v != 0.0,
                        Tag::Double(v) => *v != 0.0,
                        other => {
                            return Err(self.err(
                                name_pos,
                                format!(
                                    "bool() expects a boolean or number, got {:?}",
                                    other.tag_type()
                                ),
                            ));
                        }
                    };
                    Ok(Tag::Byte(i8::from(truthy)))
                }
                "uuid" => {
                    if args.len() != 1 {
                        return Err(self.err(
                            name_pos,
                            format!("uuid() expects 1 argument, got {}", args.len()),
                        ));
                    }
                    let Some(s) = args[0].as_str() else {
                        return Err(self.err(name_pos, "uuid() expects a string argument"));
                    };
                    parse_uuid_to_int_array(s, name_pos)
                }
                _ => Err(self.err(name_pos, format!("unknown SNBT operation '{name}'"))),
            }
        })();
        self.pop_depth();
        res
    }
}

const fn is_unquoted_start_byte(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'.' || b == b'_'
}

const fn is_unquoted_start(c: char) -> bool {
    c.is_ascii() && is_unquoted_start_byte(c as u8)
}

const fn is_unquoted_cont_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'+' || b == b'-'
}

fn parse_uuid_to_int_array(s: &str, err_pos: usize) -> Result<Tag, SnbtError> {
    // Standard hyphenated UUID: 8-4-4-4-12 hex digits (36 ASCII chars).
    let bytes = s.as_bytes();
    if bytes.len() != 36
        || bytes[8] != b'-'
        || bytes[13] != b'-'
        || bytes[18] != b'-'
        || bytes[23] != b'-'
    {
        return Err(SnbtError::new(
            err_pos,
            format!("invalid UUID string '{s}' (expected 8-4-4-4-12 hex format)"),
        ));
    }
    let hex: String = s.chars().filter(|&c| c != '-').collect();
    if hex.len() != 32 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(SnbtError::new(
            err_pos,
            format!("invalid hex digits in UUID '{s}'"),
        ));
    }
    let msb = u64::from_str_radix(&hex[0..16], 16)
        .map_err(|_| SnbtError::new(err_pos, "invalid UUID"))?;
    let lsb = u64::from_str_radix(&hex[16..32], 16)
        .map_err(|_| SnbtError::new(err_pos, "invalid UUID"))?;
    Ok(Tag::IntArray(vec![
        (msb >> 32) as i32,
        msb as i32,
        (lsb >> 32) as i32,
        lsb as i32,
    ]))
}

// --- 1.21.5+ Number Parsing ---

#[derive(Clone, Copy, PartialEq, Eq)]
enum IntType {
    Byte,
    Short,
    Int,
    Long,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Signedness {
    Signed,
    Unsigned,
}

fn parse_number_token(token: &str, offset: usize) -> Result<Tag, SnbtError> {
    if token.is_empty() {
        return Err(SnbtError::new(offset, "empty numeric literal"));
    }

    let (sign, rest) = if let Some(r) = token.strip_prefix('-') {
        (-1i8, r)
    } else if let Some(r) = token.strip_prefix('+') {
        (1i8, r)
    } else {
        (0i8, token)
    };

    if rest.is_empty() {
        return Err(SnbtError::new(
            offset,
            format!("invalid numeric literal '{token}'"),
        ));
    }

    // Hexadecimal: 0x / 0X
    if let Some(hex_body) = rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")) {
        return parse_radix_int(token, offset, sign, hex_body, 16);
    }

    // Binary: 0b / 0B (only when followed by something; bare `0b`/`0B` is decimal byte zero!)
    if let Some(bin_body) = rest.strip_prefix("0b").or_else(|| rest.strip_prefix("0B"))
        && !bin_body.is_empty()
    {
        return parse_radix_int(token, offset, sign, bin_body, 2);
    }

    // Check if float/double: contains '.', 'e'/'E', or ends with 'f'/'F'/'d'/'D'
    let last_byte = rest.as_bytes()[rest.len() - 1];
    let has_dot_or_exp = rest.bytes().any(|b| b == b'.' || b == b'e' || b == b'E');
    let has_float_suffix = matches!(last_byte, b'f' | b'F' | b'd' | b'D');

    if has_dot_or_exp || has_float_suffix {
        return parse_float_literal(token, offset, sign, rest);
    }

    // Otherwise decimal integer (default signed).
    parse_radix_int(token, offset, sign, rest, 10)
}

fn parse_radix_int(
    full_token: &str,
    offset: usize,
    sign: i8,
    body: &str,
    radix: u32,
) -> Result<Tag, SnbtError> {
    let default_signedness = if radix == 10 {
        Signedness::Signed
    } else {
        Signedness::Unsigned
    };

    let (digits_raw, signedness, int_type) =
        split_int_suffix(body, radix, default_signedness, offset, full_token)?;

    // Validate digit group with underscores (no leading/trailing underscore, no empty digits).
    let clean_digits = strip_and_validate_underscores(digits_raw, radix, offset, full_token)?;

    if signedness == Signedness::Unsigned && sign < 0 {
        return Err(SnbtError::new(
            offset,
            format!("negative sign is not allowed on unsigned integer '{full_token}'"),
        ));
    }

    match (signedness, int_type) {
        (Signedness::Unsigned, IntType::Byte) => {
            let v = u8::from_str_radix(&clean_digits, radix).map_err(|_| {
                SnbtError::new(
                    offset,
                    format!("unsigned byte out of range: '{full_token}'"),
                )
            })?;
            Ok(Tag::Byte(v as i8))
        }
        (Signedness::Unsigned, IntType::Short) => {
            let v = u16::from_str_radix(&clean_digits, radix).map_err(|_| {
                SnbtError::new(
                    offset,
                    format!("unsigned short out of range: '{full_token}'"),
                )
            })?;
            Ok(Tag::Short(v as i16))
        }
        (Signedness::Unsigned, IntType::Int) => {
            let v = u32::from_str_radix(&clean_digits, radix).map_err(|_| {
                SnbtError::new(offset, format!("unsigned int out of range: '{full_token}'"))
            })?;
            Ok(Tag::Int(v as i32))
        }
        (Signedness::Unsigned, IntType::Long) => {
            let v = u64::from_str_radix(&clean_digits, radix).map_err(|_| {
                SnbtError::new(
                    offset,
                    format!("unsigned long out of range: '{full_token}'"),
                )
            })?;
            Ok(Tag::Long(v as i64))
        }
        (Signedness::Signed, ty) => {
            let mag = u128::from_str_radix(&clean_digits, radix).map_err(|_| {
                SnbtError::new(offset, format!("integer out of range: '{full_token}'"))
            })?;
            let signed_val: i128 = if sign < 0 {
                -(mag as i128)
            } else {
                mag as i128
            };
            match ty {
                IntType::Byte => i8::try_from(signed_val).map(Tag::Byte).map_err(|_| {
                    SnbtError::new(offset, format!("byte out of range: '{full_token}'"))
                }),
                IntType::Short => i16::try_from(signed_val).map(Tag::Short).map_err(|_| {
                    SnbtError::new(offset, format!("short out of range: '{full_token}'"))
                }),
                IntType::Int => i32::try_from(signed_val).map(Tag::Int).map_err(|_| {
                    SnbtError::new(offset, format!("int out of range: '{full_token}'"))
                }),
                IntType::Long => i64::try_from(signed_val).map(Tag::Long).map_err(|_| {
                    SnbtError::new(offset, format!("long out of range: '{full_token}'"))
                }),
            }
        }
    }
}

fn split_int_suffix<'a>(
    body: &'a str,
    radix: u32,
    default_signedness: Signedness,
    offset: usize,
    full_token: &str,
) -> Result<(&'a str, Signedness, IntType), SnbtError> {
    if body.is_empty() {
        return Err(SnbtError::new(
            offset,
            format!("missing digits in '{full_token}'"),
        ));
    }

    // For hex, `b`/`B` is a valid hex digit, so a single trailing `b`/`B` is part of the
    // hex digits unless preceded by `s`/`u` (`sb`/`ub`). All other suffix letters (`s`, `u`,
    // `i`, `l`) are not hex digits.
    let bytes = body.as_bytes();
    let len = bytes.len();

    // Check 2-char suffix first: [sSuU][bBsSiIlL]
    if len >= 2 {
        let prev = bytes[len - 2].to_ascii_lowercase();
        let last = bytes[len - 1].to_ascii_lowercase();
        if matches!(prev, b's' | b'u') && matches!(last, b'b' | b's' | b'i' | b'l') {
            let sign = if prev == b's' {
                Signedness::Signed
            } else {
                Signedness::Unsigned
            };
            let ty = match last {
                b'b' => IntType::Byte,
                b's' => IntType::Short,
                b'i' => IntType::Int,
                b'l' => IntType::Long,
                _ => unreachable!(),
            };
            return Ok((&body[..len - 2], sign, ty));
        }
    }

    // Check 1-char suffix:
    // - `b`/`B` (non-hex only) -> Byte
    // - `s`/`S` -> Short
    // - `i`/`I` -> Int
    // - `l`/`L` -> Long
    // - `u`/`U` -> Unsigned Int
    let last = bytes[len - 1].to_ascii_lowercase();
    match last {
        b'b' if radix != 16 => Ok((&body[..len - 1], default_signedness, IntType::Byte)),
        b's' => Ok((&body[..len - 1], default_signedness, IntType::Short)),
        b'i' => Ok((&body[..len - 1], default_signedness, IntType::Int)),
        b'l' => Ok((&body[..len - 1], default_signedness, IntType::Long)),
        b'u' => Ok((&body[..len - 1], Signedness::Unsigned, IntType::Int)),
        _ => Ok((body, default_signedness, IntType::Int)),
    }
}

fn is_radix_digit(b: u8, radix: u32) -> bool {
    match radix {
        2 => matches!(b, b'0' | b'1'),
        10 => b.is_ascii_digit(),
        16 => b.is_ascii_hexdigit(),
        _ => false,
    }
}

fn strip_and_validate_underscores(
    raw: &str,
    radix: u32,
    offset: usize,
    full_token: &str,
) -> Result<String, SnbtError> {
    let bytes = raw.as_bytes();
    if bytes.is_empty() || bytes[0] == b'_' || bytes[bytes.len() - 1] == b'_' {
        return Err(SnbtError::new(
            offset,
            format!("invalid numeric literal '{full_token}'"),
        ));
    }
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        if b == b'_' {
            continue;
        }
        if !is_radix_digit(b, radix) {
            return Err(SnbtError::new(
                offset,
                format!("invalid digit '{}' in '{full_token}'", b as char),
            ));
        }
        out.push(b as char);
    }
    if out.is_empty() {
        return Err(SnbtError::new(
            offset,
            format!("missing digits in '{full_token}'"),
        ));
    }
    Ok(out)
}

fn parse_float_literal(
    full_token: &str,
    offset: usize,
    sign: i8,
    rest: &str,
) -> Result<Tag, SnbtError> {
    let last = rest.as_bytes()[rest.len() - 1];
    let (body, is_f32) = match last {
        b'f' | b'F' => (&rest[..rest.len() - 1], true),
        b'd' | b'D' => (&rest[..rest.len() - 1], false),
        _ => (rest, false),
    };

    if body.is_empty() {
        return Err(SnbtError::new(
            offset,
            format!("invalid floating-point literal '{full_token}'"),
        ));
    }

    // Split optional exponent `e` / `E`
    let (mantissa, exp_part) = match body.find(['e', 'E']) {
        Some(idx) => (&body[..idx], Some(&body[idx + 1..])),
        None => (body, None),
    };

    // Split mantissa into optional integer part and optional fractional part
    let mut clean = String::with_capacity(full_token.len());
    if sign < 0 {
        clean.push('-');
    }

    if let Some((int_part, frac_part)) = mantissa.split_once('.') {
        if int_part.is_empty() && frac_part.is_empty() {
            return Err(SnbtError::new(
                offset,
                format!("invalid floating-point literal '{full_token}'"),
            ));
        }
        if !int_part.is_empty() {
            clean.push_str(&strip_and_validate_underscores(
                int_part, 10, offset, full_token,
            )?);
        } else {
            clean.push('0');
        }
        clean.push('.');
        if !frac_part.is_empty() {
            clean.push_str(&strip_and_validate_underscores(
                frac_part, 10, offset, full_token,
            )?);
        } else {
            clean.push('0');
        }
    } else {
        clean.push_str(&strip_and_validate_underscores(
            mantissa, 10, offset, full_token,
        )?);
    }

    if let Some(exp) = exp_part {
        clean.push('e');
        let exp_digits = if let Some(r) = exp.strip_prefix('-') {
            clean.push('-');
            r
        } else if let Some(r) = exp.strip_prefix('+') {
            clean.push('+');
            r
        } else {
            exp
        };
        clean.push_str(&strip_and_validate_underscores(
            exp_digits, 10, offset, full_token,
        )?);
    }

    if is_f32 {
        let v: f32 = clean
            .parse()
            .map_err(|_| SnbtError::new(offset, format!("invalid float literal '{full_token}'")))?;
        if !v.is_finite() {
            return Err(SnbtError::new(
                offset,
                format!("float literal overflows to non-finite value: '{full_token}'"),
            ));
        }
        Ok(Tag::Float(v))
    } else {
        let v: f64 = clean.parse().map_err(|_| {
            SnbtError::new(offset, format!("invalid double literal '{full_token}'"))
        })?;
        if !v.is_finite() {
            return Err(SnbtError::new(
                offset,
                format!("double literal overflows to non-finite value: '{full_token}'"),
            ));
        }
        Ok(Tag::Double(v))
    }
}
