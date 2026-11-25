//! Identifiers (`namespace:path`), typed resource keys, and tag keys.
//!
//! Implements Section 1 of `docs/specs/registry/identifiers-registries-tags.md`.

use std::cmp::Ordering;
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// The default namespace used when omitted in single-string parsing (`minecraft`).
pub const DEFAULT_NAMESPACE: &str = "minecraft";

/// The separator between namespace and path (`:`).
pub const SEPARATOR: char = ':';

/// Maximum identifier length on the network in UTF-16 code units (`32,767`).
pub const NETWORK_MAX_UTF16_UNITS: usize = 32_767;

/// Maximum identifier length on the network in UTF-8 wire bytes (`3 * 32,767 = 98,301`).
pub const NETWORK_MAX_UTF8_BYTES: usize = 3 * NETWORK_MAX_UTF16_UNITS;

/// Errors raised when parsing, validating, resolving, or decoding identifiers and keys.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IdentifierError {
    /// Namespace contained an invalid character or the reserved value `".."`.
    #[error("Non [a-z0-9_.-] character in namespace of location: {}", escape_java_string(.raw))]
    InvalidNamespace {
        /// Combined `<namespace>:<path>` string (escaped when formatted).
        raw: String,
    },

    /// Path contained an invalid character (including a second `:`).
    #[error("Non [a-z0-9/._-] character in path of location: {}", escape_java_string(.raw))]
    InvalidPath {
        /// Combined `<namespace>:<path>` string (escaped when formatted).
        raw: String,
    },

    /// Non-empty command-stream parsing consumed zero valid identifier characters.
    #[error("argument.id.invalid: empty identifier")]
    EmptyCommandIdentifier,

    /// Filesystem resolution attempted to escape the root or namespace directory.
    #[error("Identifier path escapes root directory: {}", escape_java_string(.identifier))]
    PathEscapesRoot {
        /// Canonical identifier string that attempted to escape.
        identifier: String,
    },

    /// Network wire string exceeded protocol bounds or was malformed/truncated.
    #[error("Invalid network identifier encoding: {reason}")]
    NetworkEncoding {
        /// Description of the wire framing failure.
        reason: String,
    },

    /// A hashed tag identifier was expected (`#<id>`) but the leading `#` was absent.
    #[error("Not a tag id: {}", escape_java_string(.raw))]
    NotATagId {
        /// Raw input string (escaped when formatted).
        raw: String,
    },
}

/// Escape control characters, quotes, backslashes, and non-ASCII code points using
/// Java-style string escapes so raw control/non-ASCII characters never appear in diagnostics.
pub fn escape_java_string(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\u{0008}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{000C}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (' '..='~').contains(&c) => out.push(c),
            c => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    use std::fmt::Write as _;
                    let _ = write!(out, "\\u{unit:04X}");
                }
            }
        }
    }
    out
}

/// Returns `true` if `ch` is allowed in an identifier namespace (`[a-z0-9_.-]`).
#[inline]
pub const fn is_valid_namespace_char(ch: char) -> bool {
    matches!(ch, 'a'..='z' | '0'..='9' | '_' | '-' | '.')
}

/// Returns `true` if `ch` is allowed in an identifier path (`[a-z0-9/._-]`).
#[inline]
pub const fn is_valid_path_char(ch: char) -> bool {
    matches!(ch, 'a'..='z' | '0'..='9' | '_' | '-' | '.' | '/')
}

/// Returns `true` if `ch` is consumed by command-stream identifier parsing (`[0-9a-z_:./-]`).
#[inline]
pub const fn is_valid_command_char(ch: char) -> bool {
    matches!(ch, 'a'..='z' | '0'..='9' | '_' | ':' | '.' | '/' | '-')
}

/// Validates a namespace string.
///
/// Empty strings pass character validation; `".."` is explicitly rejected even though `.`
/// is an allowed character.
pub fn is_valid_namespace(namespace: &str) -> bool {
    namespace != ".." && namespace.chars().all(is_valid_namespace_char)
}

/// Validates a path string. Empty strings pass character validation.
pub fn is_valid_path(path: &str) -> bool {
    path.chars().all(is_valid_path_char)
}

/// A two-part ASCII identifier (`<namespace>:<path>`).
///
/// Natural ordering (`Ord`) compares **`path` first**, then **`namespace` second**.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Identifier {
    namespace: String,
    path: String,
}

impl Identifier {
    /// Parse an identifier from a single string using the spec's ordered rules:
    ///
    /// 1. Split on the **first** `:`.
    /// 2. If no `:` is present, or `:` is at index 0, use the default namespace (`minecraft`).
    /// 3. Validate `namespace` first, then `path`.
    pub fn parse(input: &str) -> Result<Self, IdentifierError> {
        match input.find(SEPARATOR) {
            None => Self::from_parts(DEFAULT_NAMESPACE, input),
            Some(0) => Self::from_parts(DEFAULT_NAMESPACE, &input[1..]),
            Some(idx) => Self::from_parts(&input[..idx], &input[idx + 1..]),
        }
    }

