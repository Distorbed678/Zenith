//! World directory layout (26.3), coordinates, and identifier path-escape validation.

use std::fmt;
use std::path::{Component, Path, PathBuf};

use crate::error::{Result, StorageError};

/// Global signed 32-bit chunk coordinates `(x, z)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChunkPos {
    /// Global chunk X coordinate.
    pub x: i32,
    /// Global chunk Z coordinate.
    pub z: i32,
}

impl ChunkPos {
    /// Creates a new chunk position.
    #[must_use]
    pub const fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }

    /// Region X coordinate (`x >> 5`, equivalent to `floor(x / 32)`).
    #[must_use]
    pub const fn region_x(self) -> i32 {
        self.x >> 5
    }

    /// Region Z coordinate (`z >> 5`, equivalent to `floor(z / 32)`).
    #[must_use]
    pub const fn region_z(self) -> i32 {
        self.z >> 5
    }

    /// Enclosing region coordinates.
    #[must_use]
    pub const fn region_pos(self) -> RegionPos {
        RegionPos::new(self.region_x(), self.region_z())
    }

    /// Region-local X coordinate (`x & 31`, always in `0..32`).
    #[must_use]
    pub const fn local_x(self) -> u8 {
        (self.x & 31) as u8
    }

    /// Region-local Z coordinate (`z & 31`, always in `0..32`).
    #[must_use]
    pub const fn local_z(self) -> u8 {
        (self.z & 31) as u8
    }

    /// Header slot index in `0..1024` (`lx + lz * 32`).
    #[must_use]
    pub const fn header_index(self) -> usize {
        (self.local_x() as usize) + (self.local_z() as usize) * 32
    }

    /// External oversized-chunk filename: `c.<x>.<z>.mcc` (global coordinates).
    #[must_use]
    pub fn external_filename(self) -> String {
        format!("c.{}.{}.mcc", self.x, self.z)
    }
}

/// Region file coordinates `(rx, rz)`, each covering a 32×32 grid of chunks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RegionPos {
    /// Region X coordinate.
    pub x: i32,
    /// Region Z coordinate.
    pub z: i32,
}

impl RegionPos {
    /// Creates a new region position.
    #[must_use]
    pub const fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }

    /// Region filename: `r.<rx>.<rz>.mca`.
    #[must_use]
    pub fn filename(self) -> String {
        format!("r.{}.{}.mca", self.x, self.z)
    }

    /// Packed 64-bit LRU cache key: `(rx as u32 as u64) | ((rz as u32 as u64) << 32)`.
    #[must_use]
    pub const fn pack_key(self) -> u64 {
        (self.x as u32 as u64) | ((self.z as u32 as u64) << 32)
    }

    /// Unpacks a 64-bit key created by [`RegionPos::pack_key`].
    #[must_use]
    pub const fn unpack_key(key: u64) -> Self {
        Self {
            x: (key & 0xFFFF_FFFF) as u32 as i32,
            z: (key >> 32) as u32 as i32,
        }
    }

    /// Parses a region filename matching `^r\.(-?[0-9]+)\.(-?[0-9]+)\.mca$`.
    #[must_use]
    pub fn parse_filename(name: &str) -> Option<Self> {
        let rest = name.strip_prefix("r.")?.strip_suffix(".mca")?;
        let (rx_str, rz_str) = rest.split_once('.')?;
        if rz_str.contains('.') {
            return None;
        }
        let rx = parse_signed_ascii_int(rx_str)?;
        let rz = parse_signed_ascii_int(rz_str)?;
        Some(Self::new(rx, rz))
    }

    /// Returns the chunk position for region-local coordinates `(lx, lz)` (`0..32`).
    #[must_use]
    pub const fn chunk_pos(self, lx: u8, lz: u8) -> ChunkPos {
        ChunkPos::new((self.x << 5) | (lx as i32), (self.z << 5) | (lz as i32))
    }
}

