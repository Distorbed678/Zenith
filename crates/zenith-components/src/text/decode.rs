//! Text component decoding (spec A.2, A.3, A.6).

use zenith_codec::codec::DataResult;
use zenith_codec::json::{Json, ParseMode, parse};
use zenith_nbt::{Accounter, Tag};

use super::events::{decode_click_event, decode_hover_event};
use super::style::{Style, TextColor, shadow_color_from_floats};
use super::{
    Component, Content, NbtContent, NbtSource, ObjectContent, ObjectInfo, ScoreContent, ScoreName,
    SelectorContent, Translatable, TranslationArg, default_atlas,
};
use crate::dynval::{
    Dyn, Rec, read_bool, read_float, read_identifier, read_int, read_list, read_nbt_string,
    read_string,
};
use crate::grammar::{validate_block_position, validate_entity_selector};
use crate::nbt_path::NbtPath;
use crate::profile::decode_profile;
use crate::registry_context::RegistryContext;

/// Maximum length in UTF-16 code units for a login disconnect JSON string (spec A.6.3).
pub const LOGIN_DISCONNECT_MAX_CHARS: usize = 262_144;

/// Maximum length in UTF-16 code units of a written book page's compact JSON encoding (spec A.6.3).
pub const BOOK_PAGE_MAX_JSON_CHARS: usize = 32_767;

/// Content kind names in their fixed fallback-guess order (spec A.2.1).
// PARITY [oracle]: settled by the text-component suite (spec open question 4: kind guess order).
pub const CONTENT_KIND_ORDER: [&str; 7] = [
    "text",
    "translatable",
    "keybind",
    "score",
    "selector",
    "nbt",
    "object",
];

/// NBT data-source names in their fixed fallback-guess order (spec A.2.1).
// PARITY [oracle]: settled by the text-component suite (spec open question 4: source guess order).
pub const NBT_SOURCE_ORDER: [&str; 3] = ["entity", "block", "storage"];

/// Object info kind names in their fixed fallback-guess order (spec A.2.1).
// PARITY [oracle]: settled by the text-component suite (spec open question 4: object guess order).
pub const OBJECT_KIND_ORDER: [&str; 2] = ["atlas", "player"];

