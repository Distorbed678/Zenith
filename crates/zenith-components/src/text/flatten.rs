//! Flattening and styled rendering of text components (spec A.2.2, A.3.2, A.8).

use std::collections::HashMap;

use zenith_nbt::NbtString;
use zenith_registry::Identifier;

use super::events::{ClickEvent, HoverEvent};
use super::style::{Style, TextColor};
use super::translate::{DecomposedPart, LanguageProvider, decompose_translatable};
use super::{Component, Content, ObjectInfo};
use crate::profile::Profile;

/// The inline object placeholder character emitted during styled rendering (spec A.2.2).
pub const OBJECT_PLACEHOLDER_CHAR: char = '\u{FFFC}';

/// The default font identifier when unset during rendering (spec A.3.1).
pub const DEFAULT_FONT: &str = "minecraft:default";

/// Provides client key-binding display names for `keybind` contents (spec A.2.2).
///
/// On a dedicated server pass `None` so keybind contents render as their raw key name.
pub trait KeybindProvider {
    /// Resolves a key-mapping name (e.g. `key.jump`) to a component.
    fn resolve(&self, key_name: &str, lang: &dyn LanguageProvider) -> Component;
}

/// Simple client key-binding table: returns the bound display name if known, otherwise falls back to
/// looking the key name up as a translation key (spec A.2.2).
#[derive(Debug, Clone, Default)]
pub struct ClientKeybindTable {
    bindings: HashMap<String, Component>,
}

impl ClientKeybindTable {
    /// Creates an empty key-binding table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds `key_name` to a display component.
    pub fn bind(&mut self, key_name: impl Into<String>, display: Component) {
        self.bindings.insert(key_name.into(), display);
    }
}

impl KeybindProvider for ClientKeybindTable {
    fn resolve(&self, key_name: &str, lang: &dyn LanguageProvider) -> Component {
        if let Some(bound) = self.bindings.get(key_name) {
            bound.clone()
        } else if let Some(translated) = lang.get(key_name) {
            Component::literal(translated)
        } else {
            Component::literal(key_name)
        }
    }
}

/// Render-time font for a styled run (sprite fonts exist at render time only and are never serialized).
#[derive(Debug, Clone, PartialEq)]
pub enum RenderFont {
    /// A font resource identifier (default `minecraft:default`).
    Resource(Identifier),
    /// An atlas sprite font produced by an `object` atlas content.
    AtlasSprite {
        /// Atlas id.
        atlas: Identifier,
        /// Sprite id.
        sprite: Identifier,
    },
    /// A player head sprite font produced by an `object` player content.
    PlayerSprite {
        /// Player profile.
        profile: Profile,
        /// Whether the hat layer is shown.
        hat: bool,
    },
}

/// Effective render-time style of a styled run.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderStyle {
    /// Text colour (`None` means context default).
    pub color: Option<TextColor>,
    /// Shadow ARGB colour.
    pub shadow_color: Option<i32>,
    /// Bold flag (unset renders as false).
    pub bold: bool,
    /// Italic flag (unset renders as false).
    pub italic: bool,
    /// Underlined flag (unset renders as false).
    pub underlined: bool,
    /// Strikethrough flag (unset renders as false).
    pub strikethrough: bool,
    /// Obfuscated flag (unset renders as false).
    pub obfuscated: bool,
    /// Click event.
    pub click_event: Option<ClickEvent>,
    /// Hover event.
    pub hover_event: Option<HoverEvent>,
    /// Shift-click insertion text.
    pub insertion: Option<String>,
    /// Effective render font.
    pub font: RenderFont,
}

impl RenderStyle {
    /// Builds a [`RenderStyle`] from an effective serialized [`Style`].
    pub fn from_style(style: &Style) -> Self {
        let font = style
            .font
            .clone()
            .unwrap_or_else(|| Identifier::parse(DEFAULT_FONT).expect("valid default font"));
        Self {
            color: style.color,
            shadow_color: style.shadow_color,
            bold: style.bold.unwrap_or(false),
            italic: style.italic.unwrap_or(false),
            underlined: style.underlined.unwrap_or(false),
            strikethrough: style.strikethrough.unwrap_or(false),
            obfuscated: style.obfuscated.unwrap_or(false),
            click_event: style.click_event.clone(),
            hover_event: style.hover_event.clone(),
            insertion: style.insertion.clone(),
            font: RenderFont::Resource(font),
        }
    }
}

/// One styled run of rendered text (with `§` codes interpreted and unpaired surrogates replaced by U+FFFD).
#[derive(Debug, Clone, PartialEq)]
pub struct StyledRun {
    /// Rendered text (always valid UTF-8; unpaired surrogates become U+FFFD).
    pub text: String,
    /// Effective style for this run.
    pub style: RenderStyle,
}

