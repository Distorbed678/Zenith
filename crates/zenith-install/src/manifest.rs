//! Official version facts (hashes and IDs) from Mojang's public manifest.
//!
//! Order of preference: live manifest → cached copy → built-in table. The launcher's own JSON files are never
//! trusted for hashes, because anyone can edit them.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, anyhow, bail};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const KNOWN_VERSIONS: &str = include_str!("../data/known_versions.toml");

/// Where a set of version facts came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FactsSource {
    MojangOnline,
    MojangCached,
    BuiltIn,
}

/// The official hashes and IDs for one release.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionFacts {
    pub id: String,
    pub client_sha1: String,
    pub asset_index_id: String,
    pub asset_index_sha1: String,
    pub source: FactsSource,
}

#[derive(Deserialize)]
struct Known {
    version: Vec<KnownVersion>,
}

#[derive(Deserialize)]
struct KnownVersion {
    id: String,
    client_sha1: String,
    asset_index_id: String,
    asset_index_sha1: String,
}

fn cache_path(id: &str) -> PathBuf {
    crate::cache_dir().join("mojang").join(format!("{id}.json"))
}

fn facts_from_version_json(
    id: &str,
    v: &serde_json::Value,
    source: FactsSource,
) -> anyhow::Result<VersionFacts> {
    let s = |ptr: &str| {
        v.pointer(ptr)
            .and_then(|x| x.as_str())
            .map(str::to_owned)
            .ok_or_else(|| anyhow!("missing {ptr}"))
    };
    Ok(VersionFacts {
        id: id.to_owned(),
        client_sha1: s("/downloads/client/sha1")?,
        asset_index_id: s("/assetIndex/id")?,
        asset_index_sha1: s("/assetIndex/sha1")?,
        source,
    })
}

fn fetch_online(id: &str) -> anyhow::Result<VersionFacts> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("zenith-install")
        .build();
    let manifest: serde_json::Value = agent.get(MANIFEST_URL).call()?.into_json()?;
    let entry = manifest["versions"]
        .as_array()
        .and_then(|vs| vs.iter().find(|v| v["id"] == id))
        .ok_or_else(|| anyhow!("version {id} is not in Mojang's manifest"))?;
    let url = entry["url"].as_str().context("manifest entry has no url")?;
    let expected = entry["sha1"]
        .as_str()
        .context("manifest entry has no sha1")?;
    let body = agent.get(url).call()?.into_string()?;
    let got = hex::encode(Sha1::digest(body.as_bytes()));
    if got != expected {
        bail!("version JSON hash mismatch (expected {expected}, got {got})");
    }
    let path = cache_path(id);
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(&path, &body);
    facts_from_version_json(id, &serde_json::from_str(&body)?, FactsSource::MojangOnline)
}

fn from_cache(id: &str) -> Option<VersionFacts> {
    let v: serde_json::Value = serde_json::from_slice(&fs::read(cache_path(id)).ok()?).ok()?;
    facts_from_version_json(id, &v, FactsSource::MojangCached).ok()
}

fn built_in(id: &str) -> Option<VersionFacts> {
    let known: Known =
        toml::from_str(KNOWN_VERSIONS).expect("built-in known_versions.toml is valid");
    known
        .version
        .into_iter()
        .find(|k| k.id == id)
        .map(|k| VersionFacts {
            id: k.id,
            client_sha1: k.client_sha1,
            asset_index_id: k.asset_index_id,
            asset_index_sha1: k.asset_index_sha1,
            source: FactsSource::BuiltIn,
        })
}

/// Official facts for `id`. With `offline`, the network is never touched.
pub fn version_facts(id: &str, offline: bool) -> anyhow::Result<VersionFacts> {
    let online_err = if offline {
        None
    } else {
        match fetch_online(id) {
            Ok(f) => return Ok(f),
            Err(e) => Some(e),
        }
    };
    if let Some(f) = from_cache(id).or_else(|| built_in(id)) {
        return Ok(f);
    }
    match online_err {
        Some(e) => Err(e.context(format!("no official facts for {id}"))),
        None => bail!("no cached or built-in facts for {id}; run once without --offline"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_table_parses_and_has_26_3() {
        let f = built_in("26.3").expect("26.3 present");
        assert_eq!(f.client_sha1.len(), 40);
        assert_eq!(f.asset_index_id, "34");
    }
}