    /// Construct an identifier directly from separate `namespace` and `path` parts.
    ///
    /// Note that `("", "foo")` succeeds with an empty namespace (matching the spec),
    /// while `("..", "foo")` fails namespace validation.
    pub fn from_parts(
        namespace: impl Into<String>,
        path: impl Into<String>,
    ) -> Result<Self, IdentifierError> {
        let namespace = namespace.into();
        let path = path.into();
        if !is_valid_namespace(&namespace) {
            return Err(IdentifierError::InvalidNamespace {
                raw: format!("{namespace}:{path}"),
            });
        }
        if !is_valid_path(&path) {
            return Err(IdentifierError::InvalidPath {
                raw: format!("{namespace}:{path}"),
            });
        }
        Ok(Self { namespace, path })
    }

    /// Construct an identifier in the default `minecraft` namespace.
    pub fn with_default_namespace(path: impl Into<String>) -> Result<Self, IdentifierError> {
        Self::from_parts(DEFAULT_NAMESPACE, path)
    }

    /// Returns the root meta-registry identifier (`minecraft:root`).
    pub fn root_registry() -> Self {
        Self {
            namespace: DEFAULT_NAMESPACE.to_owned(),
            path: "root".to_owned(),
        }
    }

    /// Parse an identifier from a command stream.
    ///
    /// Greedily consumes contiguous characters in `[0-9a-z_:./-]` and feeds the consumed
    /// slice into [`Identifier::parse`]. When `non_empty` is `true`, zero consumed characters
    /// fails with [`IdentifierError::EmptyCommandIdentifier`]; when `false`, zero consumed
    /// characters parses `""` as `minecraft:`.
    ///
    /// Returns `(identifier, bytes_consumed)`. On error, the caller should treat the cursor
    /// as rewound to 0.
    pub fn parse_command(input: &str, non_empty: bool) -> Result<(Self, usize), IdentifierError> {
        let consumed = input
            .char_indices()
            .take_while(|&(_, c)| is_valid_command_char(c))
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);

        if non_empty && consumed == 0 {
            return Err(IdentifierError::EmptyCommandIdentifier);
        }

