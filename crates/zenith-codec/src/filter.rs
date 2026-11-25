//! The `filter` section (spec §3.7, §5.3): regex patterns with Java `Pattern` syntax and unanchored search.
//!
//! Patterns are compiled with `fancy-regex` (look-around and backreferences like Java). A small translation layer
//! rewrites Java-only syntax: `\Q…\E` quoting and the `\Z` anchor. `^` and `$` behave as in Java without the
//! MULTILINE flag for the strings that matter here (namespaces and resource paths never contain line breaks).

use fancy_regex::Regex;

use crate::codec::{Codec, DataResult, RecordReader, list};
use crate::json::Json;

/// A compiled Java-syntax regular expression, searched unanchored (`Matcher.find`).
#[derive(Debug, Clone)]
pub struct JavaRegex {
    source: String,
    regex: Regex,
}

impl JavaRegex {
    /// Compiles `pattern`.
    pub fn new(pattern: &str) -> Result<Self, String> {
        let translated = translate_java_pattern(pattern);
        match Regex::new(&translated) {
            Ok(regex) => Ok(Self {
                source: pattern.to_owned(),
                regex,
            }),
            Err(e) => Err(format!("Invalid regex pattern '{pattern}': {e}")),
        }
    }

    /// The original pattern text.
    pub fn as_str(&self) -> &str {
        &self.source
    }

    /// Whether the pattern matches **anywhere** in `text`.
    pub fn find_in(&self, text: &str) -> bool {
        // A runtime error (backtracking limit) counts as "no match".
        self.regex.is_match(text).unwrap_or(false)
    }
}

/// Rewrites Java-only regex syntax into the dialect `fancy-regex` understands.
pub fn translate_java_pattern(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('Q') => {
                let mut literal = String::new();
                loop {
                    match chars.next() {
                        None => break,
                        Some('\\') if chars.peek() == Some(&'E') => {
                            chars.next();
                            break;
                        }
                        Some(ch) => literal.push(ch),
                    }
                }
                out.push_str(&fancy_regex::escape(&literal));
            }
            Some('Z') => out.push_str("(?=\\n?\\z)"),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// One `block` entry: either pattern may be absent (absent matches everything).
#[derive(Debug, Clone)]
pub struct FilterEntry {
    /// Namespace pattern.
    pub namespace: Option<JavaRegex>,
    /// Path pattern.
    pub path: Option<JavaRegex>,
}

/// A decoded `filter` section.
#[derive(Debug, Clone, Default)]
pub struct PackFilter {
    /// The `block` entries.
    pub block: Vec<FilterEntry>,
}

impl PackFilter {
    /// Whether the filter is active for namespace `ns`: some entry's namespace pattern finds a match anywhere in
    /// it, or has no namespace pattern.
    pub fn namespace_active(&self, ns: &str) -> bool {
        self.block
            .iter()
            .any(|e| e.namespace.as_ref().is_none_or(|r| r.find_in(ns)))
    }

    /// Whether the filter blocks the full resource path (for example `recipe/stone.json`): some entry's path
    /// pattern finds a match anywhere in it, or has no path pattern.
    pub fn blocks_path(&self, path: &str) -> bool {
        self.block
            .iter()
            .any(|e| e.path.as_ref().is_none_or(|r| r.find_in(path)))
    }
}

struct RegexCodec;

impl Codec<JavaRegex> for RegexCodec {
    fn decode(&self, input: &Json) -> DataResult<JavaRegex> {
        match input {
            Json::String(s) => match JavaRegex::new(&s.to_string_lossy()) {
                Ok(r) => DataResult::Success(r),
                Err(m) => DataResult::error(m),
            },
            other => DataResult::error(format!("Not a string: {}", other.type_name())),
        }
    }
    fn encode(&self, value: &JavaRegex) -> DataResult<Json> {
        DataResult::Success(Json::string(value.source.clone()))
    }
}

struct FilterEntryCodec;

impl Codec<FilterEntry> for FilterEntryCodec {
    fn decode(&self, input: &Json) -> DataResult<FilterEntry> {
        let mut r = RecordReader::new(input);
        let namespace = r.optional_opt("namespace", &RegexCodec);
        let path = r.optional_opt("path", &RegexCodec);
        r.finish(|| Some(FilterEntry { namespace, path }))
    }
    fn encode(&self, value: &FilterEntry) -> DataResult<Json> {
        let mut w = crate::codec::RecordWriter::new();
        w.field_opt("namespace", &RegexCodec, value.namespace.as_ref());
        w.field_opt("path", &RegexCodec, value.path.as_ref());
        w.finish()
    }
}

/// Codec for the `filter` section.
#[derive(Debug, Clone, Copy, Default)]
pub struct PackFilterCodec;

impl Codec<PackFilter> for PackFilterCodec {
    fn decode(&self, input: &Json) -> DataResult<PackFilter> {
        let mut r = RecordReader::new(input);
        let block = r.required("block", &list(FilterEntryCodec));
        r.finish(|| Some(PackFilter { block: block? }))
    }
    fn encode(&self, value: &PackFilter) -> DataResult<Json> {
        let mut w = crate::codec::RecordWriter::new();
        w.field("block", &list(FilterEntryCodec), &value.block);
        w.finish()
    }
}

impl std::fmt::Debug for FilterEntryCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FilterEntryCodec")
    }
}

impl std::fmt::Debug for RegexCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RegexCodec")
    }
}
