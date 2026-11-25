//! Access to the files of one pack: folder, zip, in-memory, and the overlay-layered composite (spec §5.1, §5.2).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::fs;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use zenith_registry::{Identifier, is_valid_namespace};

use crate::format::PackType;
use crate::log::{LogExt, LogSink, SharedLog};

/// The pack metadata file name.
pub const PACK_META: &str = "pack.mcmeta";
/// The suffix of sidecar metadata files.
pub const SIDECAR_SUFFIX: &str = ".mcmeta";

/// Errors of directory listings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceError {
    /// A listing directory must not end with `/` (an internal error in the game).
    TrailingSlash(String),
}

impl fmt::Display for ResourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TrailingSlash(d) => write!(f, "listing directory must not end with '/': {d}"),
        }
    }
}

impl std::error::Error for ResourceError {}

/// Read access to the files of one pack (or one pack layer).
pub trait PackResources: Send + Sync + fmt::Debug {
    /// The pack id (for logs).
    fn pack_id(&self) -> &str;

    /// A file at the pack root (such as `pack.mcmeta`).
    fn root_resource(&self, name: &str) -> Option<Vec<u8>>;

    /// The bytes of `ns:path` (`data/<ns>/<path>` or `assets/…`).
    fn get(&self, ty: PackType, id: &Identifier) -> Option<Vec<u8>>;

    /// Whether the file exists.
    fn has(&self, ty: PackType, id: &Identifier) -> bool {
        self.get(ty, id).is_some()
    }

    /// The namespaces the pack provides.
    fn namespaces(&self, ty: PackType) -> BTreeSet<String>;

    /// Every file under `<root>/<namespace>/<dir>/`, as identifiers (`dir` must not end with `/`).
    /// Names that are not valid identifiers (for example uppercase) are logged and skipped.
    fn list(
        &self,
        ty: PackType,
        namespace: &str,
        dir: &str,
    ) -> Result<Vec<Identifier>, ResourceError>;

    /// The resources of an overlay directory, relative to the pack root. `None` refuses the overlay.
    fn overlay(&self, directory: &str) -> Option<Arc<dyn PackResources>>;
}

fn check_dir(dir: &str) -> Result<(), ResourceError> {
    if dir.ends_with('/') {
        Err(ResourceError::TrailingSlash(dir.to_owned()))
    } else {
        Ok(())
    }
}

fn is_path_segment_char(c: char) -> bool {
    matches!(c, '-' | '.' | '_' | 'a'..='z' | '0'..='9')
}

/// Folder-pack segment rule: `[-._a-z0-9]+`, not `.` or `..`, no empty segment.
pub fn is_valid_folder_path(path: &str) -> bool {
    !path.is_empty()
        && path.split('/').all(|seg| {
            !seg.is_empty() && seg != "." && seg != ".." && seg.chars().all(is_path_segment_char)
        })
}

fn make_identifier(
    log: &dyn LogSink,
    pack: &str,
    namespace: &str,
    dir: &str,
    rel: &str,
) -> Option<Identifier> {
    let path = if dir.is_empty() {
        rel.to_owned()
    } else {
        format!("{dir}/{rel}")
    };
    match Identifier::from_parts(namespace, path.clone()) {
        Ok(id) => Some(id),
        Err(_) => {
            log.warn(format!(
                "Invalid path in pack {pack}: {namespace}:{path}, ignoring"
            ));
            None
        }
    }
}

// ----------------------------------------------------------------------------------------------------------
// Folder pack
// ----------------------------------------------------------------------------------------------------------

/// A pack stored as a directory.
#[derive(Debug, Clone)]
pub struct FolderPackResources {
    id: String,
    root: PathBuf,
    log: SharedLog,
}

impl FolderPackResources {
    /// A folder pack rooted at `root`.
    pub fn new(id: impl Into<String>, root: impl Into<PathBuf>, log: SharedLog) -> Self {
        Self {
            id: id.into(),
            root: root.into(),
            log,
        }
    }

    /// The pack's root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn ns_dir(&self, ty: PackType, ns: &str) -> PathBuf {
        self.root.join(ty.root()).join(ns)
    }
}

fn walk_regular_files(dir: &Path, rel: &mut Vec<String>, out: &mut Vec<String>) {
    let Ok(read) = fs::read_dir(dir) else { return };
    for entry in read.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if ft.is_symlink() {
            continue; // not followed, not a regular file
        }
        if ft.is_dir() {
            rel.push(name);
            walk_regular_files(&entry.path(), rel, out);
            rel.pop();
        } else if ft.is_file() {
            let mut full = rel.join("/");
            if !full.is_empty() {
                full.push('/');
            }
            full.push_str(&name);
            out.push(full);
        }
    }
}

