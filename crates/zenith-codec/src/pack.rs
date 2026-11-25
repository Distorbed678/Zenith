//! Pack identity, discovery and the repository (spec §1.3, §2).

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use zenith_registry::{FeatureFlagSet, FeatureUniverse, Identifier, KnownPack};

use crate::format::{Compatibility, FormatRange, PackFormat, PackType};
use crate::log::{LogExt, SharedLog};
use crate::meta::{
    DropReason, FilterSection, McmetaFile, PackMetadata, decode_features_section,
    decode_filter_section, decode_overlay_entries, decode_pack_section, read_pack_metadata,
    validate_overlays,
};
use crate::resources::{
    FolderPackResources, LayeredPackResources, PACK_META, PackResources, ResourceError,
    ZipPackResources,
};
use crate::text::Component;

/// Where a pack came from (spec §1.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PackSource {
    /// `vanilla` and built-in resource packs.
    BuiltIn,
    /// Built-in experimental data packs: **never** added automatically.
    Feature,
    /// The world's `datapacks/` folder.
    World,
    /// Server-downloaded resource packs.
    Server,
    /// The client `resourcepacks/` folder.
    Default,
}

impl PackSource {
    /// Whether a newly discovered pack from this source is added to the selection automatically.
    pub fn auto_add_when_new(self) -> bool {
        !matches!(self, Self::Feature)
    }
}

/// Default position of a pack in the stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Position {
    /// Highest priority end.
    Top,
    /// Lowest priority end.
    Bottom,
}

/// A pack's selection configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionConfig {
    /// Always part of the selection.
    pub required: bool,
    /// Where it goes when inserted by default.
    pub default_position: Position,
    /// Whether the position is fixed (cannot be moved by the user).
    pub fixed: bool,
}

impl SelectionConfig {
    /// Not required, default TOP, not fixed (world, feature and built-in resource packs).
    pub const OPTIONAL_TOP: Self = Self {
        required: false,
        default_position: Position::Top,
        fixed: false,
    };
    /// Not required, default BOTTOM, not fixed (the data `vanilla` pack).
    pub const OPTIONAL_BOTTOM: Self = Self {
        required: false,
        default_position: Position::Bottom,
        fixed: false,
    };
    /// Required, default BOTTOM, not fixed (the client `vanilla` pack).
    pub const REQUIRED_BOTTOM: Self = Self {
        required: true,
        default_position: Position::Bottom,
        fixed: false,
    };
    /// Required, TOP, fixed (server-downloaded resource packs).
    pub const REQUIRED_FIXED_TOP: Self = Self {
        required: true,
        default_position: Position::Top,
        fixed: true,
    };
}

/// One discovered pack.
#[derive(Debug, Clone)]
pub struct Pack {
    /// Unique id within its repository.
    pub id: String,
    /// Title.
    pub title: Component,
    /// Source kind.
    pub source: PackSource,
    /// Known-pack info (namespace, id, version).
    pub known_pack: Option<KnownPack>,
    /// Metadata read from `pack.mcmeta`.
    pub metadata: PackMetadata,
    /// Selection configuration.
    pub selection: SelectionConfig,
    /// Pack type.
    pub ty: PackType,
    base: Arc<dyn PackResources>,
}

impl Pack {
    /// Builds a pack from its resources by reading `pack.mcmeta` (spec §3.2). `Err` means the pack is dropped.
    #[allow(clippy::too_many_arguments)]
    pub fn from_resources(
        id: impl Into<String>,
        title: Component,
        source: PackSource,
        known_pack: Option<KnownPack>,
        selection: SelectionConfig,
        resources: Arc<dyn PackResources>,
        ctx: &DiscoveryContext,
    ) -> Result<Self, DropReason> {
        let id = id.into();
        let metadata = read_pack_metadata(&*resources, ctx.ty, &ctx.universe, &*ctx.log)?;
        Ok(Self {
            id,
            title,
            source,
            known_pack,
            metadata,
            selection,
            ty: ctx.ty,
            base: resources,
        })
    }

    /// Builds a pack from already-known metadata (synthetic packs).
    #[allow(clippy::too_many_arguments)]
    pub fn with_metadata(
        id: impl Into<String>,
        title: Component,
        source: PackSource,
        known_pack: Option<KnownPack>,
        selection: SelectionConfig,
        ty: PackType,
        metadata: PackMetadata,
        resources: Arc<dyn PackResources>,
    ) -> Self {
        Self {
            id: id.into(),
            title,
            source,
            known_pack,
            metadata,
            selection,
            ty,
            base: resources,
        }
    }