/// Decodes a text component from a format-neutral [`Dyn`] tree (JSON or NBT).
pub fn decode_component(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<Component> {
    match d {
        Dyn::Str(s) => DataResult::Success(Component::new(Content::Literal(s.clone()))),
        Dyn::List(items) => decode_component_list(items, registries),
        Dyn::Map(_) => decode_component_map(d, registries),
        other => DataResult::error(format!(
            "Not a valid text component: {} is not a string, list or map",
            other.kind()
        )),
    }
}

/// Decodes a text component from a [`Json`] value.
pub fn decode_component_json(
    json: &Json,
    registries: &dyn RegistryContext,
) -> DataResult<Component> {
    decode_component(&Dyn::from_json(json), registries)
}

/// Decodes a text component from strict JSON text.
pub fn decode_component_str_json(
    text: &str,
    registries: &dyn RegistryContext,
) -> DataResult<Component> {
    match parse(text, ParseMode::Data) {
        Ok(json) => decode_component_json(&json, registries),
        Err(e) => DataResult::error(format!("Invalid JSON: {e}")),
    }
}

/// Decodes a login disconnect reason JSON string, enforcing the 262,144-character limit (spec A.6.3).
pub fn decode_login_disconnect_json(
    text: &str,
    registries: &dyn RegistryContext,
) -> DataResult<Component> {
    if text.encode_utf16().count() > LOGIN_DISCONNECT_MAX_CHARS {
        return DataResult::error(format!(
            "Disconnect reason exceeds {LOGIN_DISCONNECT_MAX_CHARS} characters"
        ));
    }
    decode_component_str_json(text, registries)
}

/// Decodes a text component from an NBT [`Tag`].
pub fn decode_component_nbt(tag: &Tag, registries: &dyn RegistryContext) -> DataResult<Component> {
    decode_component(&Dyn::from_tag(tag), registries)
}

/// Decodes a written book page from [`Dyn`], rejecting pages whose compact JSON encoding exceeds
/// [`BOOK_PAGE_MAX_JSON_CHARS`] (32,767 UTF-16 code units; spec A.6.3, edge case 36).
pub fn decode_book_page(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<Component> {
    decode_component(d, registries).and_then(|comp| {
        match super::encode::encode_component_json_string(&comp) {
            DataResult::Success(json) => {
                if json.encode_utf16().count() > BOOK_PAGE_MAX_JSON_CHARS {
                    DataResult::error(format!(
                        "Book page exceeds {BOOK_PAGE_MAX_JSON_CHARS} JSON characters"
                    ))
                } else {
                    DataResult::Success(comp)
                }
            }
            DataResult::Error { message, .. } => DataResult::error(message),
            DataResult::Failure(m) => DataResult::Failure(m),
        }
    })
}

/// Decodes a text component from network NBT bytes (nameless root tag, default quota 2,097,152 bytes, depth 512).
pub fn decode_component_network_nbt(
    bytes: &[u8],
    registries: &dyn RegistryContext,
) -> Result<Component, String> {
    let mut acc = Accounter::default_quota();
    let tag = zenith_nbt::from_bytes_network(bytes, &mut acc)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Empty network NBT (TAG_End) is not a valid text component".to_owned())?;
    decode_component_nbt(&tag, registries)
        .demand()
        .map_err(|e| e.message)
}

fn decode_component_list(items: &[Dyn], registries: &dyn RegistryContext) -> DataResult<Component> {
    if items.is_empty() {
        return DataResult::error("Empty list is not a valid text component");
    }
    let mut good = Vec::with_capacity(items.len());
    let mut errors = Vec::new();
    for (i, item) in items.iter().enumerate() {
        match decode_component(item, registries) {
            DataResult::Success(c) => good.push(c),
            DataResult::Error { message, partial } => {
                errors.push(format!("Element {i}: {message}"));
                if let Some(p) = partial {
                    good.push(p);
                }
            }
            DataResult::Failure(m) => return DataResult::Failure(m),
        }
    }
    let merged = merge_component_list(good);
    if errors.is_empty() {
        match merged {
            Some(c) => DataResult::Success(c),
            None => DataResult::error("Empty list is not a valid text component"),
        }
    } else {
        DataResult::Error {
            message: errors.join("; "),
            partial: merged,
        }
    }
}

fn merge_component_list(mut comps: Vec<Component>) -> Option<Component> {
    if comps.is_empty() {
        return None;
    }
    let mut first = comps.remove(0);
    first.siblings.extend(comps);
    Some(first)
}

fn decode_component_map(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<Component> {
    let mut r = Rec::new(d);

    // 1. Content (explicit `type` or fixed fallback order).
    let content = match decode_content(d, registries) {
        DataResult::Success(c) => Some(c),
        DataResult::Error { message, partial } => {
            r.push_error(message);
            partial
        }
        DataResult::Failure(m) => return DataResult::Failure(m),
    };

    // 2. Siblings (`extra`): optional, but if present must be a non-empty list.
    let siblings = r.optional("extra", Vec::new(), |v| decode_extra_list(v, registries));

    // 3. Style fields (all 11 fields, canonicalized when all unset).
    let style = decode_style_fields(&mut r, registries);

    r.finish(|| {
        Some(Component {
            content: content?,
            siblings,
            style,
        })
    })
}

fn decode_extra_list(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<Vec<Component>> {
    let items = match read_list(d) {
        DataResult::Success(i) => i,
        other => return other.map(|_| Vec::new()),
    };
    if items.is_empty() {
        return DataResult::error("Expected non-empty extra list");
    }
    let mut good = Vec::with_capacity(items.len());
    let mut errors = Vec::new();
    for (i, item) in items.iter().enumerate() {
        match decode_component(item, registries) {
            DataResult::Success(c) => good.push(c),
            DataResult::Error { message, partial } => {
                errors.push(format!("Element {i}: {message}"));
                if let Some(p) = partial {
                    good.push(p);
                }
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

fn decode_style_fields(r: &mut Rec<'_>, registries: &dyn RegistryContext) -> Style {
    let color = r.optional_opt("color", |v| {
        read_string(v).and_then(|s| match TextColor::parse(&s) {
            Ok(c) => DataResult::Success(c),
            Err(m) => DataResult::error(m),
        })
    });
    let shadow_color = r.optional_opt("shadow_color", decode_shadow_color);
    let bold = r.optional_opt("bold", read_bool);
    let italic = r.optional_opt("italic", read_bool);
    let underlined = r.optional_opt("underlined", read_bool);
    let strikethrough = r.optional_opt("strikethrough", read_bool);
    let obfuscated = r.optional_opt("obfuscated", read_bool);
    let click_event = r.optional_opt("click_event", |v| decode_click_event(v, registries));
    let hover_event = r.optional_opt("hover_event", |v| decode_hover_event(v, registries));
    let insertion = r.optional_opt("insertion", read_string);
    let font = r.optional_opt("font", read_identifier);

    Style {
        color,
        shadow_color,
        bold,
        italic,
        underlined,
        strikethrough,
        obfuscated,
        click_event,
        hover_event,
        insertion,
        font,
    }
}

fn decode_shadow_color(d: &Dyn) -> DataResult<i32> {
    if matches!(d, Dyn::Num(_)) {
        return read_int(d);
    }
    if matches!(
        d,
        Dyn::List(_) | Dyn::ByteArray(_) | Dyn::IntArray(_) | Dyn::LongArray(_)
    ) {
        return read_list(d).and_then(|items| {
            if items.len() != 4 {
                return DataResult::error(format!(
                    "Expected 4 floats for shadow_color, got {}",
                    items.len()
                ));
            }
            let mut vals = [0.0f32; 4];
            for (i, item) in items.iter().enumerate() {
                match read_float(item) {
                    DataResult::Success(f) => vals[i] = f,
                    other => return other.map(|_| 0),
                }
            }
            DataResult::Success(shadow_color_from_floats(vals[0], vals[1], vals[2], vals[3]))
        });
    }
    DataResult::error(format!(
        "Expected integer or 4-float list for shadow_color, got {}",
        d.kind()
    ))
}

fn decode_content(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<Content> {
    if let Some(ty) = d.get("type") {
        let kind = match read_string(ty) {
            DataResult::Success(s) => s,
            other => return other.map(|_| unreachable!()),
        };
        return decode_content_kind(&kind, d, registries);
    }
    let mut errors = Vec::new();
    for kind in CONTENT_KIND_ORDER {
        match decode_content_kind(kind, d, registries) {
            DataResult::Success(c) => return DataResult::Success(c),
            DataResult::Failure(m) => return DataResult::Failure(m),
            DataResult::Error { message, .. } => errors.push(format!("{kind}: {message}")),
        }
    }
    DataResult::error(format!(
        "No matching codec for text component content ({})",
        errors.join("; ")
    ))
}

fn decode_content_kind(
    kind: &str,
    d: &Dyn,
    registries: &dyn RegistryContext,
) -> DataResult<Content> {
    match kind {
        "text" => decode_text(d),
        "translatable" => decode_translatable(d, registries),
        "keybind" => decode_keybind(d),
        "score" => decode_score(d),
        "selector" => decode_selector(d, registries),
        "nbt" => decode_nbt(d, registries),
        "object" => decode_object(d, registries),
        other => DataResult::error(format!("Unknown text component type: {other}")),
    }
}

fn decode_text(d: &Dyn) -> DataResult<Content> {
    let mut r = Rec::new(d);
    let text = r.required("text", read_nbt_string);
    r.finish(|| Some(Content::Literal(text?)))
}

fn decode_translatable(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<Content> {
    let mut r = Rec::new(d);
    let key = r.required("translate", read_string);
    let fallback = r.lenient("fallback", None, |v| read_string(v).map(Some));
    let args = r.optional("with", Vec::new(), |v| decode_with_args(v, registries));
    r.finish(|| {
        Some(Content::Translatable(Translatable {
            key: key?,
            fallback,
            args,
        }))
    })
}

fn decode_with_args(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<Vec<TranslationArg>> {
    let items = match read_list(d) {
        DataResult::Success(i) => i,
        other => return other.map(|_| Vec::new()),
    };
    let mut good = Vec::with_capacity(items.len());
    let mut errors = Vec::new();
    for (i, item) in items.iter().enumerate() {
        match decode_translation_arg(item, registries) {
            DataResult::Success(a) => good.push(a),
            DataResult::Error { message, partial } => {
                errors.push(format!("Element {i}: {message}"));
                if let Some(p) = partial {
                    good.push(p);
                }
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

fn decode_translation_arg(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<TranslationArg> {
    match d {
        Dyn::Bool(b) => DataResult::Success(TranslationArg::Bool(*b)),
        Dyn::Num(n) => DataResult::Success(TranslationArg::Num(n.narrowed())),
        Dyn::Str(s) => DataResult::Success(TranslationArg::Str(s.clone())),
        Dyn::Null => DataResult::error("Null is not a valid translation argument"),
        _ => decode_component(d, registries).map(|c| match c.as_plain_literal() {
            Some(s) => TranslationArg::Str(s.clone()),
            None => TranslationArg::Component(c),
        }),
    }
}

fn decode_keybind(d: &Dyn) -> DataResult<Content> {
    let mut r = Rec::new(d);
    let keybind = r.required("keybind", read_string);
    r.finish(|| Some(Content::Keybind(keybind?)))
}

fn decode_score(d: &Dyn) -> DataResult<Content> {
    let mut r = Rec::new(d);
    let score = r.required("score", |v| {
        let mut sr = Rec::new(v);
        let name = sr.required("name", read_string);
        let objective = sr.required("objective", read_string);
        sr.finish(|| {
            let raw_name = name?;
            let parsed_name = if validate_entity_selector(&raw_name).is_ok() {
                ScoreName::Selector(raw_name)
            } else {
                ScoreName::Plain(raw_name)
            };
            Some(ScoreContent {
                name: parsed_name,
                objective: objective?,
            })
        })
    });
    r.finish(|| Some(Content::Score(score?)))
}

fn decode_selector(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<Content> {
    let mut r = Rec::new(d);
    let pattern = r.required("selector", |v| {
        read_string(v).and_then(|s| match validate_entity_selector(&s) {
            Ok(()) => DataResult::Success(s),
            Err(m) => DataResult::error(format!("Invalid selector: {m}")),
        })
    });
    let separator = r.optional_opt("separator", |v| decode_component(v, registries));
    r.finish(|| {
        Some(Content::Selector(SelectorContent {
            pattern: pattern?,
            separator: separator.map(Box::new),
        }))
    })
}

fn decode_nbt(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<Content> {
    let mut r = Rec::new(d);
    let path = r.required("nbt", |v| {
        read_string(v).and_then(|s| match NbtPath::parse(&s) {
            Ok(_) => DataResult::Success(s),
            Err(m) => DataResult::error(m),
        })
    });
    let interpret = r.lenient("interpret", false, read_bool);
    let plain = r.lenient("plain", false, read_bool);
    let separator = r.lenient("separator", None, |v| {
        decode_component(v, registries).map(|c| Some(Box::new(c)))
    });
    if interpret && plain {
        r.push_error("Cannot have both interpret and plain set to true");
    }
    let source = match decode_nbt_source(d) {
        DataResult::Success(s) => Some(s),
        DataResult::Error { message, .. } => {
            r.push_error(message);
            None
        }
        DataResult::Failure(m) => return DataResult::Failure(m),
    };
    r.finish(|| {
        Some(Content::Nbt(NbtContent {
            path: path?,
            interpret,
            plain,
            separator,
            source: source?,
        }))
    })
}

fn decode_nbt_source(d: &Dyn) -> DataResult<NbtSource> {
    if let Some(src) = d.get("source") {
        let kind = match read_string(src) {
            DataResult::Success(s) => s,
            other => return other.map(|_| unreachable!()),
        };
        return decode_nbt_source_kind(&kind, d);
    }
    let mut errors = Vec::new();
    for kind in NBT_SOURCE_ORDER {
        match decode_nbt_source_kind(kind, d) {
            DataResult::Success(s) => return DataResult::Success(s),
            DataResult::Failure(m) => return DataResult::Failure(m),
            DataResult::Error { message, .. } => errors.push(format!("{kind}: {message}")),
        }
    }
    DataResult::error(format!(
        "No matching codec for NBT data source ({})",
        errors.join("; ")
    ))
}

fn decode_nbt_source_kind(kind: &str, d: &Dyn) -> DataResult<NbtSource> {
    match kind {
        "entity" => {
            let mut r = Rec::new(d);
            let e = r.required("entity", |v| {
                read_string(v).and_then(|s| match validate_entity_selector(&s) {
                    Ok(()) => DataResult::Success(s),
                    Err(m) => DataResult::error(format!("Invalid selector: {m}")),
                })
            });
            r.finish(|| Some(NbtSource::Entity(e?)))
        }
        "block" => {
            let mut r = Rec::new(d);
            let b = r.required("block", |v| {
                read_string(v).and_then(|s| match validate_block_position(&s) {
                    Ok(()) => DataResult::Success(s),
                    Err(m) => DataResult::error(m),
                })
            });
            r.finish(|| Some(NbtSource::Block(b?)))
        }
        "storage" => {
            let mut r = Rec::new(d);
            let s = r.required("storage", read_identifier);
            r.finish(|| Some(NbtSource::Storage(s?)))
        }
        other => DataResult::error(format!("Unknown NBT data source: {other}")),
    }
}

fn decode_object(d: &Dyn, registries: &dyn RegistryContext) -> DataResult<Content> {
    let mut r = Rec::new(d);
    let info = match decode_object_info(d) {
        DataResult::Success(info) => Some(info),
        DataResult::Error { message, .. } => {
            r.push_error(message);
            None
        }
        DataResult::Failure(m) => return DataResult::Failure(m),
    };
    let fallback = r.optional_opt("fallback", |v| decode_component(v, registries));
    r.finish(|| {
        Some(Content::Object(ObjectContent {
            info: info?,
            fallback: fallback.map(Box::new),
        }))
    })
}

fn decode_object_info(d: &Dyn) -> DataResult<ObjectInfo> {
    if let Some(obj) = d.get("object") {
        let kind = match read_string(obj) {
            DataResult::Success(s) => s,
            other => return other.map(|_| unreachable!()),
        };
        return decode_object_info_kind(&kind, d);
    }
    let mut errors = Vec::new();
    for kind in OBJECT_KIND_ORDER {
        match decode_object_info_kind(kind, d) {
            DataResult::Success(o) => return DataResult::Success(o),
            DataResult::Failure(m) => return DataResult::Failure(m),
            DataResult::Error { message, .. } => errors.push(format!("{kind}: {message}")),
        }
    }
    DataResult::error(format!(
        "No matching codec for object info ({})",
        errors.join("; ")
    ))
}

fn decode_object_info_kind(kind: &str, d: &Dyn) -> DataResult<ObjectInfo> {
    match kind {
        "atlas" => {
            let mut r = Rec::new(d);
            let sprite = r.required("sprite", read_identifier);
            let atlas = r.optional("atlas", default_atlas(), read_identifier);
            r.finish(|| {
                Some(ObjectInfo::Atlas {
                    sprite: sprite?,
                    atlas,
                })
            })
        }
        "player" => {
            let mut r = Rec::new(d);
            let profile = r.required("player", decode_profile);
            let hat = r.optional("hat", true, read_bool);
            r.finish(|| {
                Some(ObjectInfo::Player {
                    profile: profile?,
                    hat,
                })
            })
        }
        other => DataResult::error(format!("Unknown object type: {other}")),
    }
}
