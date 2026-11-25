//! Translation lookup, lang-file `%d`/`%f` rewriting, and placeholder formatting (spec A.7).

use std::collections::HashMap;

use zenith_nbt::NbtString;

use super::{Component, Translatable, TranslationArg};

/// Provides translated template strings (after lang-file load-time rewriting).
pub trait LanguageProvider {
    /// Looks up a translation key; returns `None` when the key is missing.
    fn get(&self, key: &str) -> Option<&str>;
}

/// An empty language table (every key is missing, falling back to `fallback` or the key itself).
#[derive(Debug, Clone, Copy, Default)]
pub struct EmptyLanguage;

impl LanguageProvider for EmptyLanguage {
    fn get(&self, _key: &str) -> Option<&str> {
        None
    }
}

/// An in-memory language table. Values inserted with [`LanguageTable::insert_lang_entry`] undergo the
/// spec A.7.1 load-time `%d`/`%f` rewrite automatically.
#[derive(Debug, Clone, Default)]
pub struct LanguageTable {
    entries: HashMap<String, String>,
    fallback: Option<Box<LanguageTable>>,
}

impl LanguageTable {
    /// Creates an empty language table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Layers this table over a base fallback table (e.g. selected language layered over `en_us`).
    pub fn with_fallback(mut self, base: LanguageTable) -> Self {
        self.fallback = Some(Box::new(base));
        self
    }

    /// Inserts a raw lang-file entry, applying the spec A.7.1 load-time `%d`/`%f` rewrite.
    pub fn insert_lang_entry(&mut self, key: impl Into<String>, raw_value: &str) {
        self.entries
            .insert(key.into(), rewrite_lang_value(raw_value));
    }

    /// Inserts an already-rewritten template directly.
    pub fn insert_exact(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.entries.insert(key.into(), value.into());
    }

    /// Loads entries from an iterator of `(key, raw_value)` pairs (such as a parsed lang JSON map).
    pub fn from_lang_entries<'a>(iter: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut t = Self::new();
        for (k, v) in iter {
            t.insert_lang_entry(k, v);
        }
        t
    }
}

impl LanguageProvider for LanguageTable {
    fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .get(key)
            .map(String::as_str)
            .or_else(|| self.fallback.as_deref().and_then(|fb| fb.get(key)))
    }
}

/// Applies the spec A.7.1 rule 2 load-time lang-file rewrite:
/// each occurrence of `%`, an optional `N$`, any run of digits and dots, then `d` or `f`
/// is rewritten to `%s` (or `%N$s`).
///
/// This rewrite does NOT apply to `fallback` strings or to a key used as its own template.
pub fn rewrite_lang_value(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some((end, pos_digits)) = match_df_placeholder(bytes, i)
        {
            out.push('%');
            if let Some((ps, pe)) = pos_digits {
                out.push_str(&raw[ps..pe]);
                out.push('$');
            }
            out.push('s');
            i = end;
            continue;
        }
        let ch = raw[i..].chars().next().expect("valid utf-8 boundary");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Matches `%(\d+\$)?[\d.]*[df]` starting at `bytes[start] == b'%'`.
/// Returns `(end_byte_index, optional (digits_start, digits_end))`.
fn match_df_placeholder(bytes: &[u8], start: usize) -> Option<(usize, Option<(usize, usize)>)> {
    let mut j = start + 1;
    let mut pos = None;

    // Optional `\d+\$`
    let d_start = j;
    while j < bytes.len() && bytes[j].is_ascii_digit() {
        j += 1;
    }
    if j > d_start && j < bytes.len() && bytes[j] == b'$' {
        pos = Some((d_start, j));
        j += 1;
    } else {
        // Backtrack: digits belong to `[\d.]*` instead.
        j = start + 1;
    }

    // `[\d.]*`
    while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b'.') {
        j += 1;
    }

    if j < bytes.len() && matches!(bytes[j], b'd' | b'f') {
        Some((j + 1, pos))
    } else {
        None
    }
}

/// One piece produced when decomposing a [`Translatable`] template.
#[derive(Debug, Clone, PartialEq)]
pub enum DecomposedPart<'a> {
    /// Literal text (either verbatim template text or formatted primitive argument).
    Text(NbtString),
    /// A component argument to be rendered with the translatable's effective style as parent.
    Component(&'a Component),
}