        let id = Self::parse(&input[..consumed])?;
        Ok((id, consumed))
    }

    /// Returns the namespace portion.
    #[inline]
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Returns the path portion.
    #[inline]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Formats as `<path>` when `namespace == "minecraft"`, otherwise `<namespace>:<path>`.
    pub fn to_short_string(&self) -> String {
        if self.namespace == DEFAULT_NAMESPACE {
            self.path.clone()
        } else {
            self.to_string()
        }
    }

    /// Formats as `<namespace>.<path>`.
    pub fn to_language_key(&self) -> String {
        format!("{}.{}", self.namespace, self.path)
    }

    /// Formats as `<path>` when `namespace == "minecraft"`, otherwise `<namespace>.<path>`.
    pub fn to_short_language_key(&self) -> String {
        if self.namespace == DEFAULT_NAMESPACE {
            self.path.clone()
        } else {
            self.to_language_key()
        }
    }

    /// Formats as `<prefix>.<namespace>.<path>`.
    pub fn to_language_key_prefixed(&self, prefix: &str) -> String {
        format!("{prefix}.{}.{}", self.namespace, self.path)
    }

    /// Formats as `<prefix>.<namespace>.<path>.<suffix>`.
    pub fn to_language_key_prefixed_suffixed(&self, prefix: &str, suffix: &str) -> String {
        format!("{prefix}.{}.{}.{suffix}", self.namespace, self.path)
    }

    /// Formats the canonical string with every `/` and `:` replaced by `_`.
    pub fn to_debug_file_name(&self) -> String {
        self.to_string().replace(['/', ':'], "_")
    }

    /// Resolves this identifier under `root` as `root / namespace / path`, normalizing both
    /// paths and rejecting any target that escapes either `root` or `root / namespace`
    /// (for example via `..` segments or leading `/`).
    pub fn resolve_under(&self, root: &Path) -> Result<PathBuf, IdentifierError> {
        if self.path.starts_with('/') {
            return Err(IdentifierError::PathEscapesRoot {
                identifier: self.to_string(),
            });
        }

        let norm_root = normalize_lexical(root);
        let norm_ns_root = normalize_lexical(&norm_root.join(&self.namespace));
        if !norm_ns_root.starts_with(&norm_root) {
            return Err(IdentifierError::PathEscapesRoot {
                identifier: self.to_string(),
            });
        }

        let norm_target = normalize_lexical(&norm_ns_root.join(&self.path));
        if !norm_target.starts_with(&norm_ns_root) || !norm_target.starts_with(&norm_root) {
            return Err(IdentifierError::PathEscapesRoot {
                identifier: self.to_string(),
            });
        }

        Ok(norm_target)
    }

    /// Validates that this identifier fits within the network protocol UTF-8 / UTF-16 limits.
    pub fn validate_network_len(&self) -> Result<(), IdentifierError> {
        let canonical_bytes = self.namespace.len() + 1 + self.path.len();
        // Valid identifiers are pure ASCII, so UTF-16 unit count == byte count,
        // but check both limits explicitly per spec.
        if canonical_bytes > NETWORK_MAX_UTF8_BYTES || canonical_bytes > NETWORK_MAX_UTF16_UNITS {
            return Err(IdentifierError::NetworkEncoding {
                reason: format!(
                    "identifier length {canonical_bytes} exceeds limit of {NETWORK_MAX_UTF16_UNITS} UTF-16 code units"
                ),
            });
        }
        Ok(())
    }

    /// Encodes this identifier as a protocol VarInt-prefixed UTF-8 string.
    pub fn encode_network(&self, buf: &mut Vec<u8>) -> Result<(), IdentifierError> {
        self.validate_network_len()?;
        let s = self.to_string();
        write_varint(buf, s.len() as i32);
        buf.extend_from_slice(s.as_bytes());
        Ok(())
    }

    /// Decodes a protocol VarInt-prefixed UTF-8 string and parses it as an [`Identifier`].
    ///
    /// Enforces negative-length, byte-limit (`98,301`), truncation, and UTF-16 unit-limit
    /// (`32,767`) checks before identifier validation runs.
    pub fn decode_network(bytes: &[u8]) -> Result<(Self, usize), IdentifierError> {
        let (byte_len, header_len) =
            read_varint(bytes).map_err(|reason| IdentifierError::NetworkEncoding { reason })?;
        if byte_len < 0 {
            return Err(IdentifierError::NetworkEncoding {
                reason: format!("negative string length {byte_len}"),
            });
        }
        let byte_len = byte_len as usize;
        if byte_len > NETWORK_MAX_UTF8_BYTES {
            return Err(IdentifierError::NetworkEncoding {
                reason: format!(
                    "wire byte length {byte_len} exceeds maximum {NETWORK_MAX_UTF8_BYTES}"
                ),
            });
        }
        let end =
            header_len
                .checked_add(byte_len)
                .ok_or_else(|| IdentifierError::NetworkEncoding {
                    reason: "string length overflow".to_owned(),
                })?;
        if end > bytes.len() {
            return Err(IdentifierError::NetworkEncoding {
                reason: format!(
                    "truncated buffer: expected {byte_len} bytes, only {} available",
                    bytes.len() - header_len
                ),
            });
        }
        let text = std::str::from_utf8(&bytes[header_len..end]).map_err(|e| {
            IdentifierError::NetworkEncoding {
                reason: format!("invalid UTF-8: {e}"),
            }
        })?;
        let utf16_units = text.encode_utf16().count();
        if utf16_units > NETWORK_MAX_UTF16_UNITS {
            return Err(IdentifierError::NetworkEncoding {
                reason: format!(
                    "string length {utf16_units} UTF-16 code units exceeds maximum {NETWORK_MAX_UTF16_UNITS}"
                ),
            });
        }
        let id = Self::parse(text)?;
        Ok((id, end))
    }
}

impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.namespace, self.path)
    }
}

impl FromStr for Identifier {
    type Err = IdentifierError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl Ord for Identifier {
    fn cmp(&self, other: &Self) -> Ordering {
        self.path
            .cmp(&other.path)
            .then_with(|| self.namespace.cmp(&other.namespace))
    }
}

impl PartialOrd for Identifier {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Serialize for Identifier {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Identifier {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(|err| {
            serde::de::Error::custom(format!("Not a valid resource location: {err}"))
        })
    }
}

/// A typed resource key `(registry_id, element_id)` identifying one entry within a specific registry.
///
/// Registry keys themselves use `minecraft:root` as their `registry_id`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResourceKey {
    registry: Identifier,
    location: Identifier,
}

impl ResourceKey {
    /// Create a resource key for an entry `location` inside `registry`.
    pub fn new(registry: Identifier, location: Identifier) -> Self {
        Self { registry, location }
    }

    /// Create a registry key `(minecraft:root, registry_id)`.
    pub fn registry_key(registry_id: Identifier) -> Self {
        Self {
            registry: Identifier::root_registry(),
            location: registry_id,
        }
    }

