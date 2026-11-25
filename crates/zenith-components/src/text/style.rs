//! Text style: colours, tri-state flags, events, font, insertion (spec A.3).

use zenith_registry::Identifier;

use super::events::{ClickEvent, HoverEvent};

/// The 16 named colours: (name, legacy code, RGB).
pub const NAMED_COLORS: [(&str, char, u32); 16] = [
    ("black", '0', 0x000000),
    ("dark_blue", '1', 0x0000AA),
    ("dark_green", '2', 0x00AA00),
    ("dark_aqua", '3', 0x00AAAA),
    ("dark_red", '4', 0xAA0000),
    ("dark_purple", '5', 0xAA00AA),
    ("gold", '6', 0xFFAA00),
    ("gray", '7', 0xAAAAAA),
    ("dark_gray", '8', 0x555555),
    ("blue", '9', 0x5555FF),
    ("green", 'a', 0x55FF55),
    ("aqua", 'b', 0x55FFFF),
    ("red", 'c', 0xFF5555),
    ("light_purple", 'd', 0xFF55FF),
    ("yellow", 'e', 0xFFFF55),
    ("white", 'f', 0xFFFFFF),
];

/// A text colour. Equality compares only the RGB value; the name only affects encoding.
#[derive(Debug, Clone, Copy)]
pub struct TextColor {
    rgb: u32,
    name: Option<&'static str>,
}

impl PartialEq for TextColor {
    fn eq(&self, other: &Self) -> bool {
        self.rgb == other.rgb
    }
}
impl Eq for TextColor {}

impl TextColor {
    /// A colour created from an RGB value (`#RRGGBB` when encoded).
    pub fn from_rgb(rgb: u32) -> Self {
        Self {
            rgb: rgb & 0xFF_FFFF,
            name: None,
        }
    }

    /// A named colour (`None` for an unknown name).
    pub fn named(name: &str) -> Option<Self> {
        NAMED_COLORS
            .iter()
            .find(|(n, _, _)| *n == name)
            .map(|(n, _, rgb)| Self {
                rgb: *rgb,
                name: Some(n),
            })
    }

    /// The colour of a legacy `§` code character (case-insensitive).
    pub fn from_legacy_code(code: char) -> Option<Self> {
        let c = code.to_ascii_lowercase();
        NAMED_COLORS
            .iter()
            .find(|(_, k, _)| *k == c)
            .map(|(n, _, rgb)| Self {
                rgb: *rgb,
                name: Some(n),
            })
    }

    /// The RGB value.
    pub fn rgb(&self) -> u32 {
        self.rgb
    }

    /// Parses `color`: a name (case-sensitive) or `#` + hexadecimal.
    pub fn parse(s: &str) -> Result<Self, String> {
        if let Some(hex) = s.strip_prefix('#') {
            let v = java_parse_hex_i32(hex).map_err(|_| format!("Invalid color value: {s}"))?;
            if !(0..=0xFF_FFFF).contains(&v) {
                return Err(format!("Color value out of range: {s}"));
            }
            Ok(Self::from_rgb(v as u32))
        } else {
            Self::named(s).ok_or_else(|| format!("Invalid color name: {s}"))
        }
    }

    /// The encoded form: the name if created from one, else `#RRGGBB`.
    pub fn serialize(&self) -> String {
        match self.name {
            Some(n) => n.to_owned(),
            None => format!("#{:06X}", self.rgb),
        }
    }
}

