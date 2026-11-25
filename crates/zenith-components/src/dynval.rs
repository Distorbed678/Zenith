//! A format-neutral value tree shared by the JSON and NBT forms of components.
//!
//! The JSON layer (`zenith_codec::json`) and the NBT model (`zenith_nbt`) differ in exactly the places the text
//! component spec cares about: JSON has real booleans and numbers that keep their text, NBT has typed numbers, typed
//! arrays and strings that may hold unpaired surrogates. [`Dyn`] holds either, remembering enough to apply each
//! format's own rules (spec A.6.1 rules 8 and 9, A.7.3). Decoding code is written once against [`Dyn`].
//!
//! JSON numbers stay [`JsonNumber`] and JSON objects keep their order (`JsonObject`), so number, `null` and
//! key-order handling is exactly the shared layer's.

use zenith_codec::codec::DataResult;
use zenith_codec::json::{Json, JsonNumber, JsonObject};
use zenith_nbt::java_number::{java_double_to_string, java_float_to_string};
use zenith_nbt::{Compound, List, NbtString, Tag};
use zenith_registry::Identifier;

/// A number with its source format's type.
#[derive(Debug, Clone, PartialEq)]
pub enum Num {
    /// A JSON number (decimal text kept).
    Json(JsonNumber),
    /// NBT byte.
    Byte(i8),
    /// NBT short.
    Short(i16),
    /// NBT int.
    Int(i32),
    /// NBT long.
    Long(i64),
    /// NBT float.
    Float(f32),
    /// NBT double.
    Double(f64),
}

impl Num {
    /// The value as a 32-bit integer (JSON: truncate + wrap; NBT: Java's `intValue`).
    pub fn to_i32(&self) -> Result<i32, String> {
        Ok(match self {
            Self::Json(n) => n.to_i32().map_err(|e| e.to_string())?,
            Self::Byte(v) => i32::from(*v),
            Self::Short(v) => i32::from(*v),
            Self::Int(v) => *v,
            Self::Long(v) => *v as i32,
            Self::Float(v) => *v as i32,
            Self::Double(v) => *v as i32,
        })
    }

    /// The value as a 64-bit integer.
    pub fn to_i64(&self) -> Result<i64, String> {
        Ok(match self {
            Self::Json(n) => n.to_i64().map_err(|e| e.to_string())?,
            Self::Byte(v) => i64::from(*v),
            Self::Short(v) => i64::from(*v),
            Self::Int(v) => i64::from(*v),
            Self::Long(v) => *v,
            Self::Float(v) => *v as i64,
            Self::Double(v) => *v as i64,
        })
    }

    /// The value as a 32-bit float.
    pub fn to_f32(&self) -> f32 {
        match self {
            Self::Json(n) => n.to_f32(),
            Self::Byte(v) => f32::from(*v),
            Self::Short(v) => f32::from(*v),
            Self::Int(v) => *v as f32,
            Self::Long(v) => *v as f32,
            Self::Float(v) => *v,
            Self::Double(v) => *v as f32,
        }
    }

    /// The value as a 64-bit float.
    pub fn to_f64(&self) -> f64 {
        match self {
            Self::Json(n) => n.to_f64(),
            Self::Byte(v) => f64::from(*v),
            Self::Short(v) => f64::from(*v),
            Self::Int(v) => f64::from(*v),
            Self::Long(v) => *v as f64,
            Self::Float(v) => f64::from(*v),
            Self::Double(v) => *v,
        }
    }

    /// Boolean reading (spec A.6.1 rules 8–9): NBT numbers are true iff non-zero as a double;
    /// JSON numbers are rejected (only JSON `true`/`false` are booleans).
    pub fn to_bool(&self) -> Result<bool, String> {
        match self {
            Self::Json(n) => Err(format!("Not a boolean: {}", n.text())),
            other => Ok(other.to_f64() != 0.0),
        }
    }