    /// Features the pack requests.
    pub fn requested_features(&self) -> &FeatureFlagSet {
        &self.metadata.requested_features
    }

    /// Compatibility class.
    pub fn compatibility(&self) -> Compatibility {
        self.metadata.compatibility
    }

    /// Whether the pack is compatible with the running game.
    pub fn is_compatible(&self) -> bool {
        self.metadata.compatibility.is_compatible()
    }

    /// The pack's base content (no overlays).
    pub fn base_resources(&self) -> &Arc<dyn PackResources> {
        &self.base
    }

    /// Opens the pack's content: base plus the active overlays in listed order. An overlay the resources refuse
    /// (security deviation #1) or cannot open contributes nothing.
    pub fn open(&self) -> Arc<dyn PackResources> {
        if self.metadata.overlays.is_empty() {
            return Arc::clone(&self.base);
        }
        let overlays = self
            .metadata
            .overlays
            .iter()
            .filter_map(|dir| self.base.overlay(dir))
            .collect();
        Arc::new(LayeredPackResources::new(Arc::clone(&self.base), overlays))
    }

    /// The pack's filter section (decoded at discovery).
    pub fn filter(&self) -> &FilterSection {
        &self.metadata.filter
    }
}

/// Everything discovery needs besides the file system.
#[derive(Debug, Clone)]
pub struct DiscoveryContext {
    /// Pack type being discovered.
    pub ty: PackType,
    /// The feature-flag universe used to read `features`.
    pub universe: Arc<FeatureUniverse>,
    /// The game version used in known-pack info (`26.3`).
    pub game_version: String,
    /// Diagnostics.
    pub log: SharedLog,
}

impl DiscoveryContext {
    /// A context for `ty` with the built-in `main` universe and version `26.3`.
    pub fn new(ty: PackType, log: SharedLog) -> Self {
        Self {
            ty,
            universe: Arc::new(FeatureUniverse::builtin_main()),
            game_version: "26.3".to_owned(),
            log,
        }
    }

    fn known(&self, id: &str) -> KnownPack {
        KnownPack::new("minecraft", id, self.game_version.clone())
    }
}

// ----------------------------------------------------------------------------------------------------------
// Java string order
// ----------------------------------------------------------------------------------------------------------

/// Compares two strings the way Java's `String.compareTo` does (UTF-16 code units).
pub fn java_string_cmp(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

// ----------------------------------------------------------------------------------------------------------
// Repository
// ----------------------------------------------------------------------------------------------------------

/// The map of available packs keyed by id, iterating in ascending Java string order (spec §2.1).
#[derive(Debug, Clone, Default)]
pub struct PackRepository {
    packs: Vec<Pack>,
}

impl PackRepository {
    /// Builds a repository from the packs of each source, in source order. If an id repeats, the later source wins.
    pub fn from_sources(sources: impl IntoIterator<Item = Vec<Pack>>) -> Self {
        let mut map: BTreeMap<String, Pack> = BTreeMap::new();
        for pack in sources.into_iter().flatten() {
            map.insert(pack.id.clone(), pack);
        }
        let mut packs: Vec<Pack> = map.into_values().collect();
        packs.sort_by(|a, b| java_string_cmp(&a.id, &b.id));
        Self { packs }
    }

    /// Available packs in "available order".
    pub fn available(&self) -> &[Pack] {
        &self.packs
    }

    /// Available ids in "available order".
    pub fn ids(&self) -> Vec<String> {
        self.packs.iter().map(|p| p.id.clone()).collect()
    }

    /// Looks a pack up by id.
    pub fn get(&self, id: &str) -> Option<&Pack> {
        self.packs.iter().find(|p| p.id == id)
    }

    /// Whether a pack with this id is available.
    pub fn contains(&self, id: &str) -> bool {
        self.get(id).is_some()
    }
}

// ----------------------------------------------------------------------------------------------------------
// Symlink allow-list
// ----------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum AllowRule {
    Prefix(PathBuf),
    Regex(fancy_regex::Regex),
}