fn parse_signed_ascii_int(s: &str) -> Option<i32> {
    if s.is_empty() {
        return None;
    }
    let digits = s.strip_prefix('-').unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<i32>().ok()
}

/// A validated `<namespace>:<path>` resource identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Identifier {
    namespace: String,
    path: String,
}

impl Identifier {
    /// Built-in Overworld dimension identifier (`minecraft:overworld`).
    #[must_use]
    pub fn overworld() -> Self {
        Self {
            namespace: "minecraft".to_owned(),
            path: "overworld".to_owned(),
        }
    }

    /// Built-in Nether dimension identifier (`minecraft:the_nether`).
    #[must_use]
    pub fn the_nether() -> Self {
        Self {
            namespace: "minecraft".to_owned(),
            path: "the_nether".to_owned(),
        }
    }

    /// Built-in End dimension identifier (`minecraft:the_end`).
    #[must_use]
    pub fn the_end() -> Self {
        Self {
            namespace: "minecraft".to_owned(),
            path: "the_end".to_owned(),
        }
    }

    /// Constructs and validates an identifier from separate `namespace` and `path`.
    pub fn new(namespace: impl Into<String>, path: impl Into<String>) -> Result<Self> {
        let namespace = namespace.into();
        let path = path.into();
        if !is_valid_namespace(&namespace) || !is_valid_path(&path) {
            return Err(StorageError::InvalidIdentifier(format!(
                "{namespace}:{path}"
            )));
        }
        Ok(Self { namespace, path })
    }

    /// Parses an explicit `<namespace>:<path>` string.
    pub fn parse(input: &str) -> Result<Self> {
        let Some((ns, path)) = input.split_once(':') else {
            return Err(StorageError::InvalidIdentifier(input.to_owned()));
        };
        if !is_valid_namespace(ns) || !is_valid_path(path) {
            return Err(StorageError::InvalidIdentifier(input.to_owned()));
        }
        Ok(Self {
            namespace: ns.to_owned(),
            path: path.to_owned(),
        })
    }

    /// Returns `true` if `input` is syntactically valid `<namespace>:<path>`.
    #[must_use]
    pub fn is_valid(input: &str) -> bool {
        let Some((ns, path)) = input.split_once(':') else {
            return false;
        };
        is_valid_namespace(ns) && is_valid_path(path)
    }

    /// Returns the namespace segment.
    #[must_use]
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Returns the path segment.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
}

impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.namespace, self.path)
    }
}

fn is_valid_namespace(ns: &str) -> bool {
    !ns.is_empty()
        && ns
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.'))
}

fn is_valid_path(path: &str) -> bool {
    !path.is_empty()
        && path
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.' | b'/'))
}

/// Lexically normalizes a path (collapsing `.` and resolving `..` where possible without
/// touching the filesystem).
fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push(comp.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {
                    // Cannot ascend above root; stays at root.
                }
                _ => out.push(comp.as_os_str()),
            },
            Component::Normal(seg) => out.push(seg),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}

/// Resolves `<root>/<namespace>/<path>` (with optional filename suffix such as `.dat`),
/// normalizes both paths, and rejects any result that escapes `<root>` or equals `<root>`.
pub fn resolve_identifier_path(root: &Path, id: &Identifier, suffix: &str) -> Result<PathBuf> {
    if id.path.starts_with('/') || id.namespace.starts_with('/') {
        return Err(StorageError::PathEscape {
            root: normalize_lexical(root),
            resolved: PathBuf::from(&id.path),
        });
    }

    let norm_root = normalize_lexical(root);
    let mut joined = norm_root.clone();
    joined.push(&id.namespace);

    let path_with_suffix = if suffix.is_empty() {
        id.path.clone()
    } else {
        format!("{}{suffix}", id.path)
    };
    joined.push(&path_with_suffix);

    let norm_joined = normalize_lexical(&joined);

    // Ensure normalized target is strictly inside normalized root.
    if !norm_joined.starts_with(&norm_root) || norm_joined == norm_root {
        return Err(StorageError::PathEscape {
            root: norm_root,
            resolved: norm_joined,
        });
    }

    // Also guard against relative-root `..` prefix ambiguities by checking component counts.
    let root_comps: Vec<_> = norm_root.components().collect();
    let joined_comps: Vec<_> = norm_joined.components().collect();
    if joined_comps.len() <= root_comps.len() || joined_comps[..root_comps.len()] != root_comps[..]
    {
        return Err(StorageError::PathEscape {
            root: norm_root,
            resolved: norm_joined,
        });
    }

    Ok(norm_joined)
}