impl PackResources for FolderPackResources {
    fn pack_id(&self) -> &str {
        &self.id
    }

    fn root_resource(&self, name: &str) -> Option<Vec<u8>> {
        let path = self.root.join(name);
        if path.is_file() {
            fs::read(path).ok()
        } else {
            None
        }
    }

    fn get(&self, ty: PackType, id: &Identifier) -> Option<Vec<u8>> {
        let path = self.file_path(ty, id)?;
        if path.is_file() {
            fs::read(path).ok()
        } else {
            None
        }
    }

    fn has(&self, ty: PackType, id: &Identifier) -> bool {
        self.file_path(ty, id).is_some_and(|p| p.is_file())
    }

    fn namespaces(&self, ty: PackType) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let Ok(read) = fs::read_dir(self.root.join(ty.root())) else {
            return out;
        };
        for entry in read.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !entry.path().is_dir() {
                self.log.warn(format!(
                    "Non-directory entry data/{name} in pack {} is not a namespace, rejecting",
                    self.id
                ));
                continue;
            }
            if is_valid_namespace(&name) {
                out.insert(name);
            } else {
                self.log.warn(format!(
                    "Invalid namespace directory {name} in pack {}, ignoring",
                    self.id
                ));
            }
        }
        out
    }

    fn list(
        &self,
        ty: PackType,
        namespace: &str,
        dir: &str,
    ) -> Result<Vec<Identifier>, ResourceError> {
        check_dir(dir)?;
        if !dir.is_empty()
            && !dir
                .split('/')
                .all(|s| !s.is_empty() && s != "." && s != "..")
        {
            self.log
                .warn(format!("Refusing to list suspicious directory '{dir}'"));
            return Ok(Vec::new());
        }
        let mut base = self.ns_dir(ty, namespace);
        for seg in dir.split('/').filter(|s| !s.is_empty()) {
            base.push(seg);
        }
        let mut rels = Vec::new();
        walk_regular_files(&base, &mut Vec::new(), &mut rels);
        rels.sort();
        Ok(rels
            .iter()
            .filter_map(|rel| make_identifier(&*self.log, &self.id, namespace, dir, rel))
            .collect())
    }

    fn overlay(&self, directory: &str) -> Option<Arc<dyn PackResources>> {
        if directory == ".." {
            // Security deviation #1 (docs/security-deviations.md): vanilla resolves `..` to the pack's parent
            // folder. Zenith refuses the overlay and warns.
            self.log.warn(format!(
                "Pack {} declares the overlay directory '..', which would escape the pack folder: overlay refused \
                 (security deviation #1)",
                self.id
            ));
            return None;
        }
        Some(Arc::new(Self {
            id: self.id.clone(),
            root: self.root.join(directory),
            log: Arc::clone(&self.log),
        }))
    }
}

impl FolderPackResources {
    fn file_path(&self, ty: PackType, id: &Identifier) -> Option<PathBuf> {
        if !is_valid_folder_path(id.path()) {
            self.log.warn(format!(
                "Invalid path {id} requested from pack {}, treating as not found",
                self.id
            ));
            return None;
        }
        let mut p = self.ns_dir(ty, id.namespace());
        for seg in id.path().split('/') {
            p.push(seg);
        }
        Some(p)
    }
}

// ----------------------------------------------------------------------------------------------------------
// Flat name lists (zip entries and in-memory files)
// ----------------------------------------------------------------------------------------------------------

fn flat_namespaces<'a>(
    names: impl Iterator<Item = &'a str>,
    prefix: &str,
    ty: PackType,
    zip_rules: bool,
    log: &dyn LogSink,
    pack: &str,
) -> BTreeSet<String> {
    let root = format!("{prefix}{}/", ty.root());
    let mut out = BTreeSet::new();
    let mut rejected = BTreeSet::new();
    for name in names {
        let Some(rest) = name.strip_prefix(&root) else {
            continue;
        };
        if rest.is_empty() {
            continue;
        }
        let (x, has_more) = match rest.split_once('/') {
            Some((x, _)) => (x, true),
            None => (rest, false),
        };
        if x.is_empty() {
            continue;
        }
        if !zip_rules && !has_more {
            if rejected.insert(x.to_owned()) {
                log.warn(format!(
                    "Non-directory entry {root}{x} in pack {pack} is not a namespace, rejecting"
                ));
            }
            continue;
        }
        if is_valid_namespace(x) {
            out.insert(x.to_owned());
        } else if rejected.insert(x.to_owned()) {
            log.warn(format!("Invalid namespace {x} in pack {pack}, ignoring"));
        }
    }
    out
}

