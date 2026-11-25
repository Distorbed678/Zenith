#![allow(dead_code)]

use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use zenith_codec::format::PackType;
use zenith_codec::log::MemoryLog;
use zenith_codec::pack::{DiscoveryContext, Pack, PackSource, SelectionConfig};
use zenith_codec::resources::{MemoryPack, PackResources, PathRules};
use zenith_codec::text::Component;

pub fn new_log() -> Arc<MemoryLog> {
    MemoryLog::shared()
}

pub fn data_ctx(log: &Arc<MemoryLog>) -> DiscoveryContext {
    DiscoveryContext::new(PackType::Data, log.clone())
}

pub fn resource_ctx(log: &Arc<MemoryLog>) -> DiscoveryContext {
    DiscoveryContext::new(PackType::Resource, log.clone())
}

/// A `pack.mcmeta` for the current data format with optional requested feature flags.
pub fn data_mcmeta(features: &[&str]) -> String {
    let feats = if features.is_empty() {
        String::new()
    } else {
        let ids: Vec<String> = features.iter().map(|f| format!("\"{f}\"")).collect();
        format!(",\"features\":{{\"enabled\":[{}]}}", ids.join(","))
    };
    format!("{{\"pack\":{{\"description\":\"d\",\"min_format\":121,\"max_format\":121}}{feats}}}")
}

pub fn mem_pack(id: &str, mcmeta: &str, log: &Arc<MemoryLog>) -> MemoryPack {
    MemoryPack::new(id, PathRules::Folder, log.clone()).with_mcmeta(mcmeta)
}

pub fn make_pack(
    id: &str,
    source: PackSource,
    selection: SelectionConfig,
    res: impl PackResources + 'static,
    ctx: &DiscoveryContext,
) -> Pack {
    Pack::from_resources(
        id,
        Component::literal(id),
        source,
        None,
        selection,
        Arc::new(res),
        ctx,
    )
    .unwrap_or_else(|e| panic!("pack {id} dropped: {e}"))
}

/// A data pack with the given requested features.
pub fn data_pack(id: &str, source: PackSource, features: &[&str], log: &Arc<MemoryLog>) -> Pack {
    let ctx = data_ctx(log);
    make_pack(
        id,
        source,
        SelectionConfig::OPTIONAL_TOP,
        mem_pack(id, &data_mcmeta(features), log),
        &ctx,
    )
}

/// Builds an in-memory zip archive.
pub fn make_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default();
        for (name, data) in entries {
            if name.ends_with('/') {
                w.add_directory(*name, opts).unwrap();
            } else {
                w.start_file(*name, opts).unwrap();
                w.write_all(data).unwrap();
            }
        }
        w.finish().unwrap();
    }
    buf.into_inner()
}

pub fn write_file(root: &Path, rel: &str, data: &[u8]) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, data).unwrap();
}
