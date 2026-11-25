//! A small decode/encode framework with the semantics of spec §7.2–§7.6.
//!
//! * No type coercion; JSON `null` is "absent" for record fields and an error elsewhere.
//! * Integers come from any JSON number by truncation and wrap-around; floats parse directly from text.
//! * Records ignore unknown keys, collect **all** field errors (joined with `"; "`) and can produce partial results.
//! * Optional fields are strict (invalid ⇒ error, default in the partial) or lenient (invalid ⇒ default, silently).
//! * Encoding omits fields equal to their default and empty optionals.

use std::collections::BTreeMap;
use std::fmt;

use zenith_registry::Identifier;

use crate::json::{Json, JsonNumber, JsonObject, NumberError, ParseMode};
use crate::log::{LogExt, LogSink};

/// Outcome of one decode or encode step (spec §7.5).
#[derive(Debug, Clone, PartialEq)]
pub enum DataResult<T> {
    /// A value.
    Success(T),
    /// A message plus an optional best-effort value (good list elements, defaults for bad optional fields…).
    Error {
        /// What went wrong (several messages are joined with `"; "`).
        message: String,
        /// Best-effort value, if any.
        partial: Option<T>,
    },
    /// A hard failure (an exception in the game): not a decode error. It aborts record decoding immediately and
    /// reaches the consumer, which catches it per element.
    Failure(String),
}

impl<T> DataResult<T> {
    /// An error without a partial value.
    pub fn error(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
            partial: None,
        }
    }

    /// An error with a partial value.
    pub fn error_with_partial(message: impl Into<String>, partial: T) -> Self {
        Self::Error {
            message: message.into(),
            partial: Some(partial),
        }
    }

    /// Whether this is a full success.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success(_))
    }

    /// The value of a success, else `None` (partials are not successes).
    pub fn success(self) -> Option<T> {
        match self {
            Self::Success(v) => Some(v),
            _ => None,
        }
    }

    /// The value of a success **or** the partial value of an error.
    pub fn result_or_partial(self) -> Option<T> {
        match self {
            Self::Success(v) => Some(v),
            Self::Error { partial, .. } => partial,
            Self::Failure(_) => None,
        }
    }

    /// The error message, if any (including hard failures).
    pub fn message(&self) -> Option<&str> {
        match self {
            Self::Success(_) => None,
            Self::Error { message, .. } | Self::Failure(message) => Some(message),
        }
    }

    /// Maps the value (and the partial value).
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> DataResult<U> {
        match self {
            Self::Success(v) => DataResult::Success(f(v)),
            Self::Error { message, partial } => DataResult::Error {
                message,
                partial: partial.map(f),
            },
            Self::Failure(m) => DataResult::Failure(m),
        }
    }

    /// Chains a step that depends on the value. An error with a partial value keeps its message; the chained step
    /// does not run on partials.
    pub fn and_then<U>(self, f: impl FnOnce(T) -> DataResult<U>) -> DataResult<U> {
        match self {
            Self::Success(v) => f(v),
            Self::Error { message, .. } => DataResult::error(message),
            Self::Failure(m) => DataResult::Failure(m),
        }
    }

    /// **Demand success** (spec §7.5): a partial result counts as failure.
    pub fn demand(self) -> Result<T, CodecError> {
        match self {
            Self::Success(v) => Ok(v),
            Self::Error { message, .. } => Err(CodecError {
                message,
                hard: false,
            }),
            Self::Failure(message) => Err(CodecError {
                message,
                hard: true,
            }),
        }
    }

    /// **Accept partial** (spec §7.5): logs the message and keeps the partial value.
    pub fn accept_partial(self, log: &dyn LogSink, context: &str) -> Option<T> {
        match self {
            Self::Success(v) => Some(v),
            Self::Error { message, partial } => {
                log.error(format!("{context}: {message}"));
                partial
            }
            Self::Failure(message) => {
                log.error(format!("{context}: {message}"));
                None
            }
        }
    }

    /// **Fall back** (spec §7.5): any problem gives the default, silently.
    pub fn or_default_silently(self, default: T) -> T {
        self.success().unwrap_or(default)
    }
}

