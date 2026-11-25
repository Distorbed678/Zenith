//! Server-side and MOTD text-component resolution (spec A.9).

use std::fmt;

use zenith_nbt::{Compound, Tag};
use zenith_registry::Identifier;

use super::decode::{BOOK_PAGE_MAX_JSON_CHARS, decode_component_nbt};
use super::encode::encode_component_json_string;
use super::events::HoverEvent;
use super::style::{Style, TextColor};
use super::{
    Component, Content, NbtContent, NbtSource, ObjectContent, ObjectInfo, ScoreContent, ScoreName,
    SelectorContent, Translatable, TranslationArg,
};
use crate::nbt_path::NbtPath;

/// Default resolution depth limit for commands, signs, text displays, books, loot, and boss bars (spec A.9.2).
pub const DEFAULT_DEPTH_LIMIT: usize = 100;

/// Resolution depth limit for client server-list MOTD sanitizing (spec A.9.2).
pub const MOTD_DEPTH_LIMIT: usize = 16;

/// Maximum resolved-nbt counter value that still resolves (spec A.9.1, edge case 33).
/// Counter starts at 0 and increments per resolved `nbt` content; values `<= 65_536` resolve,
/// meaning the first **65,537** `nbt` contents resolve and later ones become empty.
pub const DEFAULT_NBT_CAP: usize = 65_536;

/// What happens when resolution depth exceeds the context's limit (spec A.9.1 rule 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepthLimitBehavior {
    /// Default: return a copy of the remaining subtree unresolved.
    CopyUnresolved,
    /// Server-list MOTD: replace the node with the literal `"..."`.
    Ellipsis,
}

/// Errors that can be raised during server-side resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// A score selector matched more than one entity ("Only one entity is allowed").
    TooManyEntities {
        /// The selector that matched too many entities.
        selector: String,
    },
    /// Another command / selector error.
    CommandError(String),
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyEntities { selector } => {
                write!(
                    f,
                    "Only one entity is allowed, but selector '{selector}' matched more"
                )
            }
            Self::CommandError(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for ResolveError {}

/// An entity matched by an entity selector during resolution.
#[derive(Debug, Clone)]
pub struct ResolvedEntity {
    /// The entity's scoreboard holder name (player name or entity UUID string).
    pub score_holder_name: String,
    /// The entity's display name component (used by `selector` contents).
    pub display_name: Component,
    /// The entity's serialized NBT compound (used by `nbt` `entity` sources).
    pub nbt: Option<Compound>,
}

/// External scoreboard lookup for `score` contents.
pub trait ScoreLookup {
    /// Looks up the formatted score component for `(holder, objective)`.
    ///
    /// Returns `None` if the objective or score does not exist. When present, the returned
    /// component should already reflect the score's own number format, else the objective's, else unstyled.
    fn get_score(&self, holder: &str, objective: &str) -> Option<Component>;
}

/// External entity selector lookup for `score`, `selector`, and `nbt` (`entity`) contents.
pub trait EntitySelectorLookup {
    /// Evaluates `selector` against the command source, returning matched entities in selector order.
    fn resolve_entities(&self, selector: &str) -> Result<Vec<ResolvedEntity>, ResolveError>;
}

/// External NBT data lookup for `nbt` (`block` and `storage`) contents.
pub trait NbtDataLookup {
    /// Returns the block entity compound at `block_pos` if the position is loaded and has a block entity.
    fn block_entity_nbt(&self, block_pos: &str) -> Option<Compound>;

    /// Returns the command storage compound for `id` (always exists, even if empty).
    fn storage_nbt(&self, id: &Identifier) -> Compound;
}

/// Combined server-side resolution data source.
pub trait ResolveSource: ScoreLookup + EntitySelectorLookup + NbtDataLookup {}

impl<T: ScoreLookup + EntitySelectorLookup + NbtDataLookup> ResolveSource for T {}

/// Filter deciding which `object` contents are permitted in a resolution context.
pub trait ObjectFilter {
    /// Returns `true` to keep the object, `false` to replace it with its fallback.
    fn allow(&self, info: &ObjectInfo) -> bool;
}

/// Allows all object kinds (default for server commands, signs, books, etc.).
#[derive(Debug, Clone, Copy, Default)]
pub struct AllowAllObjects;

impl ObjectFilter for AllowAllObjects {
    fn allow(&self, _info: &ObjectInfo) -> bool {
        true
    }
}

/// Server-list MOTD object filter: rejects `player` head objects (spec A.9.2, edge case 31).
#[derive(Debug, Clone, Copy, Default)]
pub struct MotdObjectFilter;

impl ObjectFilter for MotdObjectFilter {
    fn allow(&self, info: &ObjectInfo) -> bool {
        !matches!(info, ObjectInfo::Player { .. })
    }
}

/// Resolution execution context (tracks depth limit, source, default `*` score holder, object filter,
/// and the per-resolution resolved-nbt counter).
pub struct ResolveContext<'a> {
    /// Maximum depth that is still resolved (`0..=depth_limit` resolve; `depth_limit + 1` triggers limit behavior).
    pub depth_limit: usize,
    /// Behaviour once depth exceeds `depth_limit`.
    pub on_depth_exceeded: DepthLimitBehavior,
    /// Maximum counter value that still resolves (`65_536` allows 65,537 resolutions).
    pub nbt_cap: usize,
    /// Current resolved-nbt counter (starts at 0).
    pub nbt_count: usize,
    /// Optional server-side lookup source (`None` means no command source: score/selector/nbt become empty).
    pub source: Option<&'a dyn ResolveSource>,
    /// Default scoreboard holder used when a `score` content names `"*"`.
    pub default_score_holder: Option<String>,
    /// Object filter (e.g. [`AllowAllObjects`] or [`MotdObjectFilter`]).
    pub object_filter: &'a dyn ObjectFilter,
    /// Warnings emitted during resolution (e.g. skipped invalid `nbt` `interpret` tags).
    pub warnings: Vec<String>,
}