fn flat_list<'a>(
    names: impl Iterator<Item = &'a str>,
    prefix: &str,
    ty: PackType,
    namespace: &str,
    dir: &str,
    log: &dyn LogSink,
    pack: &str,
) -> Vec<Identifier> {
    let mut start = format!("{prefix}{}/{namespace}/", ty.root());
    if !dir.is_empty() {
        start.push_str(dir);
        start.push('/');
    }
    let mut rels: Vec<&str> = names
        .filter(|n| !n.ends_with('/'))
        .filter_map(|n| n.strip_prefix(start.as_str()))
        .filter(|r| !r.is_empty())
        .collect();
    rels.sort_unstable();
    rels.dedup();
    rels.into_iter()
        .filter_map(|rel| make_identifier(log, pack, namespace, dir, rel))
        .collect()
}

// ----------------------------------------------------------------------------------------------------------
// Zip pack
// ----------------------------------------------------------------------------------------------------------

enum ZipSource {
    File(fs::File),
    Memory(Cursor<Vec<u8>>),
}

impl Read for ZipSource {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::File(f) => f.read(buf),
            Self::Memory(c) => c.read(buf),
        }
    }
}

impl Seek for ZipSource {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        match self {
            Self::File(f) => f.seek(pos),
            Self::Memory(c) => c.seek(pos),
        }
    }
}

struct ZipInner {
    /// Every entry name (directories included), in archive order.
    names: Vec<String>,
    /// Non-directory entry name → index (first occurrence wins).
    files: HashMap<String, usize>,
    archive: Mutex<Option<zip::ZipArchive<ZipSource>>>,
}

impl ZipInner {
    fn empty() -> Self {
        Self {
            names: Vec::new(),
            files: HashMap::new(),
            archive: Mutex::new(None),
        }
    }

    fn from_source(source: ZipSource, label: &str, log: &dyn LogSink) -> Self {
        match zip::ZipArchive::new(source) {
            Ok(mut archive) => {
                let mut names = Vec::with_capacity(archive.len());
                let mut files = HashMap::new();
                for i in 0..archive.len() {
                    let Ok(entry) = archive.by_index_raw(i) else {
                        continue;
                    };
                    let name = entry.name().to_owned();
                    if !entry.is_dir() && !name.ends_with('/') {
                        files.entry(name.clone()).or_insert(i);
                    }
                    names.push(name);
                }
                Self {
                    names,
                    files,
                    archive: Mutex::new(Some(archive)),
                }
            }
            Err(e) => {
                log.error(format!(
                    "Failed to open pack {label}: {e}; treating it as an empty pack"
                ));
                Self::empty()
            }
        }
    }

    fn read_index(&self, index: usize) -> Option<Vec<u8>> {
        let mut guard = self.archive.lock().unwrap_or_else(|e| e.into_inner());
        let archive = guard.as_mut()?;
        let mut entry = archive.by_index(index).ok()?;
        let mut out = Vec::new();
        entry.read_to_end(&mut out).ok()?;
        Some(out)
    }
}

/// A pack stored as a `.zip` archive. An archive that cannot be opened logs an error once and then behaves as an
/// **empty pack**.
#[derive(Clone)]
pub struct ZipPackResources {
    id: String,
    inner: Arc<ZipInner>,
    /// Entry-name prefix (empty for the archive root; `<dir>/` for overlays and nested packs).
    prefix: String,
    log: SharedLog,
}

impl fmt::Debug for ZipPackResources {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ZipPackResources")
            .field("id", &self.id)
            .field("prefix", &self.prefix)
            .field("entries", &self.inner.names.len())
            .finish()
    }
}

impl ZipPackResources {
    /// Opens the archive at `path`.
    pub fn open(id: impl Into<String>, path: &Path, log: SharedLog) -> Self {
        let id = id.into();
        let inner = match fs::File::open(path) {
            Ok(f) => ZipInner::from_source(ZipSource::File(f), &id, &*log),
            Err(e) => {
                log.error(format!(
                    "Failed to open pack {id}: {e}; treating it as an empty pack"
                ));
                ZipInner::empty()
            }
        };
        Self {
            id,
            inner: Arc::new(inner),
            prefix: String::new(),
            log,
        }
    }

