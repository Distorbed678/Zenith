//! Small command-grammar validators the text components depend on.
//!
//! The full command grammars (entity selectors with all their arguments, block positions) belong to the command
//! specs; components only need a "does this whole string parse" answer. Each validator here is structural and is
//! isolated behind one function so the command milestone can replace it.

/// Characters the command string reader accepts in an unquoted string.
fn is_unquoted(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+')
}

/// Whether `s` is a hyphenated UUID the platform parser accepts (see [`parse_uuid_lenient`]).
fn is_uuid(s: &str) -> bool {
    parse_uuid_lenient(s).is_some()
}

/// Lenient UUID string parsing: exactly five dash-separated hexadecimal groups, at most 36 characters, each group
/// parsed as a (possibly short) hexadecimal number, then packed as the platform does.
// PARITY [oracle]: settled by the text-component suite (spec open question 7: short groups such as `1-1-1-1-1`).
pub fn parse_uuid_lenient(s: &str) -> Option<[i32; 4]> {
    if s.len() > 36 {
        return None;
    }
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 5 {
        return None;
    }
    let mut v = [0i64; 5];
    for (i, p) in parts.iter().enumerate() {
        if p.is_empty() || !p.chars().all(|c| c.is_ascii_hexdigit()) || p.len() > 16 {
            return None;
        }
        v[i] = u64::from_str_radix(p, 16).ok()? as i64;
    }
    let most = ((v[0] & 0xFFFF_FFFF) << 32) | ((v[1] & 0xFFFF) << 16) | (v[2] & 0xFFFF);
    let least = ((v[3] & 0xFFFF) << 48) | (v[4] & 0xFFFF_FFFF_FFFF);
    Some([
        (most >> 32) as i32,
        most as i32,
        (least >> 32) as i32,
        least as i32,
    ])
}

/// Formats four ints as a lower-case hyphenated UUID.
pub fn format_uuid(u: [i32; 4]) -> String {
    let most = (u64::from(u[0] as u32) << 32) | u64::from(u[1] as u32);
    let least = (u64::from(u[2] as u32) << 32) | u64::from(u[3] as u32);
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        most >> 32,
        (most >> 16) & 0xFFFF,
        most & 0xFFFF,
        least >> 48,
        least & 0xFFFF_FFFF_FFFF
    )
}

const SELECTOR_KEYS: &[&str] = &[
    "name",
    "distance",
    "level",
    "x",
    "y",
    "z",
    "dx",
    "dy",
    "dz",
    "x_rotation",
    "y_rotation",
    "scores",
    "tag",
    "team",
    "limit",
    "sort",
    "gamemode",
    "advancements",
    "nbt",
    "type",
    "predicate",
];

/// Validates a complete entity-selector string: `@p`/`@a`/`@r`/`@s`/`@e`/`@n` with optional `[key=value,…]`, or a
/// player name (≤ 16 unquoted characters) or a UUID. The whole string must be consumed.
// PARITY [oracle]: settled by the text-component suite (structural check; full argument grammar is the command spec's).
pub fn validate_entity_selector(s: &str) -> Result<(), String> {
    let mut chars = s.char_indices().peekable();
    if s.starts_with('@') {
        chars.next();
        match chars.next() {
            Some((_, 'p' | 'a' | 'r' | 's' | 'e' | 'n')) => {}
            _ => return Err(format!("Unknown selector type in '{s}'")),
        }
        let rest = &s[2..];
        if rest.is_empty() {
            return Ok(());
        }
        let Some(inner) = rest.strip_prefix('[') else {
            return Err(format!("Expected end of selector in '{s}'"));
        };
        let Some(inner) = inner.strip_suffix(']') else {
            return Err(format!("Expected end of options in '{s}'"));
        };
        return validate_selector_arguments(inner);
    }
    let name_len = s.chars().take_while(|c| is_unquoted(*c)).count();
    if name_len != s.chars().count() || name_len == 0 {
        return Err(format!("Invalid name or UUID: {s}"));
    }
    if s.chars().count() > 16 && !is_uuid(s) {
        return Err(format!("Invalid name or UUID: {s}"));
    }
    Ok(())
}

