//! Find a Java runtime the user's launcher already installed.
//!
//! Zenith never needs Java to *play*. It needs one only for one-time data-generator ingestion and the
//! developer-only parity oracle. Players already have a suitable runtime: their launcher downloaded it to run the
//! game.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::discover::{GameInstall, LauncherKind};

/// A usable Java runtime.
#[derive(Debug, Clone, Serialize)]
pub struct JavaRuntime {
    /// The `java` executable.
    pub java: PathBuf,
    /// Feature (major) version, e.g. 25.
    pub major: u32,
    /// Where it was found (for messages).
    pub source: String,
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

/// Parse a `JAVA_VERSION` value ("25.0.1", "21", "1.8.0_402") into its feature version.
pub fn parse_major(version: &str) -> Option<u32> {
    let v = version.trim().trim_matches('"');
    let mut parts = v.split(['.', '_', '-', '+']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

/// Read the `release` file of a runtime home.
fn major_from_release(home: &Path) -> Option<u32> {
    let text = fs::read_to_string(home.join("release")).ok()?;
    text.lines()
        .find_map(|l| l.strip_prefix("JAVA_VERSION="))
        .and_then(parse_major)
}

/// The runtime home inside a launcher runtime folder (plain or macOS bundle layout).
fn runtime_home(dir: &Path) -> Option<PathBuf> {
    [dir.to_path_buf(), dir.join("jre.bundle/Contents/Home")]
        .into_iter()
        .find(|h| h.join("bin").join(exe("java")).is_file())
}

fn from_home(home: &Path, source: String) -> Option<JavaRuntime> {
    let home = runtime_home(home)?;
    Some(JavaRuntime {
        java: home.join("bin").join(exe("java")),
        major: major_from_release(&home)?,
        source,
    })
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    v.sort();
    v
}

/// Every runtime a launcher root provides, tagged with its component name when known.
fn launcher_runtimes(install: &GameInstall) -> Vec<(Option<String>, JavaRuntime)> {
    let mut out = Vec::new();
    match install.launcher {
        LauncherKind::Prism | LauncherKind::MultiMc => {
            for dir in subdirs(&install.root.join("java")) {
                let name = dir.file_name().map(|n| n.to_string_lossy().into_owned());
                if let Some(rt) = from_home(
                    &dir,
                    format!("{} ({})", install.launcher.label(), dir.display()),
                ) {
                    out.push((name, rt));
                }
            }
        }
        LauncherKind::Official => {
            // runtime/<component>/<platform>/<component>/
            for comp in subdirs(&install.root.join("runtime")) {
                let name = comp.file_name().map(|n| n.to_string_lossy().into_owned());
                for platform in subdirs(&comp) {
                    for home in subdirs(&platform) {
                        if let Some(rt) =
                            from_home(&home, format!("Official launcher ({})", home.display()))
                        {
                            out.push((name.clone(), rt));
                        }
                    }
                }
            }
        }
    }
    out
}

fn from_java_home() -> Option<JavaRuntime> {
    let home = PathBuf::from(std::env::var_os("JAVA_HOME")?);
    from_home(&home, format!("JAVA_HOME ({})", home.display()))
}

fn from_path() -> Option<JavaRuntime> {
    let out = Command::new(exe("java"))
        .arg("-XshowSettings:properties")
        .arg("-version")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stderr);
    let get = |key: &str| {
        text.lines()
            .find_map(|l| l.trim().strip_prefix(key))
            .map(|v| v.trim().to_owned())
    };
    let major = parse_major(&get("java.specification.version =")?)?;
    let home = PathBuf::from(get("java.home =")?);
    Some(JavaRuntime {
        java: home.join("bin").join(exe("java")),
        major,
        source: "java on PATH".to_owned(),
    })
}

/// Pick the best runtime for a game version: the launcher's own component first (e.g. `java-runtime-epsilon`), then
/// any launcher runtime with the exact major version, then `JAVA_HOME` / `PATH`, then any newer runtime.
pub fn find_java(
    install: &GameInstall,
    required_major: u32,
    component: Option<&str>,
) -> Option<JavaRuntime> {
    let mut all = launcher_runtimes(install);
    all.extend(from_java_home().map(|r| (None, r)));
    all.extend(from_path().map(|r| (None, r)));
    if let Some(c) = component
        && let Some((_, rt)) = all
            .iter()
            .find(|(n, rt)| n.as_deref() == Some(c) && rt.major >= required_major)
    {
        return Some(rt.clone());
    }
    if let Some((_, rt)) = all.iter().find(|(_, rt)| rt.major == required_major) {
        return Some(rt.clone());
    }
    all.into_iter()
        .map(|(_, rt)| rt)
        .filter(|rt| rt.major >= required_major)
        .min_by_key(|rt| rt.major)
}

#[cfg(test)]
mod tests {
    use super::parse_major;

    #[test]
    fn majors() {
        assert_eq!(parse_major("\"25.0.1\""), Some(25));
        assert_eq!(parse_major("21"), Some(21));
        assert_eq!(parse_major("1.8.0_402"), Some(8));
        assert_eq!(parse_major("17.0.2+8"), Some(17));
        assert_eq!(parse_major("x"), None);
    }
}
