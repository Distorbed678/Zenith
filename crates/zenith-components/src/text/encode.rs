//! Text component encoding (spec A.6.2).

use zenith_codec::codec::DataResult;
use zenith_codec::json::Json;
use zenith_nbt::Tag;

use super::events::{encode_click_event, encode_hover_event};
use super::{Component, Content, NbtSource, ObjectInfo, TranslationArg, default_atlas};
use crate::dynval::{Dyn, MapBuilder, Num};
use crate::profile::encode_profile;

/// Encodes a [`Component`] to a format-neutral [`Dyn`] tree.
///
/// Follows spec A.6.2:
/// - Plain literal with empty style and no siblings collapses to a bare string.
/// - Otherwise encodes as a map: content fields (never writing `type`), `extra` if non-empty,
///   then each set style field.
// PARITY [oracle]: settled by the text-component suite (spec open question 3: JSON key order).
pub fn encode_component(c: &Component) -> DataResult<Dyn> {
    if let Some(s) = c.as_plain_literal() {
        return DataResult::Success(Dyn::Str(s.clone()));
    }

    let mut b = MapBuilder::new();
    match &c.content {
        Content::Literal(s) => {
            b.put("text", Dyn::Str(s.clone()));
        }
        Content::Translatable(t) => {
            b.put("translate", Dyn::string(&t.key));
            if let Some(fb) = &t.fallback {
                b.put("fallback", Dyn::string(fb));
            }
            if !t.args.is_empty() {
                let mut encoded_args = Vec::with_capacity(t.args.len());
                for arg in &t.args {
                    match encode_translation_arg(arg) {
                        DataResult::Success(v) => encoded_args.push(v),
                        other => return other,
                    }
                }
                b.put("with", Dyn::List(encoded_args));
            }
        }
        Content::Keybind(k) => {
            b.put("keybind", Dyn::string(k));
        }
        Content::Score(s) => {
            let mut sm = MapBuilder::new();
            sm.put("name", Dyn::string(s.name.as_str()));
            sm.put("objective", Dyn::string(&s.objective));
            b.put("score", sm.build());
        }
        Content::Selector(s) => {
            b.put("selector", Dyn::string(&s.pattern));
            if let Some(sep) = &s.separator {
                match encode_component(sep) {
                    DataResult::Success(v) => {
                        b.put("separator", v);
                    }
                    other => return other,
                }
            }
        }
        Content::Nbt(n) => {
            b.put("nbt", Dyn::string(&n.path));
            if n.interpret {
                b.put("interpret", Dyn::Bool(true));
            }
            if n.plain {
                b.put("plain", Dyn::Bool(true));
            }
            if let Some(sep) = &n.separator {
                match encode_component(sep) {
                    DataResult::Success(v) => {
                        b.put("separator", v);
                    }
                    other => return other,
                }
            }
            match &n.source {
                NbtSource::Entity(e) => {
                    b.put("entity", Dyn::string(e));
                }
                NbtSource::Block(pos) => {
                    b.put("block", Dyn::string(pos));
                }
                NbtSource::Storage(id) => {
                    b.put("storage", Dyn::string(id.to_string()));
                }
            }
        }
        Content::Object(o) => {
            match &o.info {
                ObjectInfo::Atlas { sprite, atlas } => {
                    b.put("sprite", Dyn::string(sprite.to_string()));
                    if *atlas != default_atlas() {
                        b.put("atlas", Dyn::string(atlas.to_string()));
                    }
                }
                ObjectInfo::Player { profile, hat } => {
                    match encode_profile(profile) {
                        DataResult::Success(v) => {
                            b.put("player", v);
                        }
                        other => return other,
                    }
                    if !*hat {
                        b.put("hat", Dyn::Bool(false));
                    }
                }
            }
            if let Some(fb) = &o.fallback {
                match encode_component(fb) {
                    DataResult::Success(v) => {
                        b.put("fallback", v);
                    }
                    other => return other,
                }
            }
        }
    }

    if !c.siblings.is_empty() {
        let mut extra = Vec::with_capacity(c.siblings.len());
        for sib in &c.siblings {
            match encode_component(sib) {
                DataResult::Success(v) => extra.push(v),
                other => return other,
            }
        }
        b.put("extra", Dyn::List(extra));
    }

    // Style fields (only set ones).
    let st = &c.style;
    if let Some(color) = st.color {
        b.put("color", Dyn::string(color.serialize()));
    }
    if let Some(sc) = st.shadow_color {
        b.put("shadow_color", Dyn::Num(Num::Int(sc)));
    }
    if let Some(v) = st.bold {
        b.put("bold", Dyn::Bool(v));
    }
    if let Some(v) = st.italic {
        b.put("italic", Dyn::Bool(v));
    }
    if let Some(v) = st.underlined {
        b.put("underlined", Dyn::Bool(v));
    }
    if let Some(v) = st.strikethrough {
        b.put("strikethrough", Dyn::Bool(v));
    }
    if let Some(v) = st.obfuscated {
        b.put("obfuscated", Dyn::Bool(v));
    }
    if let Some(ce) = &st.click_event {
        match encode_click_event(ce) {
            DataResult::Success(v) => {
                b.put("click_event", v);
            }
            other => return other,
        }
    }
    if let Some(he) = &st.hover_event {
        match encode_hover_event(he) {
            DataResult::Success(v) => {
                b.put("hover_event", v);
            }
            other => return other,
        }
    }
    if let Some(ins) = &st.insertion {
        b.put("insertion", Dyn::string(ins));
    }
    if let Some(font) = &st.font {
        b.put("font", Dyn::string(font.to_string()));
    }

    DataResult::Success(b.build())
}

fn encode_translation_arg(arg: &TranslationArg) -> DataResult<Dyn> {
    match arg {
        TranslationArg::Null => DataResult::Success(Dyn::string("null")),
        TranslationArg::Bool(b) => DataResult::Success(Dyn::Bool(*b)),
        TranslationArg::Num(n) => DataResult::Success(Dyn::Num(n.clone())),
        TranslationArg::Str(s) => DataResult::Success(Dyn::Str(s.clone())),
        TranslationArg::Component(c) => encode_component(c),
    }
}

/// Encodes a [`Component`] to [`Json`].
pub fn encode_component_json(c: &Component) -> DataResult<Json> {
    encode_component(c).and_then(|d| match d.to_json() {
        Ok(j) => DataResult::Success(j),
        Err(m) => DataResult::error(m),
    })
}

/// Encodes a [`Component`] to a compact JSON string.
pub fn encode_component_json_string(c: &Component) -> DataResult<String> {
    encode_component_json(c).map(|j| j.to_json_string())
}

/// Encodes a [`Component`] to an NBT [`Tag`].
pub fn encode_component_nbt(c: &Component) -> DataResult<Tag> {
    encode_component(c).and_then(|d| match d.to_tag() {
        Ok(t) => DataResult::Success(t),
        Err(m) => DataResult::error(m),
    })
}

/// Encodes a [`Component`] to network NBT bytes (nameless root tag).
pub fn encode_component_network_nbt(c: &Component) -> Result<Vec<u8>, String> {
    let tag = encode_component_nbt(c).demand().map_err(|e| e.message)?;
    zenith_nbt::to_bytes_network(Some(&tag)).map_err(|e| e.to_string())
}