// ----------------------------------------------------------------------------------------------------------
// Plain-string flattening (spec A.8 rules 1 & 2)
// ----------------------------------------------------------------------------------------------------------

/// Flattens a component to an exact UTF-16 plain string (spec A.8 rule 1, edge cases 34 & 35):
/// `§` formatting codes and unpaired UTF-16 surrogates are kept as-is.
pub fn flatten_plain_utf16(
    c: &Component,
    lang: &dyn LanguageProvider,
    keybinds: Option<&dyn KeybindProvider>,
) -> NbtString {
    let mut out = Utf16Buf::default();
    walk_plain(c, lang, keybinds, None, &mut out);
    out.finish()
}

/// Flattens a component to a plain Rust `String` (spec A.8 rule 1; keeps `§` codes verbatim).
pub fn flatten_plain(
    c: &Component,
    lang: &dyn LanguageProvider,
    keybinds: Option<&dyn KeybindProvider>,
) -> String {
    flatten_plain_utf16(c, lang, keybinds)
        .to_string_lossy()
        .into_owned()
}

/// Flattens a component to a plain string truncated to at most `max_utf16_units` UTF-16 code units
/// (spec A.8 rule 2).
pub fn flatten_truncated_utf16(
    c: &Component,
    max_utf16_units: usize,
    lang: &dyn LanguageProvider,
    keybinds: Option<&dyn KeybindProvider>,
) -> NbtString {
    let mut out = Utf16Buf::default();
    walk_plain(c, lang, keybinds, Some(max_utf16_units), &mut out);
    out.finish()
}

/// Convenience wrapper returning a truncated plain Rust `String`.
pub fn flatten_truncated(
    c: &Component,
    max_utf16_units: usize,
    lang: &dyn LanguageProvider,
    keybinds: Option<&dyn KeybindProvider>,
) -> String {
    flatten_truncated_utf16(c, max_utf16_units, lang, keybinds)
        .to_string_lossy()
        .into_owned()
}

#[derive(Default)]
struct Utf16Buf {
    units: Vec<u16>,
}

impl Utf16Buf {
    fn push_nbt(&mut self, s: &NbtString, limit: Option<usize>) -> bool {
        if let Some(lim) = limit {
            if self.units.len() >= lim {
                return false;
            }
            let remaining = lim - self.units.len();
            if let Some(utf8) = s.as_str() {
                for u in utf8.encode_utf16().take(remaining) {
                    self.units.push(u);
                }
            } else {
                let all = s.to_utf16();
                let take = all.len().min(remaining);
                self.units.extend_from_slice(&all[..take]);
            }
            self.units.len() < lim
        } else {
            if let Some(utf8) = s.as_str() {
                self.units.extend(utf8.encode_utf16());
            } else {
                self.units.extend_from_slice(&s.to_utf16());
            }
            true
        }
    }

    fn push_str(&mut self, s: &str, limit: Option<usize>) -> bool {
        if let Some(lim) = limit {
            if self.units.len() >= lim {
                return false;
            }
            let remaining = lim - self.units.len();
            for u in s.encode_utf16().take(remaining) {
                self.units.push(u);
            }
            self.units.len() < lim
        } else {
            self.units.extend(s.encode_utf16());
            true
        }
    }

    fn finish(self) -> NbtString {
        NbtString::from_utf16(self.units)
    }
}

fn walk_plain(
    c: &Component,
    lang: &dyn LanguageProvider,
    keybinds: Option<&dyn KeybindProvider>,
    limit: Option<usize>,
    out: &mut Utf16Buf,
) -> bool {
    let cont = match &c.content {
        Content::Literal(s) => out.push_nbt(s, limit),
        Content::Translatable(t) => {
            let mut ok = true;
            for part in decompose_translatable(t, lang) {
                ok = match part {
                    DecomposedPart::Text(s) => out.push_nbt(&s, limit),
                    DecomposedPart::Component(sub) => walk_plain(sub, lang, keybinds, limit, out),
                };
                if !ok {
                    break;
                }
            }
            ok
        }
        Content::Keybind(k) => match keybinds {
            Some(kb) => {
                let resolved = kb.resolve(k, lang);
                walk_plain(&resolved, lang, keybinds, limit, out)
            }
            None => out.push_str(k, limit),
        },
        Content::Score(_) | Content::Nbt(_) => true,
        Content::Selector(sel) => out.push_str(&sel.pattern, limit),
        Content::Object(obj) => match &obj.fallback {
            Some(fb) => walk_plain(fb, lang, keybinds, limit, out),
            None => out.push_str(&obj.default_fallback_text(), limit),
        },
    };
    if !cont {
        return false;
    }
    for sib in &c.siblings {
        if !walk_plain(sib, lang, keybinds, limit, out) {
            return false;
        }
    }
    true
}