/// An error produced when a consumer demands success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodecError {
    /// Message.
    pub message: String,
    /// Whether this was a hard failure (exception) rather than a decode error.
    pub hard: bool,
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CodecError {}

/// A bidirectional JSON codec for values of type `T`.
pub trait Codec<T> {
    /// Decodes a JSON value.
    fn decode(&self, input: &Json) -> DataResult<T>;
    /// Encodes a value to JSON.
    fn encode(&self, value: &T) -> DataResult<Json>;

    /// Decodes JSON *text* in strict data-file mode (empty text is `null`, which then fails the codec).
    fn parse_text(&self, bytes: &[u8]) -> DataResult<T> {
        match crate::json::parse_bytes(bytes, ParseMode::Data) {
            Ok(json) => self.decode(&json),
            Err(e) => DataResult::error(format!("Invalid JSON: {e}")),
        }
    }
}

fn describe(input: &Json) -> String {
    let mut s = input.to_json_string();
    if s.len() > 80 {
        let mut cut = 80;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push('…');
    }
    s
}

fn number_failure<T>(e: NumberError) -> DataResult<T> {
    DataResult::Failure(e.to_string())
}

// ----------------------------------------------------------------------------------------------------------
// Primitives
// ----------------------------------------------------------------------------------------------------------

/// Booleans: JSON `true`/`false` only.
#[derive(Debug, Clone, Copy, Default)]
pub struct BoolCodec;

impl Codec<bool> for BoolCodec {
    fn decode(&self, input: &Json) -> DataResult<bool> {
        match input {
            Json::Bool(b) => DataResult::Success(*b),
            other => DataResult::error(format!("Not a boolean: {}", describe(other))),
        }
    }
    fn encode(&self, value: &bool) -> DataResult<Json> {
        DataResult::Success(Json::Bool(*value))
    }
}

/// Strings: JSON strings only.
#[derive(Debug, Clone, Copy, Default)]
pub struct StringCodec;

impl Codec<String> for StringCodec {
    fn decode(&self, input: &Json) -> DataResult<String> {
        match input {
            Json::String(s) => DataResult::Success(s.clone()),
            other => DataResult::error(format!("Not a string: {}", describe(other))),
        }
    }
    fn encode(&self, value: &String) -> DataResult<Json> {
        DataResult::Success(Json::String(value.clone()))
    }
}