    /// The narrowed type JSON numbers take when they reach an in-memory number (translation arguments, conversion
    /// to NBT; spec A.7.3): using the number's exact decimal value, an integer within `i64` becomes the smallest
    /// of byte/short/int/long; otherwise it becomes the nearest double, stored as float if double→float→double
    /// round-trips unchanged, else double.
    pub fn narrowed(&self) -> Self {
        match self {
            Self::Json(n) => {
                if let Some(l) = exact_decimal_to_i64(n.text()) {
                    return narrow_i64(l);
                }
                let v = n.to_f64();
                let f = v as f32;
                if f64::from(f) == v {
                    Self::Float(f)
                } else {
                    Self::Double(v)
                }
            }
            other => other.clone(),
        }
    }

    /// Canonical decimal text of the stored numeric type (`1`, `1.5`, `1.0E10`, `Infinity`).
    pub fn to_display(&self) -> String {
        match self.narrowed() {
            Self::Byte(v) => v.to_string(),
            Self::Short(v) => v.to_string(),
            Self::Int(v) => v.to_string(),
            Self::Long(v) => v.to_string(),
            Self::Float(v) => java_float_to_string(v),
            Self::Double(v) => java_double_to_string(v),
            Self::Json(n) => n.text().to_owned(),
        }
    }
}

/// Parses a JSON number's exact decimal text as a signed 64-bit integer if and only if its exact
/// mathematical value is an integer within `i64::MIN..=i64::MAX` (spec A.7.3).
///
/// Handles fractional zeros (`2.0`, `-0.0`, `9007199254740993.0`) and exponents (`1e2`, `100e-2`),
/// while returning `None` for any non-zero fractional part (`1e-400`, `1.0000000000000000001`) or
/// out-of-range integer (`1e20`, `9223372036854775808`).
fn exact_decimal_to_i64(text: &str) -> Option<i64> {
    let (neg, rest) = match text.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, text),
    };
    let (mantissa, exp) = match rest.find(['e', 'E']) {
        Some(i) => {
            let e = &rest[i + 1..];
            let (eneg, edigits) = match e.strip_prefix('-') {
                Some(r) => (true, r),
                None => (false, e.strip_prefix('+').unwrap_or(e)),
            };
            let mut v: i128 = 0;
            for b in edigits.bytes() {
                if !b.is_ascii_digit() {
                    return None;
                }
                v = (v * 10 + i128::from(b - b'0')).min(1_000_000_000);
            }
            (&rest[..i], if eneg { -v } else { v })
        }
        None => (rest, 0),
    };
    let (int_part, frac_part) = match mantissa.find('.') {
        Some(i) => (&mantissa[..i], &mantissa[i + 1..]),
        None => (mantissa, ""),
    };

    // Find first and last non-zero digit across `int_part ++ frac_part`.
    let mut first: Option<usize> = None;
    let mut last: usize = 0;
    for (idx, b) in int_part.bytes().chain(frac_part.bytes()).enumerate() {
        if !b.is_ascii_digit() {
            return None;
        }
        if b != b'0' {
            if first.is_none() {
                first = Some(idx);
            }
            last = idx;
        }
    }

    // All digits are zero (0, -0, -0.0, 0.0e-500, ...): exact integer zero.
    let Some(first) = first else {
        return Some(0);
    };

    // Place value of the last non-zero digit is 10^p, where:
    // p = exp + int_part.len() - 1 - last.
    let p = exp + (int_part.len() as i128) - 1 - (last as i128);
    if p < 0 {
        return None;
    }

    let sig_len = (last - first + 1) as i128;
    if sig_len + p > 19 {
        return None;
    }

    let mut mag: u64 = 0;
    for b in int_part
        .bytes()
        .chain(frac_part.bytes())
        .skip(first)
        .take(last - first + 1)
    {
        mag = mag.checked_mul(10)?.checked_add(u64::from(b - b'0'))?;
    }
    for _ in 0..p {
        mag = mag.checked_mul(10)?;
    }

    if neg {
        if mag == i64::MIN.unsigned_abs() {
            Some(i64::MIN)
        } else if mag <= i64::MAX as u64 {
            Some(-(mag as i64))
        } else {
            None
        }
    } else if mag <= i64::MAX as u64 {
        Some(mag as i64)
    } else {
        None
    }
}