/// The world's `allowed_symlinks.txt` (default: nothing is allowed).
///
/// Format: one rule per line; blank lines and lines starting with `#` are ignored. A rule is `[prefix]<path>`,
/// `[regex]<pattern>` or `[glob]<pattern>`; a rule without a type tag is a prefix.
#[derive(Debug, Clone, Default)]
pub struct SymlinkAllowList {
    rules: Vec<AllowRule>,
}

impl SymlinkAllowList {
    /// Nothing allowed.
    pub fn none() -> Self {
        Self::default()
    }

    /// Parses the text of `allowed_symlinks.txt`; unusable rules are logged and ignored.
    pub fn parse(text: &str, log: &SharedLog) -> Self {
        let mut rules = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (kind, body) = match line.strip_prefix('[').and_then(|r| r.split_once(']')) {
                Some((k, b)) => (k, b),
                None => ("prefix", line),
            };
            match kind {
                "prefix" => rules.push(AllowRule::Prefix(PathBuf::from(body))),
                "regex" => match fancy_regex::Regex::new(body) {
                    Ok(r) => rules.push(AllowRule::Regex(r)),
                    Err(e) => log.warn(format!(
                        "Ignoring invalid symlink allow-list regex '{body}': {e}"
                    )),
                },
                "glob" => {
                    let mut re = String::from("^");
                    let mut chars = body.chars().peekable();
                    while let Some(c) = chars.next() {
                        match c {
                            '*' if chars.peek() == Some(&'*') => {
                                chars.next();
                                re.push_str(".*");
                            }
                            '*' => re.push_str("[^/]*"),
                            '?' => re.push_str("[^/]"),
                            c => re.push_str(&fancy_regex::escape(&c.to_string())),
                        }
                    }
                    re.push('$');
                    match fancy_regex::Regex::new(&re) {
                        Ok(r) => rules.push(AllowRule::Regex(r)),
                        Err(e) => log.warn(format!(
                            "Ignoring invalid symlink allow-list glob '{body}': {e}"
                        )),
                    }
                }
                other => log.warn(format!(
                    "Ignoring symlink allow-list rule of unknown type '{other}'"
                )),
            }
        }
        Self { rules }
    }

    /// Whether the (resolved) path is allowed.
    pub fn allows(&self, path: &Path) -> bool {
        let text = path.to_string_lossy();
        self.rules.iter().any(|r| match r {
            AllowRule::Prefix(p) => path.starts_with(p),
            AllowRule::Regex(re) => re.is_match(text.as_ref()).unwrap_or(false),
        })
    }
}

// ----------------------------------------------------------------------------------------------------------
// Folder scanning (spec §2.4)
// ----------------------------------------------------------------------------------------------------------

/// Kind of a pack candidate found in a pack folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateKind {
    /// A directory pack (has a regular `pack.mcmeta`).
    Folder,
    /// A `.zip` file (contents not checked yet).
    Zip,
}

/// One candidate entry of a pack folder.
#[derive(Debug, Clone)]
pub struct PackCandidate {
    /// The entry name (with any `.zip`).
    pub name: String,
    /// The path to read (the symlink target for allowed symlinks).
    pub path: PathBuf,
    /// Folder or zip.
    pub kind: CandidateKind,
}

fn tree_has_forbidden_symlink(dir: &Path, allowed: &SymlinkAllowList) -> bool {
    let Ok(read) = fs::read_dir(dir) else {
        return false;
    };
    for entry in read.flatten() {
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            match fs::canonicalize(&path) {
                Ok(target) if allowed.allows(&target) => {}
                _ => return true,
            }
        } else if meta.is_dir() && tree_has_forbidden_symlink(&path, allowed) {
            return true;
        }
    }
    false
}

