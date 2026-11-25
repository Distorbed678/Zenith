//! Resource lookup across the pack stack (spec §5): namespace routing, filters, single and stack lookups, both
//! directory-listing modes, and `.mcmeta` sidecar ownership.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use zenith_registry::Identifier;

use crate::filter::PackFilter;
use crate::format::PackType;
use crate::log::{LogExt, SharedLog};
use crate::pack::Pack;
use crate::resources::{PackResources, ResourceError, SIDECAR_SUFFIX};

/// One selected pack as the stack sees it.
#[derive(Debug, Clone)]
pub struct StackPack {
    /// Pack id.
    pub id: String,
    /// The pack's content including its active overlays.
    pub resources: Arc<dyn PackResources>,
    /// The pack's valid filter, if any.
    pub filter: Option<Arc<PackFilter>>,
}

#[derive(Debug, Clone)]
struct Layer {
    pack_id: String,
    /// `None` for a **filter-only** layer (the pack lacks this namespace but its filter is active).
    resources: Option<Arc<dyn PackResources>>,
    filter: Option<Arc<PackFilter>>,
}

impl Layer {
    fn has(&self, ty: PackType, id: &Identifier) -> bool {
        self.resources.as_ref().is_some_and(|r| r.has(ty, id))
    }

    fn blocks(&self, path: &str) -> bool {
        self.filter.as_ref().is_some_and(|f| f.blocks_path(path))
    }
}

/// A located resource (and its sidecar, if any). Contents are read lazily.
#[derive(Debug, Clone)]
pub struct ResourceHandle {
    /// The resource id.
    pub id: Identifier,
    /// The pack the file comes from.
    pub pack_id: String,
    ty: PackType,
    source: Arc<dyn PackResources>,
    sidecar: Option<SidecarHandle>,
}

/// A `.mcmeta` sidecar of a resource.
#[derive(Debug, Clone)]
pub struct SidecarHandle {
    /// The sidecar's id (`<path>.mcmeta`).
    pub id: Identifier,
    /// The pack the sidecar comes from.
    pub pack_id: String,
    source: Arc<dyn PackResources>,
}

impl ResourceHandle {
    /// Reads the file.
    pub fn read(&self) -> Option<Vec<u8>> {
        self.source.get(self.ty, &self.id)
    }

    /// The sidecar, if one applies.
    pub fn sidecar(&self) -> Option<&SidecarHandle> {
        self.sidecar.as_ref()
    }

    /// Reads the sidecar.
    pub fn read_sidecar(&self) -> Option<Vec<u8>> {
        let s = self.sidecar.as_ref()?;
        s.source.get(self.ty, &s.id)
    }

    /// The pack that owns the sidecar, if one applies.
    pub fn sidecar_pack(&self) -> Option<&str> {
        self.sidecar.as_ref().map(|s| s.pack_id.as_str())
    }
}

/// The stack of selected packs, indexed per namespace.
#[derive(Debug, Clone)]
pub struct ResourceStack {
    ty: PackType,
    namespaces: BTreeMap<String, Vec<Layer>>,
    log: SharedLog,
}

fn sidecar_id(id: &Identifier) -> Option<Identifier> {
    Identifier::from_parts(id.namespace(), format!("{}{SIDECAR_SUFFIX}", id.path())).ok()
}

impl ResourceStack {
    /// Builds the stack from packs in selection order (index 0 = lowest priority), spec §5.3.
    pub fn new(ty: PackType, packs: Vec<StackPack>, log: SharedLog) -> Self {
        let pack_namespaces: Vec<BTreeSet<String>> =
            packs.iter().map(|p| p.resources.namespaces(ty)).collect();
        let all: BTreeSet<&String> = pack_namespaces.iter().flatten().collect();
        let mut namespaces = BTreeMap::new();
        for ns in all {
            let mut layers = Vec::new();
            for (pack, provided) in packs.iter().zip(&pack_namespaces) {
                let has = provided.contains(ns);
                let active = pack.filter.as_ref().is_some_and(|f| f.namespace_active(ns));
                match (has, active) {
                    (true, true) => layers.push(Layer {
                        pack_id: pack.id.clone(),
                        resources: Some(Arc::clone(&pack.resources)),
                        filter: pack.filter.clone(),
                    }),
                    (true, false) => layers.push(Layer {
                        pack_id: pack.id.clone(),
                        resources: Some(Arc::clone(&pack.resources)),
                        filter: None,
                    }),
                    (false, true) => layers.push(Layer {
                        pack_id: pack.id.clone(),
                        resources: None,
                        filter: pack.filter.clone(),
                    }),
                    (false, false) => {}
                }
            }
            namespaces.insert(ns.clone(), layers);
        }
        Self {
            ty,
            namespaces,
            log,
        }
    }