// ----------------------------------------------------------------------------------------------------------
// Legacy `§` formatting code stripping & styled rendering (spec A.8 rules 3, 4 & 5)
// ----------------------------------------------------------------------------------------------------------

/// Strips legacy `§<code>` formatting codes from a UTF-8 string (spec A.8 rule 3):
/// both `§` and the following code character are consumed (even for unknown codes), and a trailing lone `§`
/// is dropped.
pub fn strip_formatting_codes(s: &str) -> String {
    strip_formatting_codes_utf16(&NbtString::from(s))
}

/// Strips legacy `§<code>` formatting codes from a lossless [`NbtString`], converting unpaired UTF-16
/// surrogates to U+FFFD (spec A.8 rules 3 & 4).
pub fn strip_formatting_codes_utf16(s: &NbtString) -> String {
    let units = s.to_utf16();
    let mut out = String::new();
    let mut i = 0;
    while i < units.len() {
        let u = units[i];
        if u == 0x00A7 {
            // Consume `§` and the following UTF-16 code unit (if any); trailing `§` is dropped.
            i += 2;
            continue;
        }
        let (ch, step) = decode_utf16_unit(&units, i);
        out.push(ch);
        i += step;
    }
    out
}

/// Renders a component tree into styled runs with `§` formatting codes applied within each segment
/// (never leaking across siblings), unpaired surrogates replaced by U+FFFD, and `object` contents
/// emitted as U+FFFC with sprite fonts (spec A.2.2, A.3.2, A.8 rules 3 & 4).
pub fn render_styled_runs(
    c: &Component,
    parent_style: &Style,
    lang: &dyn LanguageProvider,
    keybinds: Option<&dyn KeybindProvider>,
) -> Vec<StyledRun> {
    let mut runs = Vec::new();
    walk_styled_runs(c, parent_style, lang, keybinds, &mut runs);
    runs
}

fn walk_styled_runs(
    c: &Component,
    parent_style: &Style,
    lang: &dyn LanguageProvider,
    keybinds: Option<&dyn KeybindProvider>,
    runs: &mut Vec<StyledRun>,
) {
    let eff = c.style.apply_to(parent_style);
    match &c.content {
        Content::Literal(s) => emit_formatted_segment(s, &eff, runs),
        Content::Translatable(t) => {
            for part in decompose_translatable(t, lang) {
                match part {
                    DecomposedPart::Text(s) => emit_formatted_segment(&s, &eff, runs),
                    DecomposedPart::Component(sub) => {
                        walk_styled_runs(sub, &eff, lang, keybinds, runs);
                    }
                }
            }
        }
        Content::Keybind(k) => match keybinds {
            Some(kb) => {
                let resolved = kb.resolve(k, lang);
                walk_styled_runs(&resolved, &eff, lang, keybinds, runs);
            }
            None => emit_formatted_segment(&NbtString::from(k.as_str()), &eff, runs),
        },
        Content::Score(_) | Content::Nbt(_) => {}
        Content::Selector(sel) => {
            emit_formatted_segment(&NbtString::from(sel.pattern.as_str()), &eff, runs);
        }
        Content::Object(obj) => {
            let mut rs = RenderStyle::from_style(&eff);
            rs.font = match &obj.info {
                ObjectInfo::Atlas { sprite, atlas } => RenderFont::AtlasSprite {
                    atlas: atlas.clone(),
                    sprite: sprite.clone(),
                },
                ObjectInfo::Player { profile, hat } => RenderFont::PlayerSprite {
                    profile: profile.clone(),
                    hat: *hat,
                },
            };
            runs.push(StyledRun {
                text: OBJECT_PLACEHOLDER_CHAR.to_string(),
                style: rs,
            });
        }
    }
    for sib in &c.siblings {
        walk_styled_runs(sib, &eff, lang, keybinds, runs);
    }
}

fn emit_formatted_segment(s: &NbtString, base_style: &Style, runs: &mut Vec<StyledRun>) {
    let units = s.to_utf16();
    if units.is_empty() {
        return;
    }
    let base_render = RenderStyle::from_style(base_style);
    let mut cur_style = base_render.clone();
    let mut cur_text = String::new();
    let mut i = 0;

    while i < units.len() {
        let u = units[i];
        if u == 0x00A7 {
            if i + 1 >= units.len() {
                // Trailing lone `§` is dropped.
                break;
            }
            let code_unit = units[i + 1];
            i += 2;
            let next_style = apply_legacy_code(&cur_style, &base_render, code_unit);
            if next_style != cur_style {
                if !cur_text.is_empty() {
                    runs.push(StyledRun {
                        text: std::mem::take(&mut cur_text),
                        style: cur_style,
                    });
                }
                cur_style = next_style;
            }
            continue;
        }
        let (ch, step) = decode_utf16_unit(&units, i);
        cur_text.push(ch);
        i += step;
    }

    if !cur_text.is_empty() {
        runs.push(StyledRun {
            text: cur_text,
            style: cur_style,
        });
    }
}