fn narrow_i64(l: i64) -> Num {
    if let Ok(b) = i8::try_from(l) {
        Num::Byte(b)
    } else if let Ok(s) = i16::try_from(l) {
        Num::Short(s)
    } else if let Ok(i) = i32::try_from(l) {
        Num::Int(i)
    } else {
        Num::Long(l)
    }
}

/// A format-neutral value.
#[derive(Debug, Clone, PartialEq)]
pub enum Dyn {
    /// JSON `null` (treated as "absent" by record readers).
    Null,
    /// A JSON boolean (NBT has none).
    Bool(bool),
    /// A number.
    Num(Num),
    /// A string (lossless UTF-16: unpaired surrogates survive in both JSON and NBT).
    Str(NbtString),
    /// A list (NBT lists may be heterogeneous).
    List(Vec<Dyn>),
    /// A map in source order (lossless UTF-16 keys).
    Map(Vec<(NbtString, Dyn)>),
    /// NBT byte array.
    ByteArray(Vec<i8>),
    /// NBT int array.
    IntArray(Vec<i32>),
    /// NBT long array.
    LongArray(Vec<i64>),
}

impl Dyn {
    /// A string value.
    pub fn string(s: impl Into<String>) -> Self {
        Self::Str(NbtString::from(s.into()))
    }

    /// An empty map.
    pub fn empty_map() -> Self {
        Self::Map(Vec::new())
    }

    /// Short type name for messages.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "boolean",
            Self::Num(_) => "number",
            Self::Str(_) => "string",
            Self::List(_) => "list",
            Self::Map(_) => "map",
            Self::ByteArray(_) => "byte array",
            Self::IntArray(_) => "int array",
            Self::LongArray(_) => "long array",
        }
    }

    /// Whether this value came from NBT-only constructs.
    pub fn is_map(&self) -> bool {
        matches!(self, Self::Map(_))
    }

    /// Map lookup that treats `null` as absent.
    pub fn get(&self, key: &str) -> Option<&Dyn> {
        match self {
            Self::Map(m) => m
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v)
                .filter(|v| !matches!(v, Dyn::Null)),
            _ => None,
        }
    }

    /// Converts from the shared JSON layer.
    pub fn from_json(json: &Json) -> Self {
        match json {
            Json::Null => Self::Null,
            Json::Bool(b) => Self::Bool(*b),
            Json::Number(n) => Self::Num(Num::Json(n.clone())),
            Json::String(s) => Self::Str(s.clone()),
            Json::Array(a) => Self::List(a.iter().map(Self::from_json).collect()),
            Json::Object(o) => Self::Map(
                o.iter()
                    .map(|(k, v)| (k.clone(), Self::from_json(v)))
                    .collect(),
            ),
        }
    }

    /// Converts from an NBT tag (compound order is the compound's own iteration order).
    pub fn from_tag(tag: &Tag) -> Self {
        match tag {
            Tag::End => Self::Null,
            Tag::Byte(v) => Self::Num(Num::Byte(*v)),
            Tag::Short(v) => Self::Num(Num::Short(*v)),
            Tag::Int(v) => Self::Num(Num::Int(*v)),
            Tag::Long(v) => Self::Num(Num::Long(*v)),
            Tag::Float(v) => Self::Num(Num::Float(*v)),
            Tag::Double(v) => Self::Num(Num::Double(*v)),
            Tag::ByteArray(v) => Self::ByteArray(v.clone()),
            Tag::String(s) => Self::Str(s.clone()),
            Tag::List(l) => Self::List(l.as_slice().iter().map(Self::from_tag).collect()),
            Tag::Compound(c) => Self::Map(
                c.iter()
                    .map(|(k, v)| (k.clone(), Self::from_tag(v)))
                    .collect(),
            ),
            Tag::IntArray(v) => Self::IntArray(v.clone()),
            Tag::LongArray(v) => Self::LongArray(v.clone()),
        }
    }

    /// Converts to JSON. Fails on values JSON cannot express (non-finite numbers) or NBT-only arrays are written
    /// as number lists.
    pub fn to_json(&self) -> Result<Json, String> {
        Ok(match self {
            Self::Null => Json::Null,
            Self::Bool(b) => Json::Bool(*b),
            Self::Num(n) => Json::Number(num_to_json(n)?),
            Self::Str(s) => Json::String(s.clone()),
            Self::List(l) => Json::Array(l.iter().map(Self::to_json).collect::<Result<_, _>>()?),
            Self::Map(m) => {
                let mut o = JsonObject::new();
                for (k, v) in m {
                    o.insert(k.clone(), v.to_json()?);
                }
                Json::Object(o)
            }
            Self::ByteArray(a) => Json::Array(
                a.iter()
                    .map(|v| Json::Number(JsonNumber::from_i64(i64::from(*v))))
                    .collect(),
            ),
            Self::IntArray(a) => Json::Array(
                a.iter()
                    .map(|v| Json::Number(JsonNumber::from_i64(i64::from(*v))))
                    .collect(),
            ),
            Self::LongArray(a) => Json::Array(
                a.iter()
                    .map(|v| Json::Number(JsonNumber::from_i64(*v)))
                    .collect(),
            ),
        })
    }

    /// Converts to NBT. JSON booleans become bytes; JSON numbers are narrowed like the library's JSON→NBT
    /// conversion does.
    pub fn to_tag(&self) -> Result<Tag, String> {
        Ok(match self {
            Self::Null => return Err("null cannot be converted to NBT".to_owned()),
            Self::Bool(b) => Tag::Byte(i8::from(*b)),
            Self::Num(n) => match n.narrowed() {
                Num::Byte(v) => Tag::Byte(v),
                Num::Short(v) => Tag::Short(v),
                Num::Int(v) => Tag::Int(v),
                Num::Long(v) => Tag::Long(v),
                Num::Float(v) => Tag::Float(v),
                Num::Double(v) => Tag::Double(v),
                Num::Json(_) => unreachable!("narrowed never returns Json"),
            },
            Self::Str(s) => Tag::String(s.clone()),
            Self::List(l) => {
                let mut list = List::new();
                for v in l {
                    list.push(v.to_tag()?);
                }
                Tag::List(list)
            }
            Self::Map(m) => {
                let mut c = Compound::new();
                for (k, v) in m {
                    c.insert(k.clone(), v.to_tag()?);
                }
                Tag::Compound(c)
            }
            Self::ByteArray(a) => Tag::ByteArray(a.clone()),
            Self::IntArray(a) => Tag::IntArray(a.clone()),
            Self::LongArray(a) => Tag::LongArray(a.clone()),
        })
    }
}