impl fmt::Debug for ResolveContext<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResolveContext")
            .field("depth_limit", &self.depth_limit)
            .field("on_depth_exceeded", &self.on_depth_exceeded)
            .field("nbt_cap", &self.nbt_cap)
            .field("nbt_count", &self.nbt_count)
            .field("has_source", &self.source.is_some())
            .field("default_score_holder", &self.default_score_holder)
            .field("warnings", &self.warnings)
            .finish()
    }
}

impl<'a> ResolveContext<'a> {
    /// Standard server context (depth 100, copy unresolved, source present, optional `*` holder override).
    pub fn server(source: &'a dyn ResolveSource, default_score_holder: Option<String>) -> Self {
        static ALLOW_ALL: AllowAllObjects = AllowAllObjects;
        Self {
            depth_limit: DEFAULT_DEPTH_LIMIT,
            on_depth_exceeded: DepthLimitBehavior::CopyUnresolved,
            nbt_cap: DEFAULT_NBT_CAP,
            nbt_count: 0,
            source: Some(source),
            default_score_holder,
            object_filter: &ALLOW_ALL,
            warnings: Vec::new(),
        }
    }

    /// Boss-bar name context (depth 100, copy unresolved, no `*` override entity; spec A.9.2).
    pub fn boss_bar(source: &'a dyn ResolveSource) -> Self {
        Self::server(source, None)
    }

    /// Client server-list MOTD sanitizing context (depth 16, `...`, no source, rejects `player` objects).
    pub fn motd() -> Self {
        static MOTD_FILTER: MotdObjectFilter = MotdObjectFilter;
        Self {
            depth_limit: MOTD_DEPTH_LIMIT,
            on_depth_exceeded: DepthLimitBehavior::Ellipsis,
            nbt_cap: DEFAULT_NBT_CAP,
            nbt_count: 0,
            source: None,
            default_score_holder: None,
            object_filter: &MOTD_FILTER,
            warnings: Vec::new(),
        }
    }
}

/// Resolves a text component tree according to spec A.9.1.
pub fn resolve_component(
    c: &Component,
    ctx: &mut ResolveContext<'_>,
) -> Result<Component, ResolveError> {
    resolve_at_depth(c, ctx, 0)
}

/// Sanitizes a server-list MOTD component (spec A.9.2, edge case 31):
/// depth limit 16 (`...`), no source, `player` objects replaced by fallback, resolution errors → empty.
pub fn resolve_motd(c: &Component) -> Component {
    let mut ctx = ResolveContext::motd();
    resolve_component(c, &mut ctx).unwrap_or_else(|_| Component::empty())
}