    /// Create an entry key belonging to the registry identified by `registry_key`.
    pub fn for_registry(registry_key: &ResourceKey, location: Identifier) -> Self {
        Self {
            registry: registry_key.location.clone(),
            location,
        }
    }

    /// Returns the enclosing registry's identifier.
    #[inline]
    pub fn registry(&self) -> &Identifier {
        &self.registry
    }

    /// Returns this entry's identifier within its registry.
    #[inline]
    pub fn location(&self) -> &Identifier {
        &self.location
    }

    /// Returns `true` if this key belongs to the given registry key.
    pub fn is_for(&self, registry_key: &ResourceKey) -> bool {
        self.registry == registry_key.location
    }
}

impl fmt::Display for ResourceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ResourceKey[{} / {}]", self.registry, self.location)
    }
}

impl Ord for ResourceKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.registry
            .cmp(&other.registry)
            .then_with(|| self.location.cmp(&other.location))
    }
}

impl PartialOrd for ResourceKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A tag key `(registry_key, tag_id)` identifying a tag within a specific registry.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TagKey {
    registry: ResourceKey,
    location: Identifier,
}

impl TagKey {
    /// Create a tag key for `location` within `registry`.
    pub fn new(registry: ResourceKey, location: Identifier) -> Self {
        Self { registry, location }
    }

    /// Convenience constructor from a registry identifier and a tag identifier.
    pub fn for_registry_id(registry_id: Identifier, location: Identifier) -> Self {
        Self {
            registry: ResourceKey::registry_key(registry_id),
            location,
        }
    }

    /// Parse a bare tag key string (`"<tag_id>"`) for the given registry.
    pub fn parse_bare(registry: ResourceKey, input: &str) -> Result<Self, IdentifierError> {
        Ok(Self {
            registry,
            location: Identifier::parse(input)?,
        })
    }

    /// Parse a hashed tag key string (`"#<tag_id>"`) for the given registry.
    ///
    /// Rejects inputs that do not begin with `#` with [`IdentifierError::NotATagId`].
    pub fn parse_hashed(registry: ResourceKey, input: &str) -> Result<Self, IdentifierError> {
        let Some(rest) = input.strip_prefix('#') else {
            return Err(IdentifierError::NotATagId {
                raw: input.to_owned(),
            });
        };
        Ok(Self {
            registry,
            location: Identifier::parse(rest)?,
        })
    }

    /// Returns the enclosing registry's [`ResourceKey`].
    #[inline]
    pub fn registry(&self) -> &ResourceKey {
        &self.registry
    }

    /// Returns the tag's identifier.
    #[inline]
    pub fn location(&self) -> &Identifier {
        &self.location
    }

    /// Returns `true` if this tag key belongs to `registry`.
    pub fn is_for(&self, registry: &ResourceKey) -> bool {
        self.registry == *registry
    }

    /// Formats as `"#<tag_id>"` for hashed JSON fields.
    pub fn to_hashed_string(&self) -> String {
        format!("#{}", self.location)
    }
}

impl fmt::Display for TagKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TagKey[{} / {}]", self.registry.location, self.location)
    }
}

impl Ord for TagKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.registry
            .cmp(&other.registry)
            .then_with(|| self.location.cmp(&other.location))
    }
}

impl PartialOrd for TagKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Lexically normalizes a path without touching the filesystem.
fn normalize_lexical(path: &Path) -> PathBuf {
    let mut stack: Vec<Component<'_>> = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => match stack.last() {
                Some(Component::Normal(_)) => {
                    stack.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {
                    // `..` at root stays at root.
                }
                _ => stack.push(Component::ParentDir),
            },
            other => stack.push(other),
        }
    }
    stack.into_iter().collect()
}

/// Writes a standard protocol VarInt (up to 5 bytes).
pub(crate) fn write_varint(buf: &mut Vec<u8>, value: i32) {
    let mut v = value as u32;
    loop {
        if (v & !0x7F) == 0 {
            buf.push(v as u8);
            return;
        }
        buf.push(((v & 0x7F) | 0x80) as u8);
        v >>= 7;
    }
}

/// Reads a standard protocol VarInt (up to 5 bytes), returning `(value, bytes_read)`.
pub(crate) fn read_varint(bytes: &[u8]) -> Result<(i32, usize), String> {
    let mut value: u32 = 0;
    let mut shift: u32 = 0;
    for (idx, &byte) in bytes.iter().enumerate() {
        if idx >= 5 {
            return Err("VarInt exceeds 5 bytes".to_owned());
        }
        value |= ((byte & 0x7F) as u32) << shift;
        if (byte & 0x80) == 0 {
            return Ok((value as i32, idx + 1));
        }
        shift += 7;
    }
    Err("unexpected EOF while reading VarInt".to_owned())
}