macro_rules! int_codec {
    ($(#[$m:meta])* $name:ident, $t:ty, $conv:ident) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, Default)]
        pub struct $name;

        impl Codec<$t> for $name {
            fn decode(&self, input: &Json) -> DataResult<$t> {
                match input {
                    Json::Number(n) => match n.$conv() {
                        Ok(v) => DataResult::Success(v),
                        Err(e) => number_failure(e),
                    },
                    other => DataResult::error(format!("Not a number: {}", describe(other))),
                }
            }
            fn encode(&self, value: &$t) -> DataResult<Json> {
                DataResult::Success(Json::Number(JsonNumber::from_i64(i64::from(*value))))
            }
        }
    };
}

int_codec!(
    /// 8-bit integers: any JSON number, truncated, low 8 bits kept.
    ByteCodec, i8, to_i8
);
int_codec!(
    /// 16-bit integers.
    ShortCodec, i16, to_i16
);
int_codec!(
    /// 32-bit integers.
    IntCodec, i32, to_i32
);
int_codec!(
    /// 64-bit integers.
    LongCodec, i64, to_i64
);

/// 32-bit floats: the decimal text is rounded directly to the nearest `f32`.
#[derive(Debug, Clone, Copy, Default)]
pub struct FloatCodec;

impl Codec<f32> for FloatCodec {
    fn decode(&self, input: &Json) -> DataResult<f32> {
        match input {
            Json::Number(n) => DataResult::Success(n.to_f32()),
            other => DataResult::error(format!("Not a number: {}", describe(other))),
        }
    }
    fn encode(&self, value: &f32) -> DataResult<Json> {
        if value.is_finite() {
            DataResult::Success(Json::Number(JsonNumber::from_text(format!("{value}"))))
        } else {
            DataResult::error("JSON does not allow non-finite numbers")
        }
    }
}

/// 64-bit floats.
#[derive(Debug, Clone, Copy, Default)]
pub struct DoubleCodec;

impl Codec<f64> for DoubleCodec {
    fn decode(&self, input: &Json) -> DataResult<f64> {
        match input {
            Json::Number(n) => DataResult::Success(n.to_f64()),
            other => DataResult::error(format!("Not a number: {}", describe(other))),
        }
    }
    fn encode(&self, value: &f64) -> DataResult<Json> {
        if value.is_finite() {
            DataResult::Success(Json::Number(JsonNumber::from_text(format!("{value}"))))
        } else {
            DataResult::error("JSON does not allow non-finite numbers")
        }
    }
}

/// Identifiers (`namespace:path`, a missing namespace means `minecraft`).
#[derive(Debug, Clone, Copy, Default)]
pub struct IdentifierCodec;

impl Codec<Identifier> for IdentifierCodec {
    fn decode(&self, input: &Json) -> DataResult<Identifier> {
        match input {
            Json::String(s) => match Identifier::parse(s) {
                Ok(id) => DataResult::Success(id),
                Err(_) => DataResult::error(format!("Not a valid resource location: {s}")),
            },
            other => DataResult::error(format!("Not a string: {}", describe(other))),
        }
    }
    fn encode(&self, value: &Identifier) -> DataResult<Json> {
        DataResult::Success(Json::String(value.to_string()))
    }
}

// ----------------------------------------------------------------------------------------------------------
// Validated values
// ----------------------------------------------------------------------------------------------------------

/// A base codec plus a check that runs **after** decoding (ranges, non-empty strings, …).
pub struct Validated<C, F> {
    base: C,
    check: F,
}

impl<C, F> fmt::Debug for Validated<C, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Validated")
    }
}

/// Wraps `base` so that decoded values (and values about to be encoded) must pass `check`.
pub fn validated<T, C, F>(base: C, check: F) -> Validated<C, F>
where
    C: Codec<T>,
    F: Fn(&T) -> Result<(), String>,
{
    Validated { base, check }
}

impl<T, C, F> Codec<T> for Validated<C, F>
where
    C: Codec<T>,
    F: Fn(&T) -> Result<(), String>,
{
    fn decode(&self, input: &Json) -> DataResult<T> {
        match self.base.decode(input) {
            DataResult::Success(v) => match (self.check)(&v) {
                Ok(()) => DataResult::Success(v),
                Err(m) => DataResult::error_with_partial(m, v),
            },
            other => other,
        }
    }
    fn encode(&self, value: &T) -> DataResult<Json> {
        match (self.check)(value) {
            Ok(()) => self.base.encode(value),
            Err(m) => DataResult::error(m),
        }
    }
}

/// Non-negative 32-bit integer (checked after truncation/wrapping, so `-0.5` passes as `0`).
pub fn non_negative_int() -> impl Codec<i32> {
    validated(IntCodec, |v: &i32| {
        if *v >= 0 {
            Ok(())
        } else {
            Err(format!("Value must be non-negative: {v}"))
        }
    })
}

// ----------------------------------------------------------------------------------------------------------
// Composite shapes
// ----------------------------------------------------------------------------------------------------------

/// A JSON array of independently decoded elements.
#[derive(Debug, Clone)]
pub struct ListCodec<C> {
    element: C,
    min: usize,
    max: usize,
}

