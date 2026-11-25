//! Legal-hygiene guard. Blocks:
//! 1. any file whose SHA-1 matches an entry in the official client jar or the official asset index;
//! 2. compiled or decompiled game code (.class, decompiler banners, game-package source outside tools/oracle);
//! 3. trademark misuse in product names (the game's name combined with "Zenith", game-named crates).
//!
//! A line containing the marker `guard:allow` is skipped by the text rules (use it for docs that *quote* a
//! forbidden form in order to forbid it).

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};
use sha1::{Digest, Sha1};

use crate::repo_root;

/// Blobs smaller than this are ignored by the hash rule (tiny/empty files collide by accident).
const MIN_BLOB: u64 = 64;
/// The only places our own Java tooling may live (parity oracle agent, JDK-only fixture generators).
const JAVA_DIRS: [&str; 2] = ["tools/oracle/", "tools/javamap-fixtures/"];
/// Any single tracked file above this size is rejected (keeps binaries out).
const MAX_FILE: u64 = 5 * 1024 * 1024;

fn game() -> String {
    // Assembled at runtime so this source file doesn't trip its own name rules.
    ["Mine", "craft"].concat()
}

fn git_files(staged: bool) -> anyhow::Result<Vec<PathBuf>> {
    let args: &[&str] = if staged {
        &[
            "diff",
            "--cached",
            "--name-only",
            "--diff-filter=ACMR",
            "-z",
        ]
    } else {
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ]
    };
    let out = Command::new("git")
        .args(args)
        .current_dir(repo_root())
        .output()
        .context("run git")?;
    if !out.status.success() {
        bail!("git failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(out
        .stdout
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| PathBuf::from(String::from_utf8_lossy(s).into_owned()))
        .collect())
}

fn sha1_bytes(b: &[u8]) -> String {
    hex::encode(Sha1::digest(b))
}

fn blocklist_path(version: &str) -> PathBuf {
    zenith_install::cache_dir()
        .join("guard")
        .join(format!("{version}.blocklist"))
}

fn http_bytes(url: &str) -> anyhow::Result<Vec<u8>> {
    let mut v = Vec::new();
    ureq::get(url).call()?.into_reader().read_to_end(&mut v)?;
    Ok(v)
}

/// Build (or load from cache) the set of official file hashes for a version.
fn blocklist(version: &str, fetch: bool) -> anyhow::Result<HashSet<String>> {
    let cache = blocklist_path(version);
    if let Ok(s) = fs::read_to_string(&cache) {
        return Ok(s.lines().map(str::to_owned).collect());
    }
    let facts = zenith_install::manifest::version_facts(version, false)?;
    let (jar_bytes, index_bytes): (Vec<u8>, Vec<u8>) = if fetch {
        let vjson: serde_json::Value = serde_json::from_slice(&fs::read(
            zenith_install::cache_dir()
                .join("mojang")
                .join(format!("{version}.json")),
        )?)?;
        let jar_url = vjson
            .pointer("/downloads/client/url")
            .and_then(|v| v.as_str())
            .context("no client url")?;
        let idx_url = vjson
            .pointer("/assetIndex/url")
            .and_then(|v| v.as_str())
            .context("no asset index url")?;
        (http_bytes(jar_url)?, http_bytes(idx_url)?)
    } else {
        let install = zenith_install::discover_installs(Some(version))
            .into_iter()
            .next()
            .with_context(|| format!("no local install of {version}; use --fetch in CI"))?;
        let idx = install
            .assets_dir
            .join("indexes")
            .join(format!("{}.json", facts.asset_index_id));
        (fs::read(&install.client_jar)?, fs::read(idx)?)
    };
    if sha1_bytes(&jar_bytes) != facts.client_sha1 {
        bail!("client jar hash does not match Mojang's manifest");
    }
    if sha1_bytes(&index_bytes) != facts.asset_index_sha1 {
        bail!("asset index hash does not match Mojang's manifest");
    }

    let mut set = HashSet::new();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(jar_bytes))?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i)?;
        if f.is_dir() || f.size() < MIN_BLOB {
            continue;
        }
        let mut buf = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut buf)?;
        set.insert(sha1_bytes(&buf));
    }
    let index: serde_json::Value = serde_json::from_slice(&index_bytes)?;
    if let Some(objs) = index["objects"].as_object() {
        for o in objs.values() {
            if let Some(h) = o["hash"]
                .as_str()
                .filter(|_| o["size"].as_u64().unwrap_or(0) >= MIN_BLOB)
            {
                set.insert(h.to_owned());
            }
        }
    }
    if let Some(d) = cache.parent() {
        fs::create_dir_all(d)?;
    }
    let mut lines: Vec<&String> = set.iter().collect();
    lines.sort();
    fs::write(
        &cache,
        lines
            .into_iter()
            .map(|s| format!("{s}\n"))
            .collect::<String>(),
    )?;
    Ok(set)
}

fn sha1_file(p: &Path) -> std::io::Result<String> {
    let mut r = BufReader::new(File::open(p)?);
    let mut h = Sha1::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            return Ok(hex::encode(h.finalize()));
        }
        h.update(&buf[..n]);
    }
}

fn is_text(bytes: &[u8]) -> bool {
    !bytes.iter().take(8192).any(|b| *b == 0)
}