/// Resolves the template string for `t` against `lang` (spec A.7.1 rule 1):
/// language entry → `fallback` → `key`.
pub fn lookup_template<'a>(t: &'a Translatable, lang: &'a dyn LanguageProvider) -> &'a str {
    if let Some(v) = lang.get(&t.key) {
        v
    } else if let Some(fb) = &t.fallback {
        fb.as_str()
    } else {
        t.key.as_str()
    }
}

/// Decomposes a [`Translatable`] against `lang` using the ordered rules of spec A.7.2 & A.7.3.
///
/// If the template is malformed, returns a single [`DecomposedPart::Text`] containing the raw
/// template text with no argument substitution at all.
pub fn decompose_translatable<'a>(
    t: &'a Translatable,
    lang: &dyn LanguageProvider,
) -> Vec<DecomposedPart<'a>> {
    let template = lookup_template(t, lang);
    match parse_and_substitute(template, &t.args) {
        Some(parts) => parts,
        None => vec![DecomposedPart::Text(NbtString::from(template))],
    }
}

/// Parses `template` according to spec A.7.2 and substitutes `args`.
/// Returns `None` if the template is malformed.
fn parse_and_substitute<'a>(
    template: &str,
    args: &'a [TranslationArg],
) -> Option<Vec<DecomposedPart<'a>>> {
    let bytes = template.as_bytes();
    let mut parts = Vec::new();
    let mut cursor = 0usize;
    let mut next_seq = 0usize;

    while cursor < bytes.len() {
        let Some(rel_pct) = template[cursor..].find('%') else {
            // Remaining tail contains no `%`.
            if cursor < bytes.len() {
                parts.push(DecomposedPart::Text(NbtString::from(&template[cursor..])));
            }
            return Some(parts);
        };
        let pct = cursor + rel_pct;
        if pct > cursor {
            parts.push(DecomposedPart::Text(NbtString::from(
                &template[cursor..pct],
            )));
        }

        // Parse placeholder starting at `pct` (spec A.7.2):
        // `%`, optional `(\d+\$)`, then one ASCII letter, `%`, or end-of-string.
        let mut j = pct + 1;
        if j >= bytes.len() {
            // `%` at the very end -> malformed.
            return None;
        }

        // Optional `N$`
        let mut positional: Option<&str> = None;
        let d_start = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j > d_start && j < bytes.len() && bytes[j] == b'$' {
            positional = Some(&template[d_start..j]);
            j += 1;
            if j >= bytes.len() {
                return None;
            }
        } else if j > d_start {
            // Digits immediately after `%` without `$` (e.g. `%1s` or `%02d`) -> malformed!
            return None;
        }

        let spec = bytes[j];
        cursor = j + 1;

        match spec {
            b'%' => {
                // `%%` emits one `%`; `%N$%` is malformed.
                if positional.is_some() {
                    return None;
                }
                parts.push(DecomposedPart::Text(NbtString::from("%")));
            }
            b's' => {
                let idx = if let Some(digits) = positional {
                    let n: i32 = digits.parse().ok()?;
                    if n <= 0 {
                        return None;
                    }
                    (n - 1) as usize
                } else {
                    let i = next_seq;
                    next_seq += 1;
                    i
                };
                let arg = args.get(idx)?;
                push_arg(&mut parts, arg);
            }
            // Any other ASCII letter (`%d`, `%S`, `%x`, …) or non-letter (`% `, …) -> malformed.
            _ => return None,
        }
    }

    Some(parts)
}

fn push_arg<'a>(parts: &mut Vec<DecomposedPart<'a>>, arg: &'a TranslationArg) {
    match arg {
        TranslationArg::Null => parts.push(DecomposedPart::Text(NbtString::from("null"))),
        TranslationArg::Bool(b) => parts.push(DecomposedPart::Text(NbtString::from(if *b {
            "true"
        } else {
            "false"
        }))),
        TranslationArg::Num(n) => parts.push(DecomposedPart::Text(NbtString::from(n.to_display()))),
        TranslationArg::Str(s) => parts.push(DecomposedPart::Text(s.clone())),
        TranslationArg::Component(c) => parts.push(DecomposedPart::Component(c)),
    }
}