/// Resolves every page of a written book on first open (spec A.9.2, edge case 36).
///
/// - A page that returns a resolution error keeps its original value.
/// - If any resolved page encodes to more than 32,767 compact JSON characters (UTF-16 units),
///   **no page changes** and the book is still marked resolved (`resolved = true`).
///
/// Returns `(resulting_pages, resolved_flag)`.
pub fn resolve_book_pages(
    pages: &[Component],
    ctx: &mut ResolveContext<'_>,
) -> (Vec<Component>, bool) {
    let mut candidate = Vec::with_capacity(pages.len());
    for page in pages {
        let resolved = resolve_component(page, ctx).unwrap_or_else(|_| page.clone());
        match encode_component_json_string(&resolved) {
            zenith_codec::codec::DataResult::Success(s)
                if s.encode_utf16().count() <= BOOK_PAGE_MAX_JSON_CHARS =>
            {
                candidate.push(resolved);
            }
            _ => {
                // Any oversized (or unencodable) resolved page cancels all page changes; book is marked resolved.
                return (pages.to_vec(), true);
            }
        }
    }
    (candidate, true)
}

fn resolve_at_depth(
    c: &Component,
    ctx: &mut ResolveContext<'_>,
    depth: usize,
) -> Result<Component, ResolveError> {
    if depth > ctx.depth_limit {
        return Ok(match ctx.on_depth_exceeded {
            DepthLimitBehavior::CopyUnresolved => c.clone(),
            DepthLimitBehavior::Ellipsis => Component::literal("..."),
        });
    }

    let next_depth = depth + 1;
    let mut resolved = resolve_content(&c.content, ctx, next_depth)?;

    for sib in &c.siblings {
        resolved
            .siblings
            .push(resolve_at_depth(sib, ctx, next_depth)?);
    }

    // Resolve `show_text` hover event in the node's own style, then overlay original style over resolved style.
    let mut own_style = c.style.clone();
    if let Some(HoverEvent::ShowText(hover_comp)) = &own_style.hover_event {
        let resolved_hover = resolve_at_depth(hover_comp, ctx, next_depth)?;
        own_style.hover_event = Some(HoverEvent::ShowText(Box::new(resolved_hover)));
    }
    resolved.style = own_style.apply_to(&resolved.style);

    Ok(resolved)
}

fn resolve_content(
    content: &Content,
    ctx: &mut ResolveContext<'_>,
    next_depth: usize,
) -> Result<Component, ResolveError> {
    match content {
        Content::Literal(s) => Ok(Component::new(Content::Literal(s.clone()))),
        Content::Keybind(k) => Ok(Component::new(Content::Keybind(k.clone()))),
        Content::Translatable(t) => {
            let mut new_args = Vec::with_capacity(t.args.len());
            for arg in &t.args {
                match arg {
                    TranslationArg::Component(sub) => {
                        new_args.push(TranslationArg::Component(resolve_at_depth(
                            sub, ctx, next_depth,
                        )?));
                    }
                    other => new_args.push(other.clone()),
                }
            }
            Ok(Component::new(Content::Translatable(Translatable {
                key: t.key.clone(),
                fallback: t.fallback.clone(),
                args: new_args,
            })))
        }
        Content::Score(score) => resolve_score(score, ctx),
        Content::Selector(sel) => resolve_selector(sel, ctx, next_depth),
        Content::Nbt(nbt) => resolve_nbt(nbt, ctx, next_depth),
        Content::Object(obj) => resolve_object(obj, ctx, next_depth),
    }
}

fn resolve_score(
    score: &ScoreContent,
    ctx: &mut ResolveContext<'_>,
) -> Result<Component, ResolveError> {
    let Some(source) = ctx.source else {
        return Ok(Component::empty());
    };
    let mut holder = match &score.name {
        ScoreName::Selector(sel) => {
            let entities = source.resolve_entities(sel)?;
            match entities.len() {
                0 => sel.clone(),
                1 => entities[0].score_holder_name.clone(),
                _ => {
                    return Err(ResolveError::TooManyEntities {
                        selector: sel.clone(),
                    });
                }
            }
        }
        ScoreName::Plain(p) => p.clone(),
    };
    if holder == "*"
        && let Some(default_holder) = &ctx.default_score_holder
    {
        holder = default_holder.clone();
    }
    Ok(source
        .get_score(&holder, &score.objective)
        .unwrap_or_else(Component::empty))
}

