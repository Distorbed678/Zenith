//! Verify that the full asset set for a version is present and intact.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use anyhow::{Context, bail};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::jar::sha1_file;

#[derive(Deserialize)]
struct AssetIndex {
    objects: BTreeMap<String, AssetObject>,
}

#[derive(Deserialize)]
struct AssetObject {
    hash: String,
    size: u64,
}

/// Outcome of an asset verification run.
#[derive(Debug, Clone, Serialize)]
pub struct AssetReport {
    pub index_id: String,
    pub objects: usize,
    pub unique_blobs: usize,
    pub total_bytes: u64,
    pub missing: usize,
    pub corrupt: usize,
    /// Up to ten example problems, for the error message.
    pub examples: Vec<String>,
}

impl AssetReport {
    pub fn ok(&self) -> bool {
        self.missing == 0 && self.corrupt == 0
    }
}

/// Check the asset index against Mojang's hash, then hash every object it lists (in parallel).
pub fn verify_assets(
    assets_dir: &Path,
    index_id: &str,
    expected_index_sha1: &str,
) -> anyhow::Result<AssetReport> {
    let index_path = assets_dir.join("indexes").join(format!("{index_id}.json"));
    if !index_path.is_file() {
        bail!(
            "asset index {} is missing; launch this version once in your launcher",
            index_path.display()
        );
    }
    let got = sha1_file(&index_path)?;
    if got != expected_index_sha1 {
        bail!(
            "asset index {index_id} does not match Mojang's (expected {expected_index_sha1}, found {got}); \
             launch this version once in your launcher to refresh it"
        );
    }
    let index: AssetIndex =
        serde_json::from_slice(&fs::read(&index_path)?).context("parse asset index")?;

    let blobs: BTreeSet<(&str, u64)> = index
        .objects
        .values()
        .map(|o| (o.hash.as_str(), o.size))
        .collect();
    let objects_dir = assets_dir.join("objects");
    let problems: Vec<(bool, String)> = blobs
        .par_iter()
        .filter_map(|&(hash, size)| {
            let Some(prefix) = hash.get(..2) else {
                return Some((false, format!("bad hash {hash}")));
            };
            let path = objects_dir.join(prefix).join(hash);
            match fs::metadata(&path) {
                Err(_) => Some((true, format!("missing {hash}"))),
                Ok(m) if m.len() != size => Some((false, format!("size mismatch {hash}"))),
                Ok(_) => match sha1_file(&path) {
                    Ok(h) if h == hash => None,
                    _ => Some((false, format!("hash mismatch {hash}"))),
                },
            }
        })
        .collect();

    let missing = problems.iter().filter(|(m, _)| *m).count();
    Ok(AssetReport {
        index_id: index_id.to_owned(),
        objects: index.objects.len(),
        unique_blobs: blobs.len(),
        total_bytes: blobs.iter().map(|(_, s)| s).sum(),
        missing,
        corrupt: problems.len() - missing,
        examples: problems.into_iter().take(10).map(|(_, p)| p).collect(),
    })
}