/// A list of `element`s with any size.
pub fn list<C>(element: C) -> ListCodec<C> {
    ListCodec {
        element,
        min: 0,
        max: usize::MAX,
    }
}

/// A list whose size must lie in `min..=max`.
pub fn sized_list<C>(element: C, min: usize, max: usize) -> ListCodec<C> {
    ListCodec { element, min, max }
}

impl<T, C: Codec<T>> Codec<Vec<T>> for ListCodec<C> {
    fn decode(&self, input: &Json) -> DataResult<Vec<T>> {
        let Json::Array(items) = input else {
            return DataResult::error(format!("Not a json array: {}", describe(input)));
        };
        let mut good = Vec::with_capacity(items.len());
        let mut errors: Vec<String> = Vec::new();
        for (i, item) in items.iter().enumerate() {
            match self.element.decode(item) {
                DataResult::Success(v) => good.push(v),
                DataResult::Error { message, .. } => {
                    errors.push(format!("Element {i}: {message}"));
                }
                DataResult::Failure(m) => return DataResult::Failure(m),
            }
        }
        if good.len() + errors.len() < self.min {
            errors.push(format!(
                "List is too short: {}, expected at least {}",
                items.len(),
                self.min
            ));
        }
        if items.len() > self.max {
            errors.push(format!(
                "List is too long: {}, expected at most {}",
                items.len(),
                self.max
            ));
            good.truncate(self.max);
        }
        if errors.is_empty() {
            DataResult::Success(good)
        } else {
            DataResult::error_with_partial(errors.join("; "), good)
        }
    }

    fn encode(&self, value: &Vec<T>) -> DataResult<Json> {
        if value.len() < self.min || value.len() > self.max {
            return DataResult::error(format!(
                "List size {} is outside {}..={}",
                value.len(),
                self.min,
                self.max
            ));
        }
        let mut out = Vec::with_capacity(value.len());
        for v in value {
            match self.element.encode(v) {
                DataResult::Success(j) => out.push(j),
                DataResult::Error { message, .. } => return DataResult::error(message),
                DataResult::Failure(m) => return DataResult::Failure(m),
            }
        }
        DataResult::Success(Json::Array(out))
    }
}

/// A JSON object whose keys are strings and whose values share one codec. Order is preserved.
#[derive(Debug, Clone)]
pub struct StringMapCodec<C> {
    value: C,
}

/// A string-keyed map codec.
pub fn string_map<C>(value: C) -> StringMapCodec<C> {
    StringMapCodec { value }
}

impl<T, C: Codec<T>> Codec<Vec<(String, T)>> for StringMapCodec<C> {
    fn decode(&self, input: &Json) -> DataResult<Vec<(String, T)>> {
        let Json::Object(obj) = input else {
            return DataResult::error(format!("Not a JSON object: {}", describe(input)));
        };
        let mut good = Vec::new();
        let mut errors = Vec::new();
        for (k, v) in obj.iter() {
            match self.value.decode(v) {
                DataResult::Success(t) => good.push((k.to_owned(), t)),
                DataResult::Error { message, .. } => {
                    errors.push(format!("Entry {k}: {message}"));
                }
                DataResult::Failure(m) => return DataResult::Failure(m),
            }
        }
        if errors.is_empty() {
            DataResult::Success(good)
        } else {
            DataResult::error_with_partial(errors.join("; "), good)
        }
    }

    fn encode(&self, value: &Vec<(String, T)>) -> DataResult<Json> {
        let mut obj = JsonObject::new();
        for (k, v) in value {
            match self.value.encode(v) {
                DataResult::Success(j) => obj.insert(k.clone(), j),
                DataResult::Error { message, .. } => return DataResult::error(message),
                DataResult::Failure(m) => return DataResult::Failure(m),
            }
        }
        DataResult::Success(Json::Object(obj))
    }
}

