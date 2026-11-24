//! Evidence that a version has really been launched at least once.
//!
//! Strong signals (any one is enough):
//! - `options.txt` carries `version:<world_version>`, which only the game itself writes.
//! - A log (`logs/latest.log` or rotated `*.log.gz`) mentions starting that exact version.
//!
//! Supporting signal: Prism/MultiMC `instance.cfg` has a non-zero `lastLaunchTime`.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Signal {
    OptionsDataVersion { path: PathBuf },
    LogMentionsVersion { path: PathBuf },
    LauncherLastLaunch { epoch_ms: u64 },
}

impl Signal {
    pub fn is_strong(&self) -> bool {
        !matches!(self, Self::LauncherLastLaunch { .. })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Evidence {
    pub game_dir: PathBuf,
    pub signals: Vec<Signal>,
}

impl Evidence {
    pub fn is_sufficient(&self) -> bool {
        self.signals.iter().any(Signal::is_strong)
    }
}

/// True if `text` contains `version` as a whole token (so "26.3" doesn't match "26.3.1" or "126.3").
pub fn mentions_version(text: &str, version: &str) -> bool {
    text.match_indices(version).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + version.len()..].chars().next();
        let boundary = |c: Option<char>| !c.is_some_and(|c| c.is_ascii_digit() || c == '.');
        boundary(before) && boundary(after)
    })
}

fn log_line_matches(line: &str, version: &str) -> bool {
    (line.contains("App version:")
        || line.contains("server version")
        || line.contains("Minecraft "))
        && mentions_version(line, version)
}

fn options_matches(path: &Path, world_version: i32) -> bool {
    fs::read_to_string(path)
        .map(|s| {
            s.lines()
                .any(|l| l.trim() == format!("version:{world_version}"))
        })
        .unwrap_or(false)
}

fn read_log(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    if path.extension().is_some_and(|e| e == "gz") {
        let mut s = String::new();
        flate2::read::GzDecoder::new(&bytes[..])
            .read_to_string(&mut s)
            .ok()?;
        Some(s)
    } else {
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }
}

fn prism_last_launch(game_dir: &Path) -> Option<u64> {
    let cfg = fs::read_to_string(game_dir.parent()?.join("instance.cfg")).ok()?;
    cfg.lines()
        .find_map(|l| l.strip_prefix("lastLaunchTime="))
        .and_then(|v| v.trim().parse().ok())
        .filter(|&v| v > 0)
}

/// Collect evidence from each game directory that runs this version.
pub fn collect(game_dirs: &[PathBuf], version: &str, world_version: i32) -> Vec<Evidence> {
    game_dirs
        .iter()
        .map(|dir| {
            let mut signals = Vec::new();
            let options = dir.join("options.txt");
            if options_matches(&options, world_version) {
                signals.push(Signal::OptionsDataVersion { path: options });
            }
            let mut logs: Vec<PathBuf> = fs::read_dir(dir.join("logs"))
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.to_string_lossy().ends_with(".log")
                        || p.to_string_lossy().ends_with(".log.gz")
                })
                .collect();
            logs.sort();
            logs.reverse();
            if let Some(path) = logs.into_iter().find(|p| {
                read_log(p).is_some_and(|t| t.lines().any(|l| log_line_matches(l, version)))
            }) {
                signals.push(Signal::LogMentionsVersion { path });
            }
            if let Some(epoch_ms) = prism_last_launch(dir) {
                signals.push(Signal::LauncherLastLaunch { epoch_ms });
            }
            Evidence {
                game_dir: dir.clone(),
                signals,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_token_boundaries() {
        assert!(mentions_version("SDL [SYSTEM]: App version: 26.3", "26.3"));
        assert!(mentions_version("server version 26.3)", "26.3"));
        assert!(!mentions_version("App version: 26.3.1", "26.3"));
        assert!(!mentions_version("App version: 126.3", "26.3"));
        assert!(!mentions_version("App version: 26.30", "26.3"));
    }

    #[test]
    fn log_lines() {
        assert!(log_line_matches(
            "[16:37:38] [Render thread/INFO]: SDL [SYSTEM]: App version: 26.3",
            "26.3"
        ));
        assert!(log_line_matches(
            "[Server thread/INFO]: Starting integrated minecraft server version 26.3",
            "26.3"
        ));
        assert!(!log_line_matches("Loaded 26.3 things", "26.3"));
    }
}