fn num_to_json(n: &Num) -> Result<JsonNumber, String> {
    Ok(match n {
        Num::Json(j) => j.clone(),
        Num::Byte(v) => JsonNumber::from_i64(i64::from(*v)),
        Num::Short(v) => JsonNumber::from_i64(i64::from(*v)),
        Num::Int(v) => JsonNumber::from_i64(i64::from(*v)),
        Num::Long(v) => JsonNumber::from_i64(*v),
        Num::Float(v) if v.is_finite() => JsonNumber::from_text(java_float_to_string(*v)),
        Num::Double(v) if v.is_finite() => JsonNumber::from_text(java_double_to_string(*v)),
        _ => return Err("JSON does not allow non-finite numbers".to_owned()),
    })
}

fn describe(d: &Dyn) -> String {
    let s = match d {
        Dyn::Str(s) => format!("\"{}\"", s.to_string_lossy()),
        Dyn::Num(n) => n.to_display(),
        Dyn::Bool(b) => b.to_string(),
        other => other.kind().to_owned(),
    };
    if s.chars().count() > 60 {
        let mut t: String = s.chars().take(60).collect();
        t.push('…');
        t
    } else {
        s
    }
}

// ----------------------------------------------------------------------------------------------------------
// Primitive readers (no coercion; JSON `null` is an error outside records)
// ----------------------------------------------------------------------------------------------------------

fn hard<T>(m: String) -> DataResult<T> {
    DataResult::Failure(m)
}

/// A string (a number never decodes as a string).
pub fn read_string(d: &Dyn) -> DataResult<String> {
    match d {
        Dyn::Str(s) => DataResult::Success(s.to_string_lossy().into_owned()),
        other => DataResult::error(format!("Not a string: {}", describe(other))),
    }
}