/// Scans `dir`'s direct entries with the rules of spec §2.4 (sorted by name for determinism).
pub fn scan_pack_folder(
    dir: &Path,
    allowed: &SymlinkAllowList,
    allow_zip: bool,
    log: &SharedLog,
) -> Vec<PackCandidate> {
    let mut names: Vec<(String, PathBuf)> = match fs::read_dir(dir) {
        Ok(read) => read
            .flatten()
            .filter_map(|e| e.file_name().to_str().map(|n| (n.to_owned(), e.path())))
            .collect(),
        Err(e) => {
            log.warn(format!("Failed to list pack folder {}: {e}", dir.display()));
            return Vec::new();
        }
    };
    names.sort();
    let mut out = Vec::new();
    for (name, path) in names {
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                log.warn(format!("Failed to read properties of '{name}': {e}"));
                continue;
            }
        };
        let (path, meta) = if meta.file_type().is_symlink() {
            let target = match fs::canonicalize(&path) {
                Ok(t) if allowed.allows(&t) => t,
                _ => {
                    log.warn(format!(
                        "Ignoring potential pack entry: '{name}' is a symlink that is not allowed"
                    ));
                    continue;
                }
            };
            match fs::metadata(&target) {
                Ok(m) => (target, m),
                Err(e) => {
                    log.warn(format!("Failed to read properties of '{name}': {e}"));
                    continue;
                }
            }
        } else {
            (path, meta)
        };
        if meta.is_dir() {
            if tree_has_forbidden_symlink(&path, allowed) {
                log.warn(format!(
                    "Ignoring pack '{name}': it contains a symlink that is not allowed"
                ));
                continue;
            }
            if path.join(PACK_META).is_file() {
                out.push(PackCandidate {
                    name,
                    path,
                    kind: CandidateKind::Folder,
                });
            } else {
                log.info(format!("Found non-pack entry '{name}', ignoring"));
            }
        } else if meta.is_file() && name.ends_with(".zip") {
            if allow_zip {
                out.push(PackCandidate {
                    name,
                    path,
                    kind: CandidateKind::Zip,
                });
            } else {
                log.warn(format!(
                    "Refusing archive '{name}': archives are only opened from the file system"
                ));
            }
        } else {
            log.info(format!("Found non-pack entry '{name}', ignoring"));
        }
    }
    out
}

fn candidate_resources(c: &PackCandidate, id: &str, log: &SharedLog) -> Arc<dyn PackResources> {
    match c.kind {
        CandidateKind::Folder => Arc::new(FolderPackResources::new(id, &c.path, Arc::clone(log))),
        CandidateKind::Zip => Arc::new(ZipPackResources::open(id, &c.path, Arc::clone(log))),
    }
}

/// Discovers the packs of a folder (`<world>/datapacks` or the client `resourcepacks`): ids `file/<entry name>`,
/// title = the entry name, no known-pack info, not required, TOP, not fixed.
pub fn discover_folder_packs(
    dir: &Path,
    source: PackSource,
    allowed: &SymlinkAllowList,
    ctx: &DiscoveryContext,
) -> Vec<Pack> {
    scan_pack_folder(dir, allowed, true, &ctx.log)
        .into_iter()
        .filter_map(|c| {
            let id = format!("file/{}", c.name);
            let res = candidate_resources(&c, &id, &ctx.log);
            match Pack::from_resources(
                id.clone(),
                Component::literal(c.name.clone()),
                source,
                None,
                SelectionConfig::OPTIONAL_TOP,
                res,
                ctx,
            ) {
                Ok(p) => Some(p),
                Err(reason) => {
                    ctx.log.warn(format!("Dropping pack {id}: {reason}"));
                    None
                }
            }
        })
        .collect()
}

/// The world `datapacks/` source (spec §2.4): creates `<world>/datapacks` if missing, then scans it.
pub fn discover_world_packs(
    world_dir: &Path,
    allowed: &SymlinkAllowList,
    ctx: &DiscoveryContext,
) -> Vec<Pack> {
    let dir = world_dir.join("datapacks");
    let real = fs::canonicalize(&dir).unwrap_or(dir);
    if let Err(e) = fs::create_dir_all(&real) {
        ctx.log
            .warn(format!("Failed to create {}: {e}", real.display()));
    }
    discover_folder_packs(&real, PackSource::World, allowed, ctx)
}

// ----------------------------------------------------------------------------------------------------------
// Synthetic and built-in packs
// ----------------------------------------------------------------------------------------------------------

/// Exposes only the listed namespaces of another pack (the jar holds more than the game exposes).
#[derive(Debug, Clone)]
pub struct RestrictedNamespaces {
    inner: Arc<dyn PackResources>,
    allowed: BTreeSet<String>,
}

impl RestrictedNamespaces {
    /// Restricts `inner` to `allowed` namespaces.
    pub fn new(
        inner: Arc<dyn PackResources>,
        allowed: impl IntoIterator<Item = &'static str>,
    ) -> Self {
        Self {
            inner,
            allowed: allowed.into_iter().map(str::to_owned).collect(),
        }
    }
}