/// Either of two shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Either<A, B> {
    /// Decoded by the first codec.
    Left(A),
    /// Decoded by the second codec.
    Right(B),
}

/// Tries the first codec, then the second. The first **success** wins; partial results are dropped; when both fail
/// the message names both failures.
#[derive(Debug, Clone)]
pub struct EitherCodec<CA, CB> {
    first: CA,
    second: CB,
}

/// Builds an [`EitherCodec`].
pub fn either<CA, CB>(first: CA, second: CB) -> EitherCodec<CA, CB> {
    EitherCodec { first, second }
}

impl<A, B, CA: Codec<A>, CB: Codec<B>> Codec<Either<A, B>> for EitherCodec<CA, CB> {
    fn decode(&self, input: &Json) -> DataResult<Either<A, B>> {
        let first = match self.first.decode(input) {
            DataResult::Success(a) => return DataResult::Success(Either::Left(a)),
            DataResult::Failure(m) => return DataResult::Failure(m),
            DataResult::Error { message, .. } => message,
        };
        match self.second.decode(input) {
            DataResult::Success(b) => DataResult::Success(Either::Right(b)),
            DataResult::Failure(m) => DataResult::Failure(m),
            DataResult::Error { message, .. } => DataResult::error(format!(
                "Failed to parse either. First: {first}; Second: {message}"
            )),
        }
    }

    fn encode(&self, value: &Either<A, B>) -> DataResult<Json> {
        match value {
            Either::Left(a) => self.first.encode(a),
            Either::Right(b) => self.second.encode(b),
        }
    }
}

/// "One element or a list": a lone element is treated as a one-element list. Encoding writes a single element
/// bare, and anything else as an array.
#[derive(Debug, Clone)]
pub struct OneOrListCodec<C> {
    element: C,
}

/// Builds a [`OneOrListCodec`].
pub fn one_or_list<C>(element: C) -> OneOrListCodec<C> {
    OneOrListCodec { element }
}

impl<T, C: Codec<T>> Codec<Vec<T>> for OneOrListCodec<C> {
    fn decode(&self, input: &Json) -> DataResult<Vec<T>> {
        // First form: the element itself; second form: a list.
        let first = match self.element.decode(input) {
            DataResult::Success(v) => return DataResult::Success(vec![v]),
            DataResult::Failure(m) => return DataResult::Failure(m),
            DataResult::Error { message, .. } => message,
        };
        match (ListCodec {
            element: &self.element,
            min: 0,
            max: usize::MAX,
        })
        .decode(input)
        {
            DataResult::Success(v) => DataResult::Success(v),
            DataResult::Failure(m) => DataResult::Failure(m),
            DataResult::Error { message, .. } => DataResult::error(format!(
                "Failed to parse either. First: {first}; Second: {message}"
            )),
        }
    }

    fn encode(&self, value: &Vec<T>) -> DataResult<Json> {
        if value.len() == 1 {
            return self.element.encode(&value[0]);
        }
        ListCodec {
            element: &self.element,
            min: 0,
            max: usize::MAX,
        }
        .encode(value)
    }
}

impl<T, C: Codec<T> + ?Sized> Codec<T> for &C {
    fn decode(&self, input: &Json) -> DataResult<T> {
        (**self).decode(input)
    }
    fn encode(&self, value: &T) -> DataResult<Json> {
        (**self).encode(value)
    }
}

/// Reads the `"type"`-style discriminator of a type-dispatched object and hands the object to the chosen variant.
///
/// `variant` returns `None` for an unknown type id. Extra keys are ignored by the variant's own record schema.
pub fn decode_dispatch<T>(
    input: &Json,
    type_key: &str,
    variant: impl FnOnce(&str, &Json) -> Option<DataResult<T>>,
) -> DataResult<T> {
    let Json::Object(obj) = input else {
        return DataResult::error(format!("Not a JSON object: {}", describe(input)));
    };
    let Some(ty) = obj.get(type_key).filter(|v| !v.is_null()) else {
        return DataResult::error(format!("No key {type_key} in {}", describe(input)));
    };
    let Json::String(ty) = ty else {
        return DataResult::error(format!("Not a string: {}", describe(ty)));
    };
    match variant(ty, input) {
        Some(r) => r,
        None => DataResult::error(format!("Unknown type: {ty}")),
    }
}