    /// Opens an archive held in memory.
    pub fn from_bytes(id: impl Into<String>, bytes: Vec<u8>, log: SharedLog) -> Self {
        let id = id.into();
        let inner = ZipInner::from_source(ZipSource::Memory(Cursor::new(bytes)), &id, &*log);
        Self {
            id,
            inner: Arc::new(inner),
            prefix: String::new(),
            log,
        }
    }

    /// The same archive, rooted at `prefix` (for example `data/minecraft/datapacks/<id>/`).
    pub fn with_prefix(&self, prefix: impl Into<String>) -> Self {
        Self {
            id: self.id.clone(),
            inner: Arc::clone(&self.inner),
            prefix: prefix.into(),
            log: Arc::clone(&self.log),
        }
    }

    /// With a different pack id.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    /// The distinct first path segments below `dir_prefix` (which ends with `/`) together with whether that child is
    /// a directory (has entries below it) — used to find the built-in feature packs inside a jar.
    pub fn children(&self, dir_prefix: &str) -> Vec<(String, bool)> {
        let mut seen: BTreeMap<String, bool> = BTreeMap::new();
        for name in &self.inner.names {
            let Some(rest) = name.strip_prefix(dir_prefix) else {
                continue;
            };
            if rest.is_empty() {
                continue;
            }
            match rest.split_once('/') {
                Some((child, _)) if !child.is_empty() => {
                    seen.insert(child.to_owned(), true);
                }
                Some(_) => {}
                None => {
                    seen.entry(rest.to_owned()).or_insert(false);
                }
            }
        }
        seen.into_iter().collect()
    }

    fn entry_name(&self, ty: PackType, id: &Identifier) -> String {
        format!(
            "{}{}/{}/{}",
            self.prefix,
            ty.root(),
            id.namespace(),
            id.path()
        )
    }
}

impl PackResources for ZipPackResources {
    fn pack_id(&self) -> &str {
        &self.id
    }

    fn root_resource(&self, name: &str) -> Option<Vec<u8>> {
        let idx = *self.inner.files.get(&format!("{}{name}", self.prefix))?;
        self.inner.read_index(idx)
    }

    fn get(&self, ty: PackType, id: &Identifier) -> Option<Vec<u8>> {
        let idx = *self.inner.files.get(&self.entry_name(ty, id))?;
        self.inner.read_index(idx)
    }

    fn has(&self, ty: PackType, id: &Identifier) -> bool {
        self.inner.files.contains_key(&self.entry_name(ty, id))
    }

    fn namespaces(&self, ty: PackType) -> BTreeSet<String> {
        flat_namespaces(
            self.inner.names.iter().map(String::as_str),
            &self.prefix,
            ty,
            true,
            &*self.log,
            &self.id,
        )
    }

    fn list(
        &self,
        ty: PackType,
        namespace: &str,
        dir: &str,
    ) -> Result<Vec<Identifier>, ResourceError> {
        check_dir(dir)?;
        Ok(flat_list(
            self.inner.names.iter().map(String::as_str),
            &self.prefix,
            ty,
            namespace,
            dir,
            &*self.log,
            &self.id,
        ))
    }

    fn overlay(&self, directory: &str) -> Option<Arc<dyn PackResources>> {
        // The overlay directory is a literal entry-name prefix: nothing can escape the archive.
        Some(Arc::new(
            self.with_prefix(format!("{}{directory}/", self.prefix)),
        ))
    }
}

// ----------------------------------------------------------------------------------------------------------
// In-memory pack
// ----------------------------------------------------------------------------------------------------------

/// Which path rules an in-memory pack follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathRules {
    /// Like a folder pack: segment validation and directory-only namespaces.
    Folder,
    /// Like a zip pack: literal entry names and file namespaces.
    Zip,
}

/// An in-memory pack for tests and synthetic content.
#[derive(Clone)]
pub struct MemoryPack {
    id: String,
    files: Arc<BTreeMap<String, Vec<u8>>>,
    rules: PathRules,
    log: SharedLog,
}

impl fmt::Debug for MemoryPack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemoryPack")
            .field("id", &self.id)
            .field("rules", &self.rules)
            .field("files", &self.files.len())
            .finish()
    }
}

impl MemoryPack {
    /// A new, empty pack with the given path rules.
    pub fn new(id: impl Into<String>, rules: PathRules, log: SharedLog) -> Self {
        Self {
            id: id.into(),
            files: Arc::new(BTreeMap::new()),
            rules,
            log,
        }
    }

    /// Adds a file by its pack-relative path (for example `data/minecraft/recipe/a.json`).
    #[must_use]
    pub fn with_file(mut self, path: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        Arc::make_mut(&mut self.files).insert(path.into(), bytes.into());
        self
    }