impl PackResources for RestrictedNamespaces {
    fn pack_id(&self) -> &str {
        self.inner.pack_id()
    }
    fn root_resource(&self, name: &str) -> Option<Vec<u8>> {
        self.inner.root_resource(name)
    }
    fn get(&self, ty: PackType, id: &Identifier) -> Option<Vec<u8>> {
        self.allowed
            .contains(id.namespace())
            .then(|| self.inner.get(ty, id))
            .flatten()
    }
    fn has(&self, ty: PackType, id: &Identifier) -> bool {
        self.allowed.contains(id.namespace()) && self.inner.has(ty, id)
    }
    fn namespaces(&self, ty: PackType) -> BTreeSet<String> {
        self.inner
            .namespaces(ty)
            .into_iter()
            .filter(|n| self.allowed.contains(n))
            .collect()
    }
    fn list(
        &self,
        ty: PackType,
        namespace: &str,
        dir: &str,
    ) -> Result<Vec<Identifier>, ResourceError> {
        if self.allowed.contains(namespace) {
            self.inner.list(ty, namespace, dir)
        } else {
            Ok(Vec::new())
        }
    }
    fn overlay(&self, directory: &str) -> Option<Arc<dyn PackResources>> {
        self.inner.overlay(directory).map(|o| {
            Arc::new(Self {
                inner: o,
                allowed: self.allowed.clone(),
            }) as Arc<dyn PackResources>
        })
    }
}

fn synthetic_metadata(
    resources: &dyn PackResources,
    ty: PackType,
    description_key: &str,
    synthetic_range: FormatRange,
    synthetic_features: FeatureFlagSet,
    ctx: &DiscoveryContext,
) -> PackMetadata {
    // A root pack.mcmeta, if any, is preferred section by section; failing or absent sections fall back.
    let file = McmetaFile::read(resources);
    let pack = file
        .section("pack")
        .and_then(|j| decode_pack_section(j, ty).demand().ok());
    let (description, range) = match pack {
        Some(p) => (p.description, p.range),
        None => (Component::translatable(description_key), synthetic_range),
    };
    let requested_features = match (ty, file.section("features")) {
        (PackType::Data, Some(j)) => decode_features_section(j, &ctx.universe)
            .demand()
            .unwrap_or(synthetic_features),
        _ => synthetic_features,
    };
    let overlays = file
        .section("overlays")
        .and_then(|j| decode_overlay_entries(j).demand().ok())
        .and_then(|entries| validate_overlays(ty, &entries, &*ctx.log).ok())
        .map(|valid| {
            valid
                .into_iter()
                .filter(|o| o.range.contains(ty.current_format()))
                .map(|o| o.directory)
                .collect()
        })
        .unwrap_or_default();
    PackMetadata {
        description,
        compatibility: range.classify(ty.current_format()),
        range,
        requested_features,
        overlays,
        filter: decode_filter_section(&file),
    }
}

/// The built-in `vanilla` data pack: synthetic metadata (description `dataPack.vanilla.description`, range
/// `[121.0, 121.*]`, requested features `{minecraft:vanilla}`), namespace `minecraft` only, known pack
/// `minecraft:core:<version>`, not required, default BOTTOM.
pub fn vanilla_data_pack(content: Arc<dyn PackResources>, ctx: &DiscoveryContext) -> Pack {
    let resources: Arc<dyn PackResources> =
        Arc::new(RestrictedNamespaces::new(content, ["minecraft"]));
    let ty = PackType::Data;
    let current = ty.current_format();
    let vanilla = ctx
        .universe
        .vanilla_set()
        .unwrap_or_else(FeatureFlagSet::empty);
    let metadata = synthetic_metadata(
        &*resources,
        ty,
        "dataPack.vanilla.description",
        FormatRange::new(current, PackFormat::wildcard(current.major)),
        vanilla,
        ctx,
    );
    Pack::with_metadata(
        "vanilla",
        Component::literal("vanilla"),
        PackSource::BuiltIn,
        Some(ctx.known("core")),
        SelectionConfig::OPTIONAL_BOTTOM,
        ty,
        metadata,
        resources,
    )
}