// ----------------------------------------------------------------------------------------------------------
// Records
// ----------------------------------------------------------------------------------------------------------

/// Reads the fields of one JSON object, accumulating every error (spec §7.3).
///
/// ```
/// use zenith_codec::codec::{IntCodec, RecordReader};
/// use zenith_codec::json::{parse, ParseMode};
///
/// let json = parse(r#"{"a": 1, "b": "bad"}"#, ParseMode::Data).unwrap();
/// let mut r = RecordReader::new(&json);
/// let a = r.required("a", &IntCodec);
/// let b = r.optional("b", &IntCodec, 7);
/// let result = r.finish(|| Some((a?, b)));
/// assert_eq!(result.result_or_partial(), Some((1, 7)));
/// ```
#[derive(Debug)]
pub struct RecordReader<'a> {
    object: Option<&'a JsonObject>,
    not_object: Option<String>,
    errors: Vec<String>,
    failure: Option<String>,
}

impl<'a> RecordReader<'a> {
    /// Starts reading `input`; a non-object makes [`finish`](Self::finish) fail with "Not a JSON object".
    pub fn new(input: &'a Json) -> Self {
        match input {
            Json::Object(o) => Self {
                object: Some(o),
                not_object: None,
                errors: Vec::new(),
                failure: None,
            },
            other => Self {
                object: None,
                not_object: Some(format!("Not a JSON object: {}", describe(other))),
                errors: Vec::new(),
                failure: None,
            },
        }
    }

    /// The raw value of a field, with `null` treated as absent.
    pub fn raw(&self, name: &str) -> Option<&'a Json> {
        self.object
            .and_then(|o| o.get(name))
            .filter(|v| !v.is_null())
    }

    fn decode_field<T>(&mut self, name: &str, codec: &impl Codec<T>, value: &Json) -> Option<T> {
        match codec.decode(value) {
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

    /// A required field: absent or `null` is the error "No key `name`".
    pub fn required<T>(&mut self, name: &str, codec: &impl Codec<T>) -> Option<T> {
        if self.failure.is_some() || self.object.is_none() {
            return None;
        }
        match self.raw(name) {
            None => {
                self.errors.push(format!("No key {name}"));
                None
            }
            Some(v) => self.decode_field(name, codec, v),
        }
    }

    /// An optional field with a default (strict): absent/`null` ⇒ default; present but invalid ⇒ error, and the
    /// partial result uses the field's partial value or the default.
    pub fn optional<T>(&mut self, name: &str, codec: &impl Codec<T>, default: T) -> T {
        if self.failure.is_some() || self.object.is_none() {
            return default;
        }
        match self.raw(name) {
            None => default,
            Some(v) => self.decode_field(name, codec, v).unwrap_or(default),
        }
    }

    /// An optional field without a default (strict): absent/`null` ⇒ `None`; invalid ⇒ error.
    pub fn optional_opt<T>(&mut self, name: &str, codec: &impl Codec<T>) -> Option<T> {
        if self.failure.is_some() || self.object.is_none() {
            return None;
        }
        match self.raw(name) {
            None => None,
            Some(v) => self.decode_field(name, codec, v),
        }
    }

    /// A **lenient** optional field: present but invalid silently yields the default (a partially valid list becomes
    /// the default, not the valid part). A hard failure still propagates.
    pub fn lenient<T>(&mut self, name: &str, codec: &impl Codec<T>, default: T) -> T {
        if self.failure.is_some() || self.object.is_none() {
            return default;
        }
        match self.raw(name) {
            None => default,
            Some(v) => match codec.decode(v) {
                DataResult::Success(t) => t,
                DataResult::Error { .. } => default,
                DataResult::Failure(m) => {
                    self.failure = Some(m);
                    default
                }
            },
        }
    }

    /// Records an extra error message (for cross-field validation).
    pub fn push_error(&mut self, message: impl Into<String>) {
        self.errors.push(message.into());
    }

    /// Whether any error has been recorded so far.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty() || self.not_object.is_some() || self.failure.is_some()
    }

    /// Finishes: success if no errors; else an error whose message joins all field errors with `"; "` and whose
    /// partial value comes from `build` (which may return `None` when a required piece is missing).
    pub fn finish<R>(self, build: impl FnOnce() -> Option<R>) -> DataResult<R> {
        if let Some(m) = self.failure {
            return DataResult::Failure(m);
        }
        if let Some(m) = self.not_object {
            return DataResult::error(m);
        }
        let value = build();
        if self.errors.is_empty() {
            match value {
                Some(v) => DataResult::Success(v),
                None => DataResult::error("incomplete record"),
            }
        } else {
            DataResult::Error {
                message: self.errors.join("; "),
                partial: value,
            }
        }
    }
}

