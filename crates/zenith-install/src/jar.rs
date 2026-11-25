//! Facts read from the client jar itself.

use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

/// The fields Zenith needs from the jar's `version.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JarVersionInfo {
    pub id: String,
    pub world_version: i32,
    pub protocol_version: i32,
    #[serde(default)]
    pub java_version: Option<u32>,
    /// The launcher runtime component this version expects, e.g. `java-runtime-epsilon`.
    #[serde(default)]
    pub java_component: Option<String>,
    #[serde(default)]
    pub stable: Option<bool>,
}

/// Read `version.json` out of a client jar.
pub fn read_jar_version(jar: &Path) -> anyhow::Result<JarVersionInfo> {
    let file = File::open(jar).with_context(|| format!("open {}", jar.display()))?;
    let mut zip =
        zip::ZipArchive::new(BufReader::new(file)).context("client jar is not a valid zip")?;
    let mut entry = zip
        .by_name("version.json")
        .context("client jar has no version.json")?;
    let mut text = String::new();
    entry.read_to_string(&mut text)?;
    serde_json::from_str(&text).context("parse version.json")
}

/// Lower-case hex SHA-1 of a file.
pub fn sha1_file(path: &Path) -> io::Result<String> {
    let mut reader = BufReader::with_capacity(1 << 20, File::open(path)?);
    let mut hasher = Sha1::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}