/// Validates that a single filename segment (such as a player UUID) cannot escape its parent.
fn validate_single_segment(root: &Path, segment: &str) -> Result<PathBuf> {
    if segment.is_empty()
        || segment == "."
        || segment == ".."
        || segment.contains('/')
        || segment.contains('\\')
        || segment.contains('\0')
    {
        return Err(StorageError::PathEscape {
            root: normalize_lexical(root),
            resolved: PathBuf::from(segment),
        });
    }
    let norm_root = normalize_lexical(root);
    let resolved = norm_root.join(segment);
    Ok(resolved)
}

/// Identifies one of the three per-dimension Anvil region stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegionStoreKind {
    /// `region/` — terrain, biome, heightmap, light, block-entity, structure, proto-chunk data.
    Terrain,
    /// `entities/` — full-chunk entity store.
    Entities,
    /// `poi/` — point-of-interest section store.
    Poi,
}

impl RegionStoreKind {
    /// Subdirectory name under `<world>/dimensions/<ns>/<path>/`.
    #[must_use]
    pub const fn dir_name(self) -> &'static str {
        match self {
            Self::Terrain => "region",
            Self::Entities => "entities",
            Self::Poi => "poi",
        }
    }
}

/// Directory layout of a 26.3 world save (`<world>/`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldLayout {
    root: PathBuf,
}

impl WorldLayout {
    /// Creates a layout rooted at `world_root`.
    #[must_use]
    pub fn new(world_root: impl Into<PathBuf>) -> Self {
        Self {
            root: world_root.into(),
        }
    }

    /// Root world directory `<world>/`.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `<world>/session.lock`.
    #[must_use]
    pub fn session_lock(&self) -> PathBuf {
        self.root.join("session.lock")
    }

    /// `<world>/level.dat`.
    #[must_use]
    pub fn level_dat(&self) -> PathBuf {
        self.root.join("level.dat")
    }

    /// `<world>/level.dat_old`.
    #[must_use]
    pub fn level_dat_old(&self) -> PathBuf {
        self.root.join("level.dat_old")
    }

    /// `<world>/level.dat_corrupted_<timestamp>`.
    #[must_use]
    pub fn level_dat_corrupted(&self, timestamp: &str) -> PathBuf {
        self.root.join(format!("level.dat_corrupted_{timestamp}"))
    }

    /// `<world>/icon.png`.
    #[must_use]
    pub fn icon_png(&self) -> PathBuf {
        self.root.join("icon.png")
    }

    /// `<world>/data/`.
    #[must_use]
    pub fn data_dir(&self) -> PathBuf {
        self.root.join("data")
    }

    /// `<world>/data/<namespace>/<path>.dat`, rejecting path escapes.
    pub fn data_file(&self, id: &Identifier) -> Result<PathBuf> {
        let base = self.data_dir();
        // First ensure `<ns>/<path>` itself does not escape `<world>/data`.
        let _ = resolve_identifier_path(&base, id, "")?;
        resolve_identifier_path(&base, id, ".dat")
    }

    /// `<world>/players/data/`.
    #[must_use]
    pub fn players_data_dir(&self) -> PathBuf {
        self.root.join("players").join("data")
    }

    /// `<world>/players/data/<uuid>.dat` (lowercase canonical hyphenated UUID).
    pub fn player_dat(&self, uuid: &str) -> Result<PathBuf> {
        validate_single_segment(&self.players_data_dir(), &format!("{uuid}.dat"))
    }