/// Builds a JSON object, omitting defaults and empty optionals (spec §7.3 "On encode").
#[derive(Debug, Default)]
pub struct RecordWriter {
    object: JsonObject,
    errors: Vec<String>,
}

impl RecordWriter {
    /// A new writer.
    pub fn new() -> Self {
        Self::default()
    }

    fn put<T>(&mut self, name: &str, codec: &impl Codec<T>, value: &T) {
        match codec.encode(value) {
            DataResult::Success(j) => self.object.insert(name, j),
            DataResult::Error { message, .. } | DataResult::Failure(message) => {
                self.errors.push(format!("{name}: {message}"));
            }
        }
    }

    /// A required field: always written.
    pub fn field<T>(&mut self, name: &str, codec: &impl Codec<T>, value: &T) -> &mut Self {
        self.put(name, codec, value);
        self
    }

    /// A field with a default: omitted when equal to the default.
    pub fn field_default<T: PartialEq>(
        &mut self,
        name: &str,
        codec: &impl Codec<T>,
        value: &T,
        default: &T,
    ) -> &mut Self {
        if value != default {
            self.put(name, codec, value);
        }
        self
    }

    /// An optional field: omitted when empty.
    pub fn field_opt<T>(
        &mut self,
        name: &str,
        codec: &impl Codec<T>,
        value: Option<&T>,
    ) -> &mut Self {
        if let Some(v) = value {
            self.put(name, codec, v);
        }
        self
    }

    /// Finishes the object.
    pub fn finish(self) -> DataResult<Json> {
        if self.errors.is_empty() {
            DataResult::Success(Json::Object(self.object))
        } else {
            DataResult::error(self.errors.join("; "))
        }
    }
}

// ----------------------------------------------------------------------------------------------------------
// Consumers (spec §7.6, §7.7)
// ----------------------------------------------------------------------------------------------------------

/// How each consumer reacts to a decode or parse error (spec §7.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorPolicy {
    /// Demand success; a partial result is a failure (pack sections).
    DemandSuccess,
    /// Silently fall back to a default (world `DataPacks`, `enabled_features`).
    LenientDefault,
    /// Log the message and keep the partial value (the rest of the world configuration).
    AcceptPartial,
    /// Demand success, record the error, and abort the **whole batch** afterwards (registry elements).
    AbortBatch,
    /// Skip only the failing file and continue (tags, functions).
    SkipFile,
    /// Warnings are logged; a fatal problem aborts (loot validation).
    WarnThenAbortOnFatal,
}