/// A string kept lossless (unpaired surrogates survive).
pub fn read_nbt_string(d: &Dyn) -> DataResult<NbtString> {
    match d {
        Dyn::Str(s) => DataResult::Success(s.clone()),
        other => DataResult::error(format!("Not a string: {}", describe(other))),
    }
}

/// A boolean: JSON `true`/`false` only (numbers rejected), NBT numbers (non-zero as double).
pub fn read_bool(d: &Dyn) -> DataResult<bool> {
    match d {
        Dyn::Bool(b) => DataResult::Success(*b),
        Dyn::Num(n) => match n.to_bool() {
            Ok(b) => DataResult::Success(b),
            Err(m) => DataResult::error(m),
        },
        other => DataResult::error(format!("Not a boolean: {}", describe(other))),
    }
}

/// A 32-bit integer.
pub fn read_int(d: &Dyn) -> DataResult<i32> {
    match d {
        Dyn::Num(n) => match n.to_i32() {
            Ok(v) => DataResult::Success(v),
            Err(m) => hard(m),
        },
        other => DataResult::error(format!("Not a number: {}", describe(other))),
    }
}

/// A 64-bit integer.
pub fn read_long(d: &Dyn) -> DataResult<i64> {
    match d {
        Dyn::Num(n) => match n.to_i64() {
            Ok(v) => DataResult::Success(v),
            Err(m) => hard(m),
        },
        other => DataResult::error(format!("Not a number: {}", describe(other))),
    }
}

/// A 32-bit float.
pub fn read_float(d: &Dyn) -> DataResult<f32> {
    match d {
        Dyn::Num(n) => DataResult::Success(n.to_f32()),
        other => DataResult::error(format!("Not a number: {}", describe(other))),
    }
}

/// A 64-bit float.
pub fn read_double(d: &Dyn) -> DataResult<f64> {
    match d {
        Dyn::Num(n) => DataResult::Success(n.to_f64()),
        other => DataResult::error(format!("Not a number: {}", describe(other))),
    }
}

/// An identifier (`minecraft` namespace by default).
pub fn read_identifier(d: &Dyn) -> DataResult<Identifier> {
    match d {
        Dyn::Str(s) => {
            let s = s.to_string_lossy();
            match Identifier::parse(&s) {
                Ok(id) => DataResult::Success(id),
                Err(_) => DataResult::error(format!("Not a valid resource location: {s}")),
            }
        }
        other => DataResult::error(format!("Not a string: {}", describe(other))),
    }
}

/// Elements of a list-like value. JSON arrays and NBT lists both count; NBT typed arrays count as lists of their
/// numbers.
pub fn read_list(d: &Dyn) -> DataResult<Vec<Dyn>> {
    match d {
        Dyn::List(l) => DataResult::Success(l.clone()),
        Dyn::ByteArray(a) => {
            DataResult::Success(a.iter().map(|v| Dyn::Num(Num::Byte(*v))).collect())
        }
        Dyn::IntArray(a) => DataResult::Success(a.iter().map(|v| Dyn::Num(Num::Int(*v))).collect()),
        Dyn::LongArray(a) => {
            DataResult::Success(a.iter().map(|v| Dyn::Num(Num::Long(*v))).collect())
        }
        other => DataResult::error(format!("Not a list: {}", describe(other))),
    }
}

/// Entries of a map value.
pub fn read_map(d: &Dyn) -> DataResult<&[(NbtString, Dyn)]> {
    match d {
        Dyn::Map(m) => DataResult::Success(m.as_slice()),
        other => DataResult::error(format!("Not a map: {}", describe(other))),
    }
}

// ----------------------------------------------------------------------------------------------------------
// Record reader
// ----------------------------------------------------------------------------------------------------------

/// Reads fields of a map with the framework's record rules (unknown keys ignored, `null` = absent, all errors
/// collected and joined with `"; "`, partial results, strict vs lenient optionals). This is `RecordReader` from
/// `zenith_codec::codec` over [`Dyn`].
#[derive(Debug)]
pub struct Rec<'a> {
    value: &'a Dyn,
    errors: Vec<String>,
    failure: Option<String>,
}