fn apply_legacy_code(cur: &RenderStyle, base: &RenderStyle, code_unit: u16) -> RenderStyle {
    let Some(c) = char::from_u32(u32::from(code_unit)) else {
        return cur.clone();
    };
    let lower = c.to_ascii_lowercase();
    if let Some(color) = TextColor::from_legacy_code(lower) {
        let mut s = cur.clone();
        s.color = Some(color);
        s.bold = false;
        s.italic = false;
        s.underlined = false;
        s.strikethrough = false;
        s.obfuscated = false;
        return s;
    }
    let mut s = cur.clone();
    match lower {
        'k' => s.obfuscated = true,
        'l' => s.bold = true,
        'm' => s.strikethrough = true,
        'n' => s.underlined = true,
        'o' => s.italic = true,
        'r' => return base.clone(),
        _ => {}
    }
    s
}

/// Decodes one character from UTF-16 units starting at `i`, turning unpaired surrogates into U+FFFD.
fn decode_utf16_unit(units: &[u16], i: usize) -> (char, usize) {
    let u = units[i];
    if (0xD800..0xDC00).contains(&u) {
        if let Some(&lo) = units.get(i + 1)
            && (0xDC00..0xE000).contains(&lo)
        {
            let cp = 0x10000 + ((u32::from(u) - 0xD800) << 10) + (u32::from(lo) - 0xDC00);
            return (char::from_u32(cp).unwrap_or('\u{FFFD}'), 2);
        }
        return ('\u{FFFD}', 1);
    }
    if (0xDC00..0xE000).contains(&u) {
        return ('\u{FFFD}', 1);
    }
    (char::from_u32(u32::from(u)).unwrap_or('\u{FFFD}'), 1)
}

/// Flattens a component to a list of single-literal components, one per non-empty text segment, each
/// carrying its effective [`Style`] (spec A.8 rule 5).
pub fn flatten_to_list(
    c: &Component,
    parent_style: &Style,
    lang: &dyn LanguageProvider,
    keybinds: Option<&dyn KeybindProvider>,
) -> Vec<Component> {
    let mut out = Vec::new();
    walk_flat_list(c, parent_style, lang, keybinds, &mut out);
    out
}

fn walk_flat_list(
    c: &Component,
    parent_style: &Style,
    lang: &dyn LanguageProvider,
    keybinds: Option<&dyn KeybindProvider>,
    out: &mut Vec<Component>,
) {
    let eff = c.style.apply_to(parent_style);
    match &c.content {
        Content::Literal(s) => {
            if !s.is_empty() {
                out.push(Component {
                    content: Content::Literal(s.clone()),
                    siblings: Vec::new(),
                    style: eff.clone(),
                });
            }
        }
        Content::Translatable(t) => {
            for part in decompose_translatable(t, lang) {
                match part {
                    DecomposedPart::Text(s) => {
                        if !s.is_empty() {
                            out.push(Component {
                                content: Content::Literal(s),
                                siblings: Vec::new(),
                                style: eff.clone(),
                            });
                        }
                    }
                    DecomposedPart::Component(sub) => {
                        walk_flat_list(sub, &eff, lang, keybinds, out);
                    }
                }
            }
        }
        Content::Keybind(k) => match keybinds {
            Some(kb) => {
                let resolved = kb.resolve(k, lang);
                walk_flat_list(&resolved, &eff, lang, keybinds, out);
            }
            None => {
                if !k.is_empty() {
                    out.push(Component {
                        content: Content::Literal(NbtString::from(k.as_str())),
                        siblings: Vec::new(),
                        style: eff.clone(),
                    });
                }
            }
        },
        Content::Score(_) | Content::Nbt(_) => {}
        Content::Selector(sel) => {
            if !sel.pattern.is_empty() {
                out.push(Component {
                    content: Content::Literal(NbtString::from(sel.pattern.as_str())),
                    siblings: Vec::new(),
                    style: eff.clone(),
                });
            }
        }
        Content::Object(obj) => match &obj.fallback {
            Some(fb) => walk_flat_list(fb, &eff, lang, keybinds, out),
            None => {
                let txt = obj.default_fallback_text();
                if !txt.is_empty() {
                    out.push(Component {
                        content: Content::Literal(NbtString::from(txt)),
                        siblings: Vec::new(),
                        style: eff.clone(),
                    });
                }
            }
        },
    }
    for sib in &c.siblings {
        walk_flat_list(sib, &eff, lang, keybinds, out);
    }
}