/// The consumers named in spec §7.6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Consumer {
    /// `pack.mcmeta` sections.
    PackMetaSection,
    /// World `DataPacks` / `enabled_features`.
    WorldPackConfig,
    /// The rest of the world configuration.
    WorldConfigRest,
    /// Dynamic-registry element (world, dimension, reloadable layers).
    RegistryElement,
    /// Registry freeze and validators.
    RegistryFreeze,
    /// Loot validation.
    LootValidation,
    /// A tag file.
    TagFile,
    /// A function file.
    FunctionFile,
}

impl Consumer {
    /// The policy for this consumer.
    pub fn policy(self) -> ErrorPolicy {
        match self {
            Self::PackMetaSection => ErrorPolicy::DemandSuccess,
            Self::WorldPackConfig => ErrorPolicy::LenientDefault,
            Self::WorldConfigRest => ErrorPolicy::AcceptPartial,
            Self::RegistryElement | Self::RegistryFreeze => ErrorPolicy::AbortBatch,
            Self::LootValidation => ErrorPolicy::WarnThenAbortOnFatal,
            Self::TagFile | Self::FunctionFile => ErrorPolicy::SkipFile,
        }
    }
}

/// One recorded registry-element failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementError {
    /// Registry id.
    pub registry: Identifier,
    /// Element id.
    pub element: Identifier,
    /// Pack the file came from.
    pub pack: String,
    /// The wrapped codec message.
    pub message: String,
}

/// Collects element errors of one registry batch. Any recorded error aborts the entire load; all of them are
/// reported together, grouped by registry and sorted by element (spec §7.7).
#[derive(Debug, Default)]
pub struct RegistryLoadErrors {
    errors: Vec<ElementError>,
}

impl RegistryLoadErrors {
    /// A new, empty collection.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one error.
    pub fn record(&mut self, error: ElementError) {
        self.errors.push(error);
    }

    /// Whether the batch must abort.
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    /// Number of recorded errors.
    pub fn len(&self) -> usize {
        self.errors.len()
    }

    /// Parses and decodes one element, **demanding success**: malformed JSON, a schema error or a partial result
    /// records an error and yields `None`. A hard failure is caught per element and recorded the same way.
    pub fn load_element<T>(
        &mut self,
        registry: &Identifier,
        element: &Identifier,
        pack: &str,
        bytes: &[u8],
        codec: &impl Codec<T>,
    ) -> Option<T> {
        match codec.parse_text(bytes).demand() {
            Ok(v) => Some(v),
            Err(e) => {
                self.record(ElementError {
                    registry: registry.clone(),
                    element: element.clone(),
                    pack: pack.to_owned(),
                    message: e.message,
                });
                None
            }
        }
    }

    /// Errors grouped by registry (sorted by registry id), each group sorted by element id.
    pub fn grouped(&self) -> BTreeMap<Identifier, Vec<&ElementError>> {
        let mut map: BTreeMap<Identifier, Vec<&ElementError>> = BTreeMap::new();
        for e in &self.errors {
            map.entry(e.registry.clone()).or_default().push(e);
        }
        for v in map.values_mut() {
            v.sort_by(|a, b| a.element.cmp(&b.element));
        }
        map
    }

    /// The log block: one section per registry.
    pub fn log_report(&self) -> String {
        let mut out = String::new();
        for (registry, errs) in self.grouped() {
            out.push_str(&format!("Registry {registry}:\n"));
            for e in errs {
                out.push_str(&format!(
                    "  Failed to parse {} from pack {}: {}\n",
                    e.element, e.pack, e.message
                ));
            }
        }
        out
    }

    /// The crash-report lines `<registry>/<element>: <message>`, sorted by registry then element.
    pub fn crash_report_lines(&self) -> Vec<String> {
        self.grouped()
            .into_iter()
            .flat_map(|(registry, errs)| {
                errs.into_iter()
                    .map(move |e| format!("{registry}/{}: {}", e.element, e.message))
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}