    /// Builds the stack from discovered packs (opening their overlays). An invalid `filter` section is logged as an
    /// error and ignored.
    pub fn from_packs(ty: PackType, packs: &[&Pack], log: SharedLog) -> Self {
        let stack_packs = packs
            .iter()
            .map(|p| {
                if let crate::meta::FilterSection::Invalid(msg) = p.filter() {
                    log.error(format!(
                        "Failed to decode filter section of pack {}: {msg}; ignoring the filter",
                        p.id
                    ));
                }
                StackPack {
                    id: p.id.clone(),
                    resources: p.open(),
                    filter: p.filter().filter().cloned(),
                }
            })
            .collect();
        Self::new(ty, stack_packs, log)
    }

    /// Every namespace some pack provides.
    pub fn namespaces(&self) -> Vec<&str> {
        self.namespaces.keys().map(String::as_str).collect()
    }

    fn handle(
        &self,
        layer: &Layer,
        id: &Identifier,
        sidecar: Option<SidecarHandle>,
    ) -> ResourceHandle {
        ResourceHandle {
            id: id.clone(),
            pack_id: layer.pack_id.clone(),
            ty: self.ty,
            source: Arc::clone(
                layer
                    .resources
                    .as_ref()
                    .expect("a file layer has resources"),
            ),
            sidecar,
        }
    }

    fn sidecar_handle(&self, layer: &Layer, sid: &Identifier) -> SidecarHandle {
        SidecarHandle {
            id: sid.clone(),
            pack_id: layer.pack_id.clone(),
            source: Arc::clone(
                layer
                    .resources
                    .as_ref()
                    .expect("a sidecar layer has resources"),
            ),
        }
    }

    /// **Single-resource lookup** (spec §5.4): the topmost pack that has the file wins; a layer whose filter blocks
    /// the path ends the search with *not found*.
    pub fn get(&self, id: &Identifier) -> Option<ResourceHandle> {
        let layers = self.namespaces.get(id.namespace())?;
        let sid = sidecar_id(id);
        for (i, layer) in layers.iter().enumerate().rev() {
            if layer.has(self.ty, id) {
                let sidecar = sid.as_ref().and_then(|sid| {
                    for l in layers[i..].iter().rev() {
                        if l.has(self.ty, sid) {
                            return Some(self.sidecar_handle(l, sid));
                        }
                        if l.blocks(sid.path()) {
                            return None;
                        }
                    }
                    None
                });
                return Some(self.handle(layer, id, sidecar));
            }
            if layer.blocks(id.path()) {
                self.log.info(format!(
                    "Resource {id} not found, but was filtered by pack {}",
                    layer.pack_id
                ));
                return None;
            }
        }
        None
    }

    /// **Resource-stack lookup** (spec §5.5): every version of the file, bottom-up (lowest pack first). Collection
    /// stops after the first layer whose filter blocks the path (its own file is kept). Each version uses only its
    /// own pack's sidecar, or none if a higher layer's filter blocked the sidecar path.
    pub fn get_stack(&self, id: &Identifier) -> Vec<ResourceHandle> {
        let Some(layers) = self.namespaces.get(id.namespace()) else {
            return Vec::new();
        };
        let sid = sidecar_id(id);
        let mut sidecar_blocked = false;
        let mut out = Vec::new();
        for layer in layers.iter().rev() {
            if layer.has(self.ty, id) {
                let sidecar = match (&sid, sidecar_blocked) {
                    (Some(sid), false) if layer.has(self.ty, sid) => {
                        Some(self.sidecar_handle(layer, sid))
                    }
                    _ => None,
                };
                out.push(self.handle(layer, id, sidecar));
            }
            if layer.blocks(id.path()) {
                break;
            }
            if let Some(sid) = &sid
                && layer.blocks(sid.path())
            {
                sidecar_blocked = true;
            }
        }
        out.reverse();
        out
    }

