//! Build the game's Java classpath from the launcher's own library metadata.
//!
//! Only platform-independent library jars are included (natives are skipped): the data generator and the headless
//! server need no native code. Every path points into the user's own launcher folder; nothing is downloaded.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

use crate::discover::{GameInstall, LauncherKind};

/// Maven coordinates `group:artifact:version` → relative jar path. Coordinates with a classifier return `None`.
pub fn maven_path(name: &str) -> Option<PathBuf> {
    let parts: Vec<&str> = name.split(':').collect();
    let [group, artifact, version] = parts.as_slice() else {
        return None;
    };
    Some(
        PathBuf::from(group.replace('.', "/"))
            .join(artifact)
            .join(version)
            .join(format!("{artifact}-{version}.jar")),
    )
}

fn library_names(json: &serde_json::Value) -> Vec<String> {
    json["libraries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| l["name"].as_str().map(str::to_owned))
        .collect()
}

fn read_json(path: &Path) -> anyhow::Result<serde_json::Value> {
    serde_json::from_slice(&fs::read(path).with_context(|| format!("read {}", path.display()))?)
        .with_context(|| format!("parse {}", path.display()))
}

/// Library coordinates the launcher lists for this version (including the LWJGL component on Prism/MultiMC).
fn coordinates(install: &GameInstall) -> anyhow::Result<Vec<String>> {
    match install.launcher {
        LauncherKind::Prism | LauncherKind::MultiMc => {
            let meta = read_json(
                &install
                    .root
                    .join(format!("meta/net.minecraft/{}.json", install.version)),
            )?;
            let mut names = library_names(&meta);
            let lwjgl = meta["requires"]
                .as_array()
                .into_iter()
                .flatten()
                .find_map(|r| {
                    (r["uid"] == "org.lwjgl3")
                        .then(|| {
                            r["equals"]
                                .as_str()
                                .or(r["suggests"].as_str())
                                .map(str::to_owned)
                        })
                        .flatten()
                });
            if let Some(v) = lwjgl {
                let path = install.root.join(format!("meta/org.lwjgl3/{v}.json"));
                if path.is_file() {
                    names.extend(library_names(&read_json(&path)?));
                }
            }
            Ok(names)
        }
        LauncherKind::Official => {
            let path = install
                .root
                .join(format!("versions/{0}/{0}.json", install.version));
            Ok(library_names(&read_json(&path)?))
        }
    }
}

/// Classpath for running the game's own Java entry points (libraries first, then the client jar).
pub fn game_classpath(install: &GameInstall) -> anyhow::Result<Vec<PathBuf>> {
    let lib_root = install.root.join("libraries");
    let mut cp: Vec<PathBuf> = Vec::new();
    for name in coordinates(install)? {
        if let Some(rel) = maven_path(&name) {
            let p = lib_root.join(rel);
            if p.is_file() && !cp.contains(&p) {
                cp.push(p);
            }
        }
    }
    if cp.is_empty() {
        bail!(
            "no libraries found for {} in {}; launch it once in your launcher",
            install.version,
            lib_root.display()
        );
    }
    cp.push(install.client_jar.clone());
    Ok(cp)
}

/// Join a classpath with the platform separator.
pub fn join_classpath(cp: &[PathBuf]) -> std::ffi::OsString {
    std::env::join_paths(cp).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::maven_path;
    use std::path::PathBuf;

    #[test]
    fn maven_paths() {
        assert_eq!(
            maven_path("com.google.code.gson:gson:2.14.0"),
            Some(PathBuf::from(
                "com/google/code/gson/gson/2.14.0/gson-2.14.0.jar"
            ))
        );
        assert_eq!(maven_path("org.lwjgl:lwjgl:3.4.3:natives-linux"), None);
    }
}