/// The client `vanilla` resource pack: namespaces `minecraft` and `realms`, synthetic range `[97.1, 97.*]`, no
/// features, **required**, default BOTTOM.
pub fn vanilla_resource_pack(content: Arc<dyn PackResources>, ctx: &DiscoveryContext) -> Pack {
    let resources: Arc<dyn PackResources> =
        Arc::new(RestrictedNamespaces::new(content, ["minecraft", "realms"]));
    let ty = PackType::Resource;
    let current = ty.current_format();
    let metadata = synthetic_metadata(
        &*resources,
        ty,
        "resourcePack.vanilla.description",
        FormatRange::new(current, PackFormat::wildcard(current.major)),
        FeatureFlagSet::empty(),
        ctx,
    );
    Pack::with_metadata(
        "vanilla",
        Component::literal("vanilla"),
        PackSource::BuiltIn,
        Some(ctx.known("core")),
        SelectionConfig::REQUIRED_BOTTOM,
        ty,
        metadata,
        resources,
    )
}

/// A built-in experimental data pack (source **feature**): title = the literal id, known pack
/// `minecraft:<id>:<version>`, not required, TOP, not fixed.
pub fn feature_pack(
    id: &str,
    resources: Arc<dyn PackResources>,
    ctx: &DiscoveryContext,
) -> Result<Pack, DropReason> {
    Pack::from_resources(
        id,
        Component::literal(id),
        PackSource::Feature,
        Some(ctx.known(id)),
        SelectionConfig::OPTIONAL_TOP,
        resources,
        ctx,
    )
}

/// A built-in resource pack (such as `high_contrast`): source built-in, known pack `minecraft:<id>:<version>`.
pub fn builtin_resource_pack(
    id: &str,
    title: Component,
    resources: Arc<dyn PackResources>,
    ctx: &DiscoveryContext,
) -> Result<Pack, DropReason> {
    Pack::from_resources(
        id,
        title,
        PackSource::BuiltIn,
        Some(ctx.known(id)),
        SelectionConfig::OPTIONAL_TOP,
        resources,
        ctx,
    )
}

/// A server-downloaded resource pack: id `server/<8-digit uppercase hex serial>/<id>`, **required, TOP, fixed**.
pub fn server_resource_pack(
    serial: u32,
    id: &str,
    title: Component,
    resources: Arc<dyn PackResources>,
    ctx: &DiscoveryContext,
) -> Result<Pack, DropReason> {
    Pack::from_resources(
        format!("server/{serial:08X}/{id}"),
        title,
        PackSource::Server,
        None,
        SelectionConfig::REQUIRED_FIXED_TOP,
        resources,
        ctx,
    )
}

/// The directory (inside the vanilla content) holding the feature packs.
pub const FEATURE_PACK_DIR: &str = "data/minecraft/datapacks/";

/// Finds the feature packs inside the vanilla jar (a zip): directory children of `data/minecraft/datapacks/` that
/// carry a `pack.mcmeta`. A `.zip` found there is refused.
pub fn discover_feature_packs_in_zip(jar: &ZipPackResources, ctx: &DiscoveryContext) -> Vec<Pack> {
    let mut out = Vec::new();
    for (name, is_dir) in jar.children(FEATURE_PACK_DIR) {
        if !is_dir {
            if name.ends_with(".zip") {
                ctx.log.warn(format!(
                    "Refusing archive '{name}' found inside the game jar: archives are only opened from the file system"
                ));
            }
            continue;
        }
        let res = jar
            .with_prefix(format!("{FEATURE_PACK_DIR}{name}/"))
            .with_id(name.clone());
        if res.root_resource(PACK_META).is_none() {
            ctx.log
                .info(format!("Found non-pack entry '{name}', ignoring"));
            continue;
        }
        match feature_pack(&name, Arc::new(res), ctx) {
            Ok(p) => out.push(p),
            Err(reason) => ctx.log.warn(format!("Dropping pack {name}: {reason}")),
        }
    }
    out
}

/// Finds feature packs in an unpacked directory with the same entry rules as the world folder (`.zip` refused;
/// ids drop a trailing `.zip`).
pub fn discover_feature_packs_in_dir(dir: &Path, ctx: &DiscoveryContext) -> Vec<Pack> {
    scan_pack_folder(dir, &SymlinkAllowList::none(), false, &ctx.log)
        .into_iter()
        .filter_map(|c| {
            let id = c.name.strip_suffix(".zip").unwrap_or(&c.name).to_owned();
            let res = candidate_resources(&c, &id, &ctx.log);
            match feature_pack(&id, res, ctx) {
                Ok(p) => Some(p),
                Err(reason) => {
                    ctx.log.warn(format!("Dropping pack {id}: {reason}"));
                    None
                }
            }
        })
        .collect()
}
