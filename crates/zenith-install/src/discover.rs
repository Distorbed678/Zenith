//! Discover game installs made by supported launchers.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// A launcher whose on-disk layout Zenith understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum LauncherKind {
    Prism,
    MultiMc,
    Official,
}

impl LauncherKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Prism => "Prism Launcher",
            Self::MultiMc => "MultiMC",
            Self::Official => "Official launcher",
        }
    }
}

/// One installed game version, as laid out by a launcher.
#[derive(Debug, Clone, Serialize)]
pub struct GameInstall {
    pub launcher: LauncherKind,
    pub root: PathBuf,
    pub version: String,
    pub client_jar: PathBuf,
    pub assets_dir: PathBuf,
    /// The asset index ID the launcher recorded. Informational only: proof uses Mojang's manifest.
    pub launcher_asset_index: Option<String>,
    /// Game directories (instances) that run this version. Launch evidence is collected from these.
    pub game_dirs: Vec<PathBuf>,
}

/// Launcher roots to search, most specific first. `ZENITH_LAUNCHER_ROOT` adds a custom root.
pub fn candidate_roots() -> Vec<(LauncherKind, PathBuf)> {
    let mut roots = Vec::new();
    if let Some(custom) = std::env::var_os("ZENITH_LAUNCHER_ROOT") {
        let p = PathBuf::from(custom);
        let kind = if p.join("instances").is_dir() {
            LauncherKind::Prism
        } else {
            LauncherKind::Official
        };
        roots.push((kind, p));
    }
    if let Some(home) = dirs::home_dir() {
        roots.push((
            LauncherKind::Prism,
            home.join(".var/app/org.prismlauncher.PrismLauncher/data/PrismLauncher"),
        ));
        roots.push((LauncherKind::Official, home.join(".minecraft")));
    }
    if let Some(data) = dirs::data_dir() {
        roots.push((LauncherKind::Prism, data.join("PrismLauncher")));
        roots.push((LauncherKind::MultiMc, data.join("multimc")));
        roots.push((LauncherKind::Official, data.join(".minecraft"))); // Windows: %APPDATA%\.minecraft
        roots.push((LauncherKind::Official, data.join("minecraft"))); // macOS: Application Support/minecraft
    }
    let mut seen = BTreeSet::new();
    roots.retain(|(_, p)| {
        p.is_dir() && seen.insert(fs::canonicalize(p).unwrap_or_else(|_| p.clone()))
    });
    roots
}

/// Every install found on this machine, optionally filtered to one version.
pub fn discover_installs(version: Option<&str>) -> Vec<GameInstall> {
    let mut out = Vec::new();
    for (kind, root) in candidate_roots() {
        let found = match kind {
            LauncherKind::Prism | LauncherKind::MultiMc => scan_prism_like(kind, &root),
            LauncherKind::Official => scan_official(&root),
        };
        out.extend(
            found
                .into_iter()
                .filter(|i| version.is_none_or(|v| i.version == v)),
        );
    }
    out
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    v.sort();
    v
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn read_json(p: &Path) -> Option<serde_json::Value> {
    serde_json::from_slice(&fs::read(p).ok()?).ok()
}

fn asset_index_from_version_json(p: &Path) -> Option<String> {
    read_json(p)?
        .get("assetIndex")?
        .get("id")?
        .as_str()
        .map(str::to_owned)
}

fn scan_prism_like(kind: LauncherKind, root: &Path) -> Vec<GameInstall> {
    // Map version -> instance game dirs, from each instance's component list.
    let mut instances: Vec<(String, PathBuf)> = Vec::new();
    for inst in subdirs(&root.join("instances")) {
        let Some(pack) = read_json(&inst.join("mmc-pack.json")) else {
            continue;
        };
        let mc_version = pack
            .get("components")
            .and_then(|c| c.as_array())
            .into_iter()
            .flatten()
            .find(|c| c.get("uid").and_then(|u| u.as_str()) == Some("net.minecraft"))
            .and_then(|c| c.get("version").and_then(|v| v.as_str()).map(str::to_owned));
        let Some(mc_version) = mc_version else {
            continue;
        };
        for game_dir in [inst.join("minecraft"), inst.join(".minecraft")] {
            if game_dir.is_dir() {
                instances.push((mc_version.clone(), game_dir));
            }
        }
    }

    let mut out = Vec::new();
    for vdir in subdirs(&root.join("libraries/com/mojang/minecraft")) {
        let version = file_name(&vdir);
        let jar = vdir.join(format!("minecraft-{version}-client.jar"));
        if !jar.is_file() {
            continue;
        }
        out.push(GameInstall {
            launcher: kind,
            root: root.to_path_buf(),
            client_jar: jar,
            assets_dir: root.join("assets"),
            launcher_asset_index: asset_index_from_version_json(
                &root.join(format!("meta/net.minecraft/{version}.json")),
            ),
            game_dirs: instances
                .iter()
                .filter(|(v, _)| *v == version)
                .map(|(_, d)| d.clone())
                .collect(),
            version,
        });
    }
    out
}

fn scan_official(root: &Path) -> Vec<GameInstall> {
    let mut out = Vec::new();
    for vdir in subdirs(&root.join("versions")) {
        let version = file_name(&vdir);
        let jar = vdir.join(format!("{version}.jar"));
        if !jar.is_file() {
            continue;
        }
        out.push(GameInstall {
            launcher: LauncherKind::Official,
            root: root.to_path_buf(),
            client_jar: jar,
            assets_dir: root.join("assets"),
            launcher_asset_index: asset_index_from_version_json(
                &vdir.join(format!("{version}.json")),
            ),
            game_dirs: vec![root.to_path_buf()],
            version,
        });
    }
    out
}