    /// Adds a `pack.mcmeta`.
    #[must_use]
    pub fn with_mcmeta(self, text: &str) -> Self {
        self.with_file(PACK_META, text.as_bytes().to_vec())
    }
}

impl PackResources for MemoryPack {
    fn pack_id(&self) -> &str {
        &self.id
    }

    fn root_resource(&self, name: &str) -> Option<Vec<u8>> {
        self.files.get(name).cloned()
    }

    fn get(&self, ty: PackType, id: &Identifier) -> Option<Vec<u8>> {
        if self.rules == PathRules::Folder && !is_valid_folder_path(id.path()) {
            self.log.warn(format!(
                "Invalid path {id} requested from pack {}, treating as not found",
                self.id
            ));
            return None;
        }
        self.files
            .get(&format!("{}/{}/{}", ty.root(), id.namespace(), id.path()))
            .cloned()
    }

    fn namespaces(&self, ty: PackType) -> BTreeSet<String> {
        flat_namespaces(
            self.files.keys().map(String::as_str),
            "",
            ty,
            self.rules == PathRules::Zip,
            &*self.log,
            &self.id,
        )
    }

    fn list(
        &self,
        ty: PackType,
        namespace: &str,
        dir: &str,
    ) -> Result<Vec<Identifier>, ResourceError> {
        check_dir(dir)?;
        Ok(flat_list(
            self.files.keys().map(String::as_str),
            "",
            ty,
            namespace,
            dir,
            &*self.log,
            &self.id,
        ))
    }

    fn overlay(&self, directory: &str) -> Option<Arc<dyn PackResources>> {
        if directory == ".." && self.rules == PathRules::Folder {
            self.log.warn(format!(
                "Pack {} declares the overlay directory '..', which would escape the pack folder: overlay refused \
                 (security deviation #1)",
                self.id
            ));
            return None;
        }
        let prefix = if directory == "." {
            String::new()
        } else {
            format!("{directory}/")
        };
        let sub: BTreeMap<String, Vec<u8>> = self
            .files
            .iter()
            .filter_map(|(k, v)| k.strip_prefix(&prefix).map(|r| (r.to_owned(), v.clone())))
            .collect();
        Some(Arc::new(Self {
            id: self.id.clone(),
            files: Arc::new(sub),
            rules: self.rules,
            log: Arc::clone(&self.log),
        }))
    }
}

// ----------------------------------------------------------------------------------------------------------
// Overlay composite
// ----------------------------------------------------------------------------------------------------------

/// A pack's base content with its active overlays on top (spec §5.1): later-listed overlays win. Metadata always
/// comes from the base.
#[derive(Debug, Clone)]
pub struct LayeredPackResources {
    base: Arc<dyn PackResources>,
    overlays: Vec<Arc<dyn PackResources>>,
}

impl LayeredPackResources {
    /// Combines `base` with `overlays` (listed order, lowest priority first).
    pub fn new(base: Arc<dyn PackResources>, overlays: Vec<Arc<dyn PackResources>>) -> Self {
        Self { base, overlays }
    }

    fn layers_top_down(&self) -> impl Iterator<Item = &Arc<dyn PackResources>> {
        self.overlays
            .iter()
            .rev()
            .chain(std::iter::once(&self.base))
    }
}

impl PackResources for LayeredPackResources {
    fn pack_id(&self) -> &str {
        self.base.pack_id()
    }

    fn root_resource(&self, name: &str) -> Option<Vec<u8>> {
        self.base.root_resource(name)
    }

    fn get(&self, ty: PackType, id: &Identifier) -> Option<Vec<u8>> {
        self.layers_top_down().find_map(|l| l.get(ty, id))
    }

    fn has(&self, ty: PackType, id: &Identifier) -> bool {
        self.layers_top_down().any(|l| l.has(ty, id))
    }

    fn namespaces(&self, ty: PackType) -> BTreeSet<String> {
        self.layers_top_down()
            .flat_map(|l| l.namespaces(ty))
            .collect()
    }

    fn list(
        &self,
        ty: PackType,
        namespace: &str,
        dir: &str,
    ) -> Result<Vec<Identifier>, ResourceError> {
        let mut all = BTreeSet::new();
        for l in self.layers_top_down() {
            all.extend(l.list(ty, namespace, dir)?);
        }
        Ok(all.into_iter().collect())
    }

    fn overlay(&self, _directory: &str) -> Option<Arc<dyn PackResources>> {
        None
    }
}
