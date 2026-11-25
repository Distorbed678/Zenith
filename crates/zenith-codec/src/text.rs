//! Validation of text components for `pack.mcmeta` descriptions (spec §3.3).
//!
//! Accepted: a JSON string, a **non-empty** array of components, or an object with a content key. Numbers, booleans,
//! `{}`, `[]` and `null` are rejected, as is an object with an invalid style value (such as an unknown `color`).
//! The component is kept as the validated JSON; rendering belongs to the client.

use zenith_registry::Identifier;

use crate::codec::{Codec, DataResult};
use crate::json::Json;

/// A validated text component (the JSON it was read from).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component(pub Json);

impl Component {
    /// A plain literal string component.
    pub fn literal(text: impl Into<String>) -> Self {
        Self(Json::string(text.into()))
    }

    /// A translatable component with no arguments.
    pub fn translatable(key: impl Into<String>) -> Self {
        let mut obj = crate::json::JsonObject::new();
        obj.insert("translate", Json::string(key.into()));
        Self(Json::Object(obj))
    }
}

/// The codec for [`Component`].
#[derive(Debug, Clone, Copy, Default)]
pub struct ComponentCodec;

const NAMED_COLORS: [&str; 16] = [
    "black",
    "dark_blue",
    "dark_green",
    "dark_aqua",
    "dark_red",
    "dark_purple",
    "gold",
    "gray",
    "dark_gray",
    "blue",
    "green",
    "aqua",
    "red",
    "light_purple",
    "yellow",
    "white",
];

impl Codec<Component> for ComponentCodec {
    fn decode(&self, input: &Json) -> DataResult<Component> {
        match validate(input, 0) {
            Ok(()) => DataResult::Success(Component(input.clone())),
            Err(m) => DataResult::error(m),
        }
    }

    fn encode(&self, value: &Component) -> DataResult<Json> {
        DataResult::Success(value.0.clone())
    }
}

fn validate(input: &Json, depth: usize) -> Result<(), String> {
    if depth > 64 {
        return Err("Text component is nested too deeply".into());
    }
    match input {
        Json::String(_) => Ok(()),
        Json::Array(items) => {
            if items.is_empty() {
                return Err("Empty array is not a valid text component".into());
            }
            items.iter().try_for_each(|i| validate(i, depth + 1))
        }
        Json::Object(obj) => validate_object(obj, depth),
        other => Err(format!(
            "Not a valid text component: {} is not a string, array or object",
            other.type_name()
        )),
    }
}

fn expect_string(obj: &crate::json::JsonObject, key: &str) -> Result<(), String> {
    match obj.get(key) {
        Some(Json::String(_)) | None => Ok(()),
        Some(other) => Err(format!(
            "`{key}` must be a string, got {}",
            other.type_name()
        )),
    }
}

fn expect_bool(obj: &crate::json::JsonObject, key: &str) -> Result<(), String> {
    match obj.get(key) {
        Some(Json::Bool(_)) | None | Some(Json::Null) => Ok(()),
        Some(other) => Err(format!(
            "`{key}` must be a boolean, got {}",
            other.type_name()
        )),
    }
}

fn validate_object(obj: &crate::json::JsonObject, depth: usize) -> Result<(), String> {
    // Content.
    let explicit = match obj.get("type") {
        Some(Json::String(t)) => Some(t.to_string_lossy()),
        Some(Json::Null) | None => None,
        Some(other) => {
            return Err(format!(
                "`type` must be a string, got {}",
                other.type_name()
            ));
        }
    };
    let kind = match explicit.as_deref() {
        Some("text") => "text",
        Some("translatable") => "translate",
        Some("score") => "score",
        Some("selector") => "selector",
        Some("keybind") => "keybind",
        Some("nbt") => "nbt",
        Some("object") => "object",
        Some(other) => return Err(format!("Unknown text component type: {other}")),
        None => [
            "text",
            "translate",
            "score",
            "selector",
            "keybind",
            "nbt",
            "object",
        ]
        .into_iter()
        .find(|k| obj.get(k).is_some_and(|v| !v.is_null()))
        .ok_or_else(|| "Don't know how to turn {...} into a Component".to_owned())?,
    };
    match kind {
        "text" => match obj.get("text") {
            Some(Json::String(_)) => {}
            Some(Json::Number(_) | Json::Bool(_)) => {} // primitives are rendered as text
            _ => return Err("`text` content is missing or not a string".into()),
        },
        "translate" => {
            match obj.get("translate") {
                Some(Json::String(_)) => {}
                _ => return Err("`translate` must be a string".into()),
            }
            expect_string(obj, "fallback")?;
            match obj.get("with") {
                None | Some(Json::Null) => {}
                Some(Json::Array(args)) => {
                    for a in args {
                        match a {
                            Json::Number(_) | Json::Bool(_) => {}
                            other => validate(other, depth + 1)?,
                        }
                    }
                }
                Some(other) => {
                    return Err(format!(
                        "`with` must be an array, got {}",
                        other.type_name()
                    ));
                }
            }
        }
        "score" => {
            let Some(Json::Object(score)) = obj.get("score") else {
                return Err("`score` must be an object".into());
            };
            for key in ["name", "objective"] {
                if !matches!(score.get(key), Some(Json::String(_))) {
                    return Err(format!("`score` needs a string `{key}`"));
                }
            }
        }
        "selector" => {
            if !matches!(obj.get("selector"), Some(Json::String(_))) {
                return Err("`selector` must be a string".into());
            }
        }
        "keybind" => {
            if !matches!(obj.get("keybind"), Some(Json::String(_))) {
                return Err("`keybind` must be a string".into());
            }
        }
        "nbt" => {
            if !matches!(obj.get("nbt"), Some(Json::String(_))) {
                return Err("`nbt` must be a string".into());
            }
            expect_bool(obj, "interpret")?;
        }
        _ => {}
    }

    // Children.
    match obj.get("extra") {
        None | Some(Json::Null) => {}
        Some(Json::Array(items)) => {
            for i in items {
                validate(i, depth + 1)?;
            }
        }
        Some(other) => {
            return Err(format!(
                "`extra` must be an array, got {}",
                other.type_name()
            ));
        }
    }

    // Style.
    match obj.get("color") {
        None | Some(Json::Null) => {}
        Some(Json::String(c)) => {
            let valid = c.as_str().is_some_and(|s| {
                let hex = s
                    .strip_prefix('#')
                    .is_some_and(|h| h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit()));
                hex || NAMED_COLORS.contains(&s)
            });
            if !valid {
                return Err(format!("Invalid color: {c}"));
            }
        }
        Some(other) => {
            return Err(format!(
                "`color` must be a string, got {}",
                other.type_name()
            ));
        }
    }
    for key in [
        "bold",
        "italic",
        "underlined",
        "strikethrough",
        "obfuscated",
    ] {
        expect_bool(obj, key)?;
    }
    if let Some(Json::String(f)) = obj.get("font") {
        if f.as_str().is_none_or(|s| Identifier::parse(s).is_err()) {
            return Err(format!("Invalid font identifier: {f}"));
        }
    } else if let Some(other) = obj.get("font").filter(|v| !v.is_null()) {
        return Err(format!(
            "`font` must be a string, got {}",
            other.type_name()
        ));
    }
    expect_string(obj, "insertion")?;
    Ok(())
}