fn validate_selector_arguments(inner: &str) -> Result<(), String> {
    // Split on top-level commas, honouring quotes and brace/bracket nesting.
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut start = 0;
    for (i, c) in inner.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '{' | '[' => depth += 1,
            '}' | ']' => {
                depth -= 1;
                if depth < 0 {
                    return Err("Unbalanced selector arguments".to_owned());
                }
            }
            ',' if depth == 0 => {
                parts.push(&inner[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if depth != 0 || quote.is_some() {
        return Err("Unbalanced selector arguments".to_owned());
    }
    parts.push(&inner[start..]);
    if inner.trim().is_empty() {
        return Ok(());
    }
    for part in parts {
        let part = part.trim_start();
        let Some(eq) = part.find('=') else {
            return Err(format!("Expected '=' in selector argument '{part}'"));
        };
        let key = part[..eq].trim_end();
        if !SELECTOR_KEYS.contains(&key) {
            return Err(format!("Unknown option '{key}'"));
        }
        if part[eq + 1..].trim().is_empty() {
            return Err(format!("Missing value for option '{key}'"));
        }
    }
    Ok(())
}

/// Validates a block-position argument string: three coordinates separated by single spaces; each a number, a `~`
/// relative offset or a `^` local offset; local and world/relative coordinates cannot be mixed.
// PARITY [oracle]: settled by the text-component suite.
pub fn validate_block_position(s: &str) -> Result<(), String> {
    let parts: Vec<&str> = s.split(' ').collect();
    if parts.len() != 3 {
        return Err("Incomplete (expected 3 coordinates)".to_owned());
    }
    let local = parts[0].starts_with('^');
    for p in &parts {
        if p.is_empty() {
            return Err("Expected a coordinate".to_owned());
        }
        let is_local = p.starts_with('^');
        if is_local != local {
            return Err("Cannot mix world & local coordinates".to_owned());
        }
        let body = if is_local || p.starts_with('~') {
            &p[1..]
        } else {
            p
        };
        if body.is_empty() && (is_local || p.starts_with('~')) {
            continue;
        }
        if !is_number(body) {
            return Err(format!("Invalid coordinate '{p}'"));
        }
    }
    Ok(())
}

fn is_number(s: &str) -> bool {
    let b = s.strip_prefix(['-', '+']).unwrap_or(s);
    !b.is_empty()
        && b.chars().all(|c| c.is_ascii_digit() || c == '.')
        && b.chars().filter(|c| *c == '.').count() <= 1
        && b.chars().any(|c| c.is_ascii_digit())
}

/// Parses a URI the way the platform's strict URI parser would, for the purpose of `open_url`: returns the scheme.
///
/// A scheme (`ALPHA *(ALPHA / DIGIT / "+" / "-" / ".")`) followed by `:` is required; the rest may not contain
/// spaces, control characters, `"<>\^`{|}` or a `%` not followed by two hexadecimal digits.
// PARITY [oracle]: settled by the text-component suite.
pub fn parse_uri_scheme(s: &str) -> Option<String> {
    let colon = s.find(':')?;
    let scheme = &s[..colon];
    let mut it = scheme.chars();
    let first = it.next()?;
    if !first.is_ascii_alphabetic()
        || !it.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return None;
    }
    let rest = &s[colon + 1..];
    if rest.is_empty() {
        return None;
    }
    let bytes: Vec<char> = rest.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == '%' {
            let h1 = bytes.get(i + 1).is_some_and(char::is_ascii_hexdigit);
            let h2 = bytes.get(i + 2).is_some_and(char::is_ascii_hexdigit);
            if !(h1 && h2) {
                return None;
            }
            i += 3;
            continue;
        }
        if c <= ' '
            || c == '\u{7f}'
            || matches!(c, '"' | '<' | '>' | '\\' | '^' | '`' | '{' | '|' | '}')
        {
            return None;
        }
        i += 1;
    }
    Some(scheme.to_owned())
}