    /// `<world>/players/data/<uuid>.dat_old`.
    pub fn player_dat_old(&self, uuid: &str) -> Result<PathBuf> {
        validate_single_segment(&self.players_data_dir(), &format!("{uuid}.dat_old"))
    }

    /// `<world>/players/data/<uuid>_corrupted_<timestamp>.dat` (note `.dat` at the end).
    pub fn player_dat_corrupted(&self, uuid: &str, timestamp: &str) -> Result<PathBuf> {
        validate_single_segment(
            &self.players_data_dir(),
            &format!("{uuid}_corrupted_{timestamp}.dat"),
        )
    }

    /// `<world>/players/advancements/`.
    #[must_use]
    pub fn players_advancements_dir(&self) -> PathBuf {
        self.root.join("players").join("advancements")
    }

    /// `<world>/players/stats/`.
    #[must_use]
    pub fn players_stats_dir(&self) -> PathBuf {
        self.root.join("players").join("stats")
    }

    /// `<world>/datapacks/`.
    #[must_use]
    pub fn datapacks_dir(&self) -> PathBuf {
        self.root.join("datapacks")
    }

    /// `<world>/generated/`.
    #[must_use]
    pub fn generated_dir(&self) -> PathBuf {
        self.root.join("generated")
    }

    /// `<world>/resourcepacks/resources.zip`.
    #[must_use]
    pub fn bundled_resourcepack(&self) -> PathBuf {
        self.root.join("resourcepacks").join("resources.zip")
    }

    /// `<world>/dimensions/`.
    #[must_use]
    pub fn dimensions_dir(&self) -> PathBuf {
        self.root.join("dimensions")
    }

    /// `<world>/dimensions/<namespace>/<path>/`, rejecting path escapes.
    pub fn dimension_dir(&self, dimension: &Identifier) -> Result<PathBuf> {
        resolve_identifier_path(&self.dimensions_dir(), dimension, "")
    }

    /// Returns a [`DimensionLayout`] for `dimension`, rejecting path escapes.
    pub fn dimension(&self, dimension: &Identifier) -> Result<DimensionLayout> {
        Ok(DimensionLayout::new(self.dimension_dir(dimension)?))
    }
}

/// Directory layout of a single dimension under `<world>/dimensions/<ns>/<path>/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DimensionLayout {
    root: PathBuf,
}

impl DimensionLayout {
    /// Creates a dimension layout rooted at `dimension_root`.
    #[must_use]
    pub fn new(dimension_root: impl Into<PathBuf>) -> Self {
        Self {
            root: dimension_root.into(),
        }
    }

    /// Dimension root directory `<world>/dimensions/<ns>/<path>/`.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `<dimension>/region/`.
    #[must_use]
    pub fn region_dir(&self) -> PathBuf {
        self.root.join(RegionStoreKind::Terrain.dir_name())
    }

    /// `<dimension>/entities/`.
    #[must_use]
    pub fn entities_dir(&self) -> PathBuf {
        self.root.join(RegionStoreKind::Entities.dir_name())
    }

    /// `<dimension>/poi/`.
    #[must_use]
    pub fn poi_dir(&self) -> PathBuf {
        self.root.join(RegionStoreKind::Poi.dir_name())
    }

    /// Subdirectory for the given [`RegionStoreKind`].
    #[must_use]
    pub fn store_dir(&self, kind: RegionStoreKind) -> PathBuf {
        self.root.join(kind.dir_name())
    }

    /// `<dimension>/data/`.
    #[must_use]
    pub fn data_dir(&self) -> PathBuf {
        self.root.join("data")
    }

    /// `<dimension>/data/<namespace>/<path>.dat`, rejecting path escapes.
    pub fn data_file(&self, id: &Identifier) -> Result<PathBuf> {
        let base = self.data_dir();
        let _ = resolve_identifier_path(&base, id, "")?;
        resolve_identifier_path(&base, id, ".dat")
    }
}