fn resolve_selector(
    sel: &SelectorContent,
    ctx: &mut ResolveContext<'_>,
    next_depth: usize,
) -> Result<Component, ResolveError> {
    let Some(source) = ctx.source else {
        return Ok(Component::empty());
    };
    let entities = source.resolve_entities(&sel.pattern)?;
    if entities.is_empty() {
        return Ok(Component::empty());
    }
    if entities.len() == 1 {
        return Ok(entities.into_iter().next().unwrap().display_name);
    }
    let sep = match &sel.separator {
        Some(s) => resolve_at_depth(s, ctx, next_depth)?,
        None => default_selector_separator(),
    };
    let names: Vec<Component> = entities.into_iter().map(|e| e.display_name).collect();
    Ok(join_components(names, &sep))
}

fn resolve_nbt(
    nbt: &NbtContent,
    ctx: &mut ResolveContext<'_>,
    next_depth: usize,
) -> Result<Component, ResolveError> {
    let Some(source) = ctx.source else {
        return Ok(Component::empty());
    };
    if ctx.nbt_count > ctx.nbt_cap {
        return Ok(Component::empty());
    }
    ctx.nbt_count += 1;

    let compounds: Vec<Compound> = match &nbt.source {
        NbtSource::Entity(sel) => source
            .resolve_entities(sel)?
            .into_iter()
            .filter_map(|e| e.nbt)
            .collect(),
        NbtSource::Block(pos) => source.block_entity_nbt(pos).into_iter().collect(),
        NbtSource::Storage(id) => vec![source.storage_nbt(id)],
    };

    let Ok(path) = NbtPath::parse(&nbt.path) else {
        return Ok(Component::empty());
    };

    let mut matched_tags = Vec::new();
    for compound in compounds {
        matched_tags.extend(path.get(&Tag::Compound(compound)));
    }
    if matched_tags.is_empty() {
        return Ok(Component::empty());
    }

    let mut rendered = Vec::new();
    if nbt.interpret {
        for tag in &matched_tags {
            match decode_component_nbt(tag).demand() {
                Ok(comp) => {
                    rendered.push(resolve_at_depth(&comp, ctx, next_depth)?);
                }
                Err(e) => {
                    ctx.warnings
                        .push(format!("Failed to interpret NBT tag as component: {e}"));
                }
            }
        }
    } else if nbt.plain {
        for tag in &matched_tags {
            let text = zenith_nbt::to_snbt(tag).unwrap_or_default();
            rendered.push(Component::literal(&text));
        }
    } else {
        for tag in &matched_tags {
            rendered.push(format_nbt_tag_colored(tag));
        }
    }

    if rendered.is_empty() {
        return Ok(Component::empty());
    }
    let sep = match &nbt.separator {
        Some(s) => resolve_at_depth(s, ctx, next_depth)?,
        None => default_nbt_separator(),
    };
    Ok(join_components(rendered, &sep))
}

fn resolve_object(
    obj: &ObjectContent,
    ctx: &mut ResolveContext<'_>,
    next_depth: usize,
) -> Result<Component, ResolveError> {
    let resolved_fallback = match &obj.fallback {
        Some(fb) => Some(Box::new(resolve_at_depth(fb, ctx, next_depth)?)),
        None => None,
    };
    if ctx.object_filter.allow(&obj.info) {
        Ok(Component::new(Content::Object(ObjectContent {
            info: obj.info.clone(),
            fallback: resolved_fallback,
        })))
    } else {
        Ok(match resolved_fallback {
            Some(fb) => *fb,
            None => Component::literal(&obj.default_fallback_text()),
        })
    }
}

/// Default separator for `selector` contents: `", "` in gray (spec A.9.1).
pub fn default_selector_separator() -> Component {
    Component::literal(", ")
        .with_style(Style::empty().with_color(TextColor::named("gray").unwrap()))
}

/// Default separator for `nbt` contents: unstyled `", "` (spec A.9.1).
pub fn default_nbt_separator() -> Component {
    Component::literal(", ")
}

fn join_components(items: Vec<Component>, separator: &Component) -> Component {
    let mut iter = items.into_iter();
    let Some(first) = iter.next() else {
        return Component::empty();
    };
    let mut root = Component::empty();
    root.siblings.push(first);
    for item in iter {
        root.siblings.push(separator.clone());
        root.siblings.push(item);
    }
    if root.siblings.len() == 1 {
        root.siblings.pop().unwrap()
    } else {
        root
    }
}