    /// **Directory listing, one winner per file** (spec §5.6), sorted by identifier. Names ending in `.mcmeta` are
    /// sidecars and never listed as data; `accept` is tested on the name with `.mcmeta` removed.
    pub fn list(
        &self,
        dir: &str,
        accept: impl Fn(&Identifier) -> bool,
    ) -> Result<Vec<ResourceHandle>, ResourceError> {
        let mut out = Vec::new();
        for (ns, layers) in &self.namespaces {
            let mut files: BTreeMap<String, usize> = BTreeMap::new();
            let mut sidecars: BTreeMap<String, usize> = BTreeMap::new();
            for (i, layer) in layers.iter().enumerate() {
                if let Some(f) = &layer.filter {
                    files.retain(|p, _| !f.blocks_path(p));
                    sidecars.retain(|p, _| !f.blocks_path(&format!("{p}{SIDECAR_SUFFIX}")));
                }
                let Some(res) = &layer.resources else {
                    continue;
                };
                for id in res.list(self.ty, ns, dir)? {
                    let name = id.path();
                    if let Some(base) = name.strip_suffix(SIDECAR_SUFFIX) {
                        if let Ok(base_id) = Identifier::from_parts(ns.as_str(), base)
                            && accept(&base_id)
                        {
                            sidecars.insert(base.to_owned(), i);
                        }
                    } else if accept(&id) {
                        files.insert(name.to_owned(), i);
                    }
                }
            }
            for (path, layer_idx) in files {
                let Ok(id) = Identifier::from_parts(ns.as_str(), path.clone()) else {
                    continue;
                };
                let layer = &layers[layer_idx];
                let sidecar = sidecars
                    .get(&path)
                    .filter(|&&s| s >= layer_idx)
                    .and_then(|&s| {
                        sidecar_id(&id).map(|sid| self.sidecar_handle(&layers[s], &sid))
                    });
                out.push(self.handle(layer, &id, sidecar));
            }
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    /// **Directory listing, stacks** (spec §5.7): every version of every file, lowest pack first, sorted by
    /// identifier. A layer's filter clears the versions and sidecars accumulated below it before it adds its own.
    pub fn list_stacks(
        &self,
        dir: &str,
        accept: impl Fn(&Identifier) -> bool,
    ) -> Result<Vec<(Identifier, Vec<ResourceHandle>)>, ResourceError> {
        let mut out = Vec::new();
        for (ns, layers) in &self.namespaces {
            let mut versions: BTreeMap<String, Vec<usize>> = BTreeMap::new();
            let mut sidecars: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
            for (i, layer) in layers.iter().enumerate() {
                if let Some(f) = &layer.filter {
                    for (p, v) in &mut versions {
                        if f.blocks_path(p) {
                            v.clear();
                        }
                    }
                    for (p, s) in &mut sidecars {
                        if f.blocks_path(&format!("{p}{SIDECAR_SUFFIX}")) {
                            s.clear();
                        }
                    }
                }
                let Some(res) = &layer.resources else {
                    continue;
                };
                for id in res.list(self.ty, ns, dir)? {
                    let name = id.path();
                    if let Some(base) = name.strip_suffix(SIDECAR_SUFFIX) {
                        if let Ok(base_id) = Identifier::from_parts(ns.as_str(), base)
                            && accept(&base_id)
                        {
                            sidecars.entry(base.to_owned()).or_default().insert(i);
                        }
                    } else if accept(&id) {
                        versions.entry(name.to_owned()).or_default().push(i);
                    }
                }
            }
            for (path, idxs) in versions {
                if idxs.is_empty() {
                    continue;
                }
                let Ok(id) = Identifier::from_parts(ns.as_str(), path.clone()) else {
                    continue;
                };
                let handles = idxs
                    .iter()
                    .map(|&i| {
                        let sidecar =
                            sidecars
                                .get(&path)
                                .filter(|s| s.contains(&i))
                                .and_then(|_| {
                                    sidecar_id(&id).map(|sid| self.sidecar_handle(&layers[i], &sid))
                                });
                        self.handle(&layers[i], &id, sidecar)
                    })
                    .collect();
                out.push((id, handles));
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }
}

/// Converts between resource file paths and element ids for one category (spec §5.9): the element id is the file
/// path with the category prefix and the extension removed, so a file named exactly `.json` gives an element with an
/// **empty path** (`ns:`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileToIdConverter {
    prefix: String,
    extension: String,
}

impl FileToIdConverter {
    /// A converter for `prefix` (for example `recipe` or `tags/block`) and `extension` (for example `.json`).
    pub fn new(prefix: impl Into<String>, extension: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
            extension: extension.into(),
        }
    }

    /// The listing directory (no trailing slash).
    pub fn directory(&self) -> &str {
        &self.prefix
    }

    /// Whether a file name has this converter's extension.
    pub fn accepts(&self, id: &Identifier) -> bool {
        id.path().ends_with(&self.extension)
    }

    /// File id → element id. `None` if the file is not under the prefix or lacks the extension.
    pub fn file_to_id(&self, file: &Identifier) -> Option<Identifier> {
        let rest = file.path().strip_prefix(&self.prefix)?.strip_prefix('/')?;
        let element = rest.strip_suffix(&self.extension)?;
        Identifier::from_parts(file.namespace(), element).ok()
    }

    /// Element id → file id.
    pub fn id_to_file(&self, id: &Identifier) -> Option<Identifier> {
        Identifier::from_parts(
            id.namespace(),
            format!("{}/{}{}", self.prefix, id.path(), self.extension),
        )
        .ok()
    }
}