/// Java `Character.digit(c, 16)`: ASCII, full-width Latin A–F/a–f, and Unicode decimal digits.
// PARITY [oracle]: settled by the text-component suite (hex digits beyond ASCII, spec open question 6).
fn java_digit16(c: char) -> Option<u32> {
    if let Some(d) = c.to_digit(16) {
        return Some(d);
    }
    let u = c as u32;
    if (0xFF21..=0xFF26).contains(&u) {
        return Some(u - 0xFF21 + 10);
    }
    if (0xFF41..=0xFF46).contains(&u) {
        return Some(u - 0xFF41 + 10);
    }
    // Starts of the runs of ten consecutive decimal digits (general category Nd).
    const ND: &[u32] = &[
        0x0660, 0x06F0, 0x07C0, 0x0966, 0x09E6, 0x0A66, 0x0AE6, 0x0B66, 0x0BE6, 0x0C66, 0x0CE6,
        0x0D66, 0x0DE6, 0x0E50, 0x0ED0, 0x0F20, 0x1040, 0x1090, 0x17E0, 0x1810, 0x1946, 0x19D0,
        0x1A80, 0x1A90, 0x1B50, 0x1BB0, 0x1C40, 0x1C50, 0xA620, 0xA8D0, 0xA900, 0xA9D0, 0xA9F0,
        0xAA50, 0xABF0, 0xFF10, 0x104A0, 0x11066, 0x110F0, 0x11136, 0x111D0, 0x112F0, 0x11450,
        0x114D0, 0x11650, 0x116C0, 0x11730, 0x118E0, 0x16A60, 0x16B50, 0x1D7CE, 0x1D7D8, 0x1D7E2,
        0x1D7EC, 0x1D7F6, 0x1E950,
    ];
    ND.iter()
        .find(|s| (**s..**s + 10).contains(&u))
        .map(|s| u - s)
}

/// Java `Integer.parseInt(s, 16)`.
fn java_parse_hex_i32(s: &str) -> Result<i32, ()> {
    let (neg, digits) = if let Some(r) = s.strip_prefix('-') {
        (true, r)
    } else {
        (false, s.strip_prefix('+').unwrap_or(s))
    };
    if digits.is_empty() {
        return Err(());
    }
    let mut acc: i64 = 0;
    for c in digits.chars() {
        let d = java_digit16(c).ok_or(())?;
        acc = acc * 16 + i64::from(d);
        if acc > i64::from(i32::MAX) + 1 {
            return Err(());
        }
    }
    let v = if neg { -acc } else { acc };
    i32::try_from(v).map_err(|_| ())
}

/// A text style. Every field is optional; an all-unset style is the empty style.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Style {
    /// `color`.
    pub color: Option<TextColor>,
    /// `shadow_color` (ARGB).
    pub shadow_color: Option<i32>,
    /// `bold`.
    pub bold: Option<bool>,
    /// `italic`.
    pub italic: Option<bool>,
    /// `underlined`.
    pub underlined: Option<bool>,
    /// `strikethrough`.
    pub strikethrough: Option<bool>,
    /// `obfuscated`.
    pub obfuscated: Option<bool>,
    /// `click_event`.
    pub click_event: Option<ClickEvent>,
    /// `hover_event`.
    pub hover_event: Option<HoverEvent>,
    /// `insertion`.
    pub insertion: Option<String>,
    /// `font`.
    pub font: Option<Identifier>,
}

impl Style {
    /// The empty style.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Whether every field is unset.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// This style's fields over `parent`'s: own fields win, unset ones fall back to the parent.
    pub fn apply_to(&self, parent: &Style) -> Style {
        Style {
            color: self.color.or(parent.color),
            shadow_color: self.shadow_color.or(parent.shadow_color),
            bold: self.bold.or(parent.bold),
            italic: self.italic.or(parent.italic),
            underlined: self.underlined.or(parent.underlined),
            strikethrough: self.strikethrough.or(parent.strikethrough),
            obfuscated: self.obfuscated.or(parent.obfuscated),
            click_event: self
                .click_event
                .clone()
                .or_else(|| parent.click_event.clone()),
            hover_event: self
                .hover_event
                .clone()
                .or_else(|| parent.hover_event.clone()),
            insertion: self.insertion.clone().or_else(|| parent.insertion.clone()),
            font: self.font.clone().or_else(|| parent.font.clone()),
        }
    }

    /// Builder: sets the colour.
    pub fn with_color(mut self, c: TextColor) -> Self {
        self.color = Some(c);
        self
    }

    /// Builder: sets `bold`.
    pub fn with_bold(mut self, b: bool) -> Self {
        self.bold = Some(b);
        self
    }

    /// Builder: sets `italic`.
    pub fn with_italic(mut self, b: bool) -> Self {
        self.italic = Some(b);
        self
    }
}

/// Converts four colour-channel floats to an ARGB integer: each channel is `floor(value × 255)` masked to 8 bits.
pub fn shadow_color_from_floats(r: f32, g: f32, b: f32, a: f32) -> i32 {
    let ch = |v: f32| ((v * 255.0).floor() as i32) & 0xFF;
    (ch(a) << 24) | (ch(r) << 16) | (ch(g) << 8) | ch(b)
}