/// Text rules for one file. Returns human-readable violations.
fn text_rules(rel: &str, text: &str) -> Vec<String> {
    let mut v = Vec::new();
    let g = game();
    let gl = g.to_lowercase();
    let in_oracle = rel.starts_with("tools/oracle/");
    // Banners that only appear in decompiler output: blocked everywhere.
    let decompiler_banners = [
        "Decompiled with",
        "// $VF:",
        "Decompiled by Procyon",
        "JD-Core",
    ];
    // Bare tool names: docs may name the tools (e.g. the dev-setup guide), code files may not.
    let is_doc = rel.ends_with(".md");
    let tool_names: &[&str] = if is_doc {
        &[]
    } else {
        &["Vineflower", "Fernflower"]
    };
    let product_misuse = [
        format!("{gl} zenith"),
        format!("zenith {gl}"),
        format!("zenith for {gl}"),
        format!("zenith-{gl}"),
        format!("{gl}-zenith"),
        "zenithmc".to_owned(),
        "mczenith".to_owned(),
    ];
    for (n, line) in text.lines().enumerate() {
        if line.contains("guard:allow") {
            continue;
        }
        let lower = line.to_lowercase();
        let at = || format!("{rel}:{}", n + 1);
        for b in decompiler_banners.iter().chain(tool_names) {
            if line.contains(b) {
                v.push(format!(
                    "{}: decompiler banner {b:?}. Decompiled code must never enter the repo",
                    at()
                ));
            }
        }
        if !in_oracle {
            let game_pkgs = [
                format!("package net.{gl}"),
                format!("import net.{gl}"),
                "import com.mojang".to_owned(),
            ];
            if game_pkgs
                .iter()
                .any(|p| line.trim_start().starts_with(p.as_str()))
            {
                v.push(format!(
                    "{}: game-package Java source outside tools/oracle/",
                    at()
                ));
            }
        }
        for m in &product_misuse {
            if lower.contains(m.as_str()) {
                v.push(format!("{}: trademark misuse {m:?} (say \"compatible with {g}: Java Edition\" instead)", at()));
            }
        }
    }
    if rel.ends_with("Cargo.toml") {
        for line in text.lines().filter(|l| l.trim_start().starts_with("name")) {
            let l = line.to_lowercase();
            if l.contains(&gl) || l.contains("\"mc-") || l.contains("-mc\"") {
                v.push(format!(
                    "{rel}: crate name uses the game's trademark: {}",
                    line.trim()
                ));
            }
        }
    }
    v
}

pub fn run(staged: bool, version: &str, fetch: bool, no_hashes: bool) -> anyhow::Result<bool> {
    let root = repo_root();
    let files = git_files(staged)?;
    let block = if no_hashes {
        None
    } else {
        Some(blocklist(version, fetch)?)
    };
    let mut violations = Vec::new();

    for rel in &files {
        let path = root.join(rel);
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        }; // deleted in worktree
        if !meta.is_file() {
            continue;
        }
        let rel_s = rel.to_string_lossy().replace('\\', "/");
        // The rule definitions themselves necessarily contain the patterns they forbid.
        let is_rules_file = rel_s == "xtask/src/guard.rs";
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if ["class", "jar", "mca", "mcr", "jfr"].contains(&ext.as_str()) {
            violations.push(format!("{rel_s}: .{ext} files are never committed"));
        }
        if ext == "java" && !JAVA_DIRS.iter().any(|d| rel_s.starts_with(d)) {
            violations.push(format!(
                "{rel_s}: .java is only allowed in {} (our own tooling)",
                JAVA_DIRS.join(", ")
            ));
        }
        if meta.len() > MAX_FILE {
            violations.push(format!(
                "{rel_s}: {} MiB is too large to commit",
                meta.len() / (1024 * 1024)
            ));
        }
        if let Some(set) = block.as_ref().filter(|_| meta.len() >= MIN_BLOB)
            && set.contains(&sha1_file(&path)?)
        {
            violations.push(format!("{rel_s}: identical to an official game file. Game files are read at runtime, never committed"));
        }
        if meta.len() <= MAX_FILE {
            let bytes = fs::read(&path)?;
            if is_text(&bytes) && !is_rules_file {
                violations.extend(text_rules(&rel_s, &String::from_utf8_lossy(&bytes)));
            }
        }
    }

    if violations.is_empty() {
        println!(
            "guard: {} file(s) clean{}",
            files.len(),
            if no_hashes {
                " (hash rule skipped)"
            } else {
                ""
            }
        );
        return Ok(true);
    }
    eprintln!("guard: {} violation(s):", violations.len());
    for v in &violations {
        eprintln!("  {v}");
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_product_misuse_and_banners() {
        let g = game();
        let bad = format!("Welcome to {g} Zenith\n// Decompiled with something\n");
        let v = text_rules("README.md", &bad);
        assert_eq!(v.len(), 2, "{v:?}");
    }

    #[test]
    fn tool_names_allowed_in_docs_only() {
        let line = format!("Decompile locally with {}.\n", ["Vine", "flower"].concat());
        assert!(text_rules("docs/guide/dev-setup.md", &line).is_empty());
        assert_eq!(text_rules("crates/x/src/lib.rs", &line).len(), 1);
    }

    #[test]
    fn allows_nominative_use_and_marker() {
        let g = game();
        let ok =
            format!("Compatible with {g}: Java Edition.\nNever write \"{g} Zenith\" guard:allow\n");
        assert!(text_rules("README.md", &ok).is_empty());
    }

    #[test]
    fn java_package_rule_only_outside_oracle() {
        let src = format!("import net.{}.server.Foo;\n", game().to_lowercase());
        assert_eq!(text_rules("crates/x/Foo.java", &src).len(), 1);
        assert!(text_rules("tools/oracle/src/Agent.java", &src).is_empty());
    }

    #[test]
    fn crate_names() {
        let toml = format!("[package]\nname = \"{}-core\"\n", game().to_lowercase());
        assert_eq!(text_rules("crates/x/Cargo.toml", &toml).len(), 1);
    }
}