/// Formats an NBT [`Tag`] as a syntax-coloured SNBT component (strings appear quoted; spec A.9.1,
/// edge case 20, open question 9).
// PARITY [oracle]: settled by the text-component suite (spec open question 9: NBT-to-text rendering).
pub fn format_nbt_tag_colored(tag: &Tag) -> Component {
    let green = Style::empty().with_color(TextColor::named("green").unwrap());
    let gold = Style::empty().with_color(TextColor::named("gold").unwrap());
    let red = Style::empty().with_color(TextColor::named("red").unwrap());
    let aqua = Style::empty().with_color(TextColor::named("aqua").unwrap());

    match tag {
        Tag::End => Component::empty(),
        Tag::String(_) => {
            let quoted = zenith_nbt::to_snbt(tag).unwrap_or_else(|_| "\"\"".to_owned());
            if quoted.len() >= 2 {
                let q = &quoted[..1];
                let inner = &quoted[1..quoted.len() - 1];
                Component::literal(q)
                    .append(Component::literal(inner).with_style(green))
                    .append(Component::literal(q))
            } else {
                Component::literal(&quoted)
            }
        }
        Tag::Byte(v) => Component::literal(&v.to_string())
            .with_style(gold)
            .append(Component::literal("b").with_style(red)),
        Tag::Short(v) => Component::literal(&v.to_string())
            .with_style(gold)
            .append(Component::literal("s").with_style(red)),
        Tag::Int(v) => Component::literal(&v.to_string()).with_style(gold),
        Tag::Long(v) => Component::literal(&v.to_string())
            .with_style(gold)
            .append(Component::literal("L").with_style(red)),
        Tag::Float(v) => Component::literal(&zenith_nbt::java_number::java_float_to_string(*v))
            .with_style(gold)
            .append(Component::literal("f").with_style(red)),
        Tag::Double(v) => Component::literal(&zenith_nbt::java_number::java_double_to_string(*v))
            .with_style(gold)
            .append(Component::literal("d").with_style(red)),
        Tag::ByteArray(arr) => {
            let mut root = Component::literal("[")
                .append(Component::literal("B").with_style(red.clone()))
                .append(Component::literal(";"));
            for (i, &v) in arr.iter().enumerate() {
                root = root.append(Component::literal(" "));
                root = root
                    .append(Component::literal(&v.to_string()).with_style(gold.clone()))
                    .append(Component::literal("B").with_style(red.clone()));
                if i + 1 < arr.len() {
                    root = root.append(Component::literal(","));
                }
            }
            root.append(Component::literal("]"))
        }
        Tag::IntArray(arr) => {
            let mut root = Component::literal("[")
                .append(Component::literal("I").with_style(red.clone()))
                .append(Component::literal(";"));
            for (i, &v) in arr.iter().enumerate() {
                root = root.append(Component::literal(" "));
                root = root.append(Component::literal(&v.to_string()).with_style(gold.clone()));
                if i + 1 < arr.len() {
                    root = root.append(Component::literal(","));
                }
            }
            root.append(Component::literal("]"))
        }
        Tag::LongArray(arr) => {
            let mut root = Component::literal("[")
                .append(Component::literal("L").with_style(red.clone()))
                .append(Component::literal(";"));
            for (i, &v) in arr.iter().enumerate() {
                root = root.append(Component::literal(" "));
                root = root
                    .append(Component::literal(&v.to_string()).with_style(gold.clone()))
                    .append(Component::literal("L").with_style(red.clone()));
                if i + 1 < arr.len() {
                    root = root.append(Component::literal(","));
                }
            }
            root.append(Component::literal("]"))
        }
        Tag::List(list) => {
            let mut root = Component::literal("[");
            for (i, elem) in list.iter().enumerate() {
                if i > 0 {
                    root = root.append(Component::literal(", "));
                }
                root = root.append(format_nbt_tag_colored(elem));
            }
            root.append(Component::literal("]"))
        }
        Tag::Compound(c) => {
            let mut entries: Vec<(Vec<u16>, &zenith_nbt::NbtString, &Tag)> =
                c.iter().map(|(k, v)| (k.to_utf16(), k, v)).collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let mut root = Component::literal("{");
            for (i, (_, k, v)) in entries.into_iter().enumerate() {
                if i > 0 {
                    root = root.append(Component::literal(", "));
                }
                let key_text = k.to_string_lossy();
                root = root
                    .append(Component::literal(&key_text).with_style(aqua.clone()))
                    .append(Component::literal(": "))
                    .append(format_nbt_tag_colored(v));
            }
            root.append(Component::literal("}"))
        }
    }
}
