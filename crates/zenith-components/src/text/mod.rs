//! Text components (spec Part A): model, JSON/NBT codecs, translation, flattening and server-side resolution.

pub mod decode;
pub mod encode;
pub mod events;
pub mod flatten;
pub mod resolve;
pub mod style;
pub mod translate;

use zenith_nbt::NbtString;
use zenith_registry::Identifier;

use crate::dynval::Num;
use crate::profile::Profile;

pub use decode::{
    BOOK_PAGE_MAX_JSON_CHARS, CONTENT_KIND_ORDER, LOGIN_DISCONNECT_MAX_CHARS, NBT_SOURCE_ORDER,
    OBJECT_KIND_ORDER, decode_book_page, decode_component, decode_component_json,
    decode_component_nbt, decode_component_network_nbt, decode_component_str_json,
    decode_login_disconnect_json,
};
pub use encode::{
    encode_component, encode_component_json, encode_component_json_string, encode_component_nbt,
    encode_component_network_nbt,
};
pub use events::{ClickEvent, Dialog, HoverEvent};
pub use flatten::{
    ClientKeybindTable, DEFAULT_FONT, KeybindProvider, OBJECT_PLACEHOLDER_CHAR, RenderFont,
    RenderStyle, StyledRun, flatten_plain, flatten_plain_utf16, flatten_to_list, flatten_truncated,
    flatten_truncated_utf16, render_styled_runs, strip_formatting_codes,
    strip_formatting_codes_utf16,
};
pub use resolve::{
    AllowAllObjects, DEFAULT_DEPTH_LIMIT, DEFAULT_NBT_CAP, DepthLimitBehavior,
    EntitySelectorLookup, MOTD_DEPTH_LIMIT, MotdObjectFilter, NbtDataLookup, ObjectFilter,
    ResolveContext, ResolveError, ResolveSource, ResolvedEntity, ScoreLookup,
    default_nbt_separator, default_selector_separator, format_nbt_tag_colored, resolve_book_pages,
    resolve_component, resolve_motd,
};
pub use style::{NAMED_COLORS, Style, TextColor, shadow_color_from_floats};
pub use translate::{
    DecomposedPart, EmptyLanguage, LanguageProvider, LanguageTable, decompose_translatable,
    lookup_template, rewrite_lang_value,
};

/// A node of a text-component tree.
#[derive(Debug, Clone, PartialEq)]
pub struct Component {
    /// What the node shows.
    pub content: Content,
    /// Children rendered after the content, inheriting the node's resolved style.
    pub siblings: Vec<Component>,
    /// The node's own style.
    pub style: Style,
}

/// The seven content kinds (A.2).
#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    /// Literal text.
    Literal(NbtString),
    /// A translation.
    Translatable(Translatable),
    /// A key binding name.
    Keybind(String),
    /// A scoreboard score.
    Score(ScoreContent),
    /// An entity selector.
    Selector(SelectorContent),
    /// An NBT query.
    Nbt(NbtContent),
    /// An inline sprite object.
    Object(ObjectContent),
}

/// A translation argument.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum TranslationArg {
    /// `null` (in memory only).
    Null,
    /// A boolean.
    Bool(bool),
    /// A number with its stored type.
    Num(Num),
    /// A plain string.
    Str(NbtString),
    /// A component.
    Component(Component),
}

/// Translatable content.
#[derive(Debug, Clone, PartialEq)]
pub struct Translatable {
    /// Translation key.
    pub key: String,
    /// Fallback template.
    pub fallback: Option<String>,
    /// Arguments.
    pub args: Vec<TranslationArg>,
}

/// The holder named by a score content.
#[derive(Debug, Clone, PartialEq)]
pub enum ScoreName {
    /// Parsed as an entity selector.
    Selector(String),
    /// A plain name.
    Plain(String),
}

impl ScoreName {
    /// The original string.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Selector(s) | Self::Plain(s) => s,
        }
    }
}

/// Score content.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreContent {
    /// Holder.
    pub name: ScoreName,
    /// Objective name.
    pub objective: String,
}

/// Selector content.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectorContent {
    /// Selector source text.
    pub pattern: String,
    /// Separator between matched names.
    pub separator: Option<Box<Component>>,
}

/// Where an `nbt` content reads from.
#[derive(Debug, Clone, PartialEq)]
pub enum NbtSource {
    /// Entities matched by a selector string.
    Entity(String),
    /// The block entity at a block-position argument string.
    Block(String),
    /// Command storage.
    Storage(Identifier),
}

/// NBT content.
#[derive(Debug, Clone, PartialEq)]
pub struct NbtContent {
    /// The NBT path text.
    pub path: String,
    /// Interpret values as components.
    pub interpret: bool,
    /// Render without syntax colours.
    pub plain: bool,
    /// Separator between values.
    pub separator: Option<Box<Component>>,
    /// Data source.
    pub source: NbtSource,
}

/// Object info.
#[derive(Debug, Clone, PartialEq)]
pub enum ObjectInfo {
    /// A sprite of an atlas.
    Atlas {
        /// Sprite id.
        sprite: Identifier,
        /// Atlas id (default `minecraft:blocks`).
        atlas: Identifier,
    },
    /// A player head.
    Player {
        /// The profile.
        profile: Profile,
        /// Whether the hat layer shows.
        hat: bool,
    },
}

/// Object content.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectContent {
    /// What the object shows.
    pub info: ObjectInfo,
    /// Fallback shown when the object cannot be rendered.
    pub fallback: Option<Box<Component>>,
}

/// The default sprite atlas.
pub const DEFAULT_ATLAS: &str = "minecraft:blocks";

/// The default sprite atlas as an identifier.
pub fn default_atlas() -> Identifier {
    Identifier::parse(DEFAULT_ATLAS).expect("valid constant")
}

impl ObjectContent {
    /// The default fallback text (A.2.2).
    pub fn default_fallback_text(&self) -> String {
        match &self.info {
            ObjectInfo::Atlas { sprite, atlas } => {
                if *atlas == default_atlas() {
                    format!("[{}]", sprite.to_short_string())
                } else {
                    format!("[{}@{}]", sprite.to_short_string(), atlas.to_short_string())
                }
            }
            ObjectInfo::Player { profile, .. } => match &profile.name {
                Some(n) => format!("[{n} head]"),
                None => "[unknown player head]".to_owned(),
            },
        }
    }
}

impl Component {
    /// A node with the given content, no siblings and the empty style.
    pub fn new(content: Content) -> Self {
        Self {
            content,
            siblings: Vec::new(),
            style: Style::empty(),
        }
    }

    /// Literal text.
    pub fn literal(text: &str) -> Self {
        Self::new(Content::Literal(NbtString::from(text)))
    }

    /// The empty literal.
    pub fn empty() -> Self {
        Self::literal("")
    }

    /// A translation with the given arguments.
    pub fn translatable(key: &str, args: Vec<TranslationArg>) -> Self {
        Self::new(Content::Translatable(Translatable {
            key: key.to_owned(),
            fallback: None,
            args,
        }))
    }

    /// Builder: sets the style.
    pub fn with_style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Builder: appends a sibling.
    pub fn append(mut self, sibling: Component) -> Self {
        self.siblings.push(sibling);
        self
    }

    /// Whether this is plain literal text with no style and no siblings (`Some(text)` if so).
    pub fn as_plain_literal(&self) -> Option<&NbtString> {
        match &self.content {
            Content::Literal(s) if self.siblings.is_empty() && self.style.is_empty() => Some(s),
            _ => None,
        }
    }
}