impl<'a> Rec<'a> {
    /// A reader over `value`; a non-map yields a "Not a map" error on [`Rec::finish`].
    pub fn new(value: &'a Dyn) -> Self {
        let mut r = Self {
            value,
            errors: Vec::new(),
            failure: None,
        };
        if !value.is_map() {
            r.errors.push(format!("Not a map: {}", describe(value)));
        }
        r
    }

    /// The raw field value, if present and not `null`.
    pub fn raw(&self, name: &str) -> Option<&'a Dyn> {
        self.value.get(name)
    }

    fn live(&self) -> bool {
        self.failure.is_none() && self.value.is_map()
    }

    fn take<T>(&mut self, name: &str, r: DataResult<T>) -> Option<T> {
        match r {
            DataResult::Success(v) => Some(v),
            DataResult::Error { message, partial } => {
                self.errors.push(format!("{name}: {message}"));
                partial
            }
            DataResult::Failure(m) => {
                self.failure = Some(m);
                None
            }
        }
    }

    /// A required field.
    pub fn required<T>(&mut self, name: &str, f: impl FnOnce(&Dyn) -> DataResult<T>) -> Option<T> {
        if !self.live() {
            return None;
        }
        match self.raw(name) {
            None => {
                self.errors.push(format!("No key {name}"));
                None
            }
            Some(v) => {
                let r = f(v);
                self.take(name, r)
            }
        }
    }

    /// A strict optional field: absent gives the default; present-but-invalid is an error (default in partial).
    pub fn optional<T>(
        &mut self,
        name: &str,
        default: T,
        f: impl FnOnce(&Dyn) -> DataResult<T>,
    ) -> T {
        if !self.live() {
            return default;
        }
        match self.raw(name) {
            None => default,
            Some(v) => {
                let r = f(v);
                self.take(name, r).unwrap_or(default)
            }
        }
    }

    /// A strict optional field without a default.
    pub fn optional_opt<T>(
        &mut self,
        name: &str,
        f: impl FnOnce(&Dyn) -> DataResult<T>,
    ) -> Option<T> {
        if !self.live() {
            return None;
        }
        match self.raw(name) {
            None => None,
            Some(v) => {
                let r = f(v);
                self.take(name, r)
            }
        }
    }

    /// A lenient optional field: any problem is silently the default.
    pub fn lenient<T>(
        &mut self,
        name: &str,
        default: T,
        f: impl FnOnce(&Dyn) -> DataResult<T>,
    ) -> T {
        if !self.live() {
            return default;
        }
        match self.raw(name) {
            None => default,
            Some(v) => match f(v) {
                DataResult::Success(t) => t,
                DataResult::Error { .. } => default,
                DataResult::Failure(m) => {
                    self.failure = Some(m);
                    default
                }
            },
        }
    }

    /// Records an extra validation error.
    pub fn push_error(&mut self, message: impl Into<String>) {
        self.errors.push(message.into());
    }

    /// Whether anything went wrong so far.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty() || self.failure.is_some()
    }

    /// Finishes: success, or an error carrying `build()`'s value as the partial.
    pub fn finish<R>(self, build: impl FnOnce() -> Option<R>) -> DataResult<R> {
        if let Some(m) = self.failure {
            return DataResult::Failure(m);
        }
        if self.errors.is_empty() {
            match build() {
                Some(v) => DataResult::Success(v),
                None => DataResult::error("incomplete record"),
            }
        } else {
            DataResult::Error {
                message: self.errors.join("; "),
                partial: if self.value.is_map() { build() } else { None },
            }
        }
    }
}

/// Builds a map in order, skipping nothing; used by encoders.
#[derive(Debug, Default)]
pub struct MapBuilder {
    entries: Vec<(NbtString, Dyn)>,
}

impl MapBuilder {
    /// An empty builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an entry (a repeated key replaces the earlier value in place).
    pub fn put(&mut self, key: impl Into<NbtString>, value: Dyn) -> &mut Self {
        let key = key.into();
        if let Some(e) = self.entries.iter_mut().find(|(k, _)| *k == key) {
            e.1 = value;
        } else {
            self.entries.push((key, value));
        }
        self
    }

    /// Finishes into a map value.
    pub fn build(self) -> Dyn {
        Dyn::Map(self.entries)
    }
}
