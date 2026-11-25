//! `cargo xtask parity <suite>`: parity suites that run against files vanilla wrote on this machine.
//!
//! The corpus is read at run time from the user's own install and worlds (and the oracle's fresh vanilla worlds).
//! Nothing from the corpus is ever written into the repo; only the pass/total counts are.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::Context;
use flate2::read::{GzDecoder, ZlibDecoder};
use zenith_nbt::{Accounter, read_named, write_named};
use zenith_parity::SuiteResult;

use crate::repo_root;

/// Cap the corpus so a huge personal world can't make the suite take hours. Sampling is deterministic.
const MAX_ITEMS: usize = 60_000;

struct Item {
    label: String,
    payload: Vec<u8>,
}

fn gunzip(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    GzDecoder::new(bytes).read_to_end(&mut out).ok()?;
    Some(out)
}

fn inflate(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    ZlibDecoder::new(bytes).read_to_end(&mut out).ok()?;
    Some(out)
}

/// Structure templates shipped inside the user's jar (gzip NBT written by the game's own tooling).
fn jar_structures(version: &str, out: &mut Vec<Item>) -> anyhow::Result<usize> {
    let Some(install) = zenith_install::discover_installs(Some(version))
        .into_iter()
        .next()
    else {
        return Ok(0);
    };
    let mut zip = zip::ZipArchive::new(fs::File::open(&install.client_jar)?)?;
    let names: Vec<String> = zip
        .file_names()
        .filter(|n| n.starts_with("data/minecraft/structure/") && n.ends_with(".nbt"))
        .map(str::to_owned)
        .collect();
    let mut n = 0;
    for name in names {
        let mut raw = Vec::new();
        zip.by_name(&name)?.read_to_end(&mut raw)?;
        if let Some(payload) = gunzip(&raw) {
            out.push(Item {
                label: format!("jar:{name}"),
                payload,
            });
            n += 1;
        }
    }
    Ok(n)
}

/// Every chunk payload in an Anvil region file (gzip, zlib and uncompressed chunks; others are skipped).
fn region_chunks(path: &Path, out: &mut Vec<Item>, skipped: &mut usize) -> anyhow::Result<()> {
    let data = fs::read(path)?;
    if data.len() < 8192 {
        return Ok(());
    }
    for i in 0..1024 {
        let loc = u32::from_be_bytes(data[i * 4..i * 4 + 4].try_into()?);
        let offset = (loc >> 8) as usize * 4096;
        if loc == 0 || offset + 5 > data.len() {
            continue;
        }
        let len = u32::from_be_bytes(data[offset..offset + 4].try_into()?) as usize;
        let kind = data[offset + 4];
        if len == 0 || offset + 4 + len > data.len() {
            continue;
        }
        let body = &data[offset + 5..offset + 4 + len];
        let payload = match kind {
            1 => gunzip(body),
            2 => inflate(body),
            3 => Some(body.to_vec()),
            _ => None, // LZ4, custom or external (.mcc): covered by the storage milestone (M3)
        };
        match payload {
            Some(payload) => out.push(Item {
                label: format!("{}#{i}", path.display()),
                payload,
            }),
            None => *skipped += 1,
        }
    }
    Ok(())
}

fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, files);
        } else if p.extension().is_some_and(|e| e == "mca" || e == "dat") {
            files.push(p);
        }
    }
}

/// World folders to sample: the oracle's fresh vanilla worlds first, then the user's saves, then extra folders.
fn world_roots(extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let oracle = zenith_install::cache_dir().join("oracle");
    let mut runs: Vec<PathBuf> = fs::read_dir(&oracle)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    runs.sort();
    roots.extend(runs.into_iter().map(|r| r.join("worlds")));
    for install in zenith_install::discover_installs(None) {
        for gd in install.game_dirs {
            roots.push(gd.join("saves"));
        }
    }
    roots.extend(extra.iter().cloned());
    roots.sort();
    roots.dedup();
    roots
}

fn first_difference(a: &[u8], b: &[u8]) -> usize {
    a.iter()
        .zip(b)
        .position(|(x, y)| x != y)
        .unwrap_or(a.len().min(b.len()))
}

/// Gather the corpus: jar structures, then world files, capped at `MAX_ITEMS`.
fn collect_corpus(version: &str, extra: &[PathBuf]) -> anyhow::Result<(Vec<Item>, usize, usize)> {
    let mut items = Vec::new();
    let structures = jar_structures(version, &mut items)?;
    let mut skipped = 0usize;
    let mut world_files = Vec::new();
    for root in world_roots(extra) {
        walk(&root, &mut world_files);
    }
    for f in &world_files {
        if items.len() >= MAX_ITEMS {
            break;
        }
        if f.extension().is_some_and(|e| e == "mca") {
            region_chunks(f, &mut items, &mut skipped)?;
        } else if let Ok(raw) = fs::read(f) {
            let payload = gunzip(&raw).unwrap_or(raw);
            items.push(Item {
                label: f.display().to_string(),
                payload,
            });
        }
    }
    items.truncate(MAX_ITEMS);

    Ok((items, structures, skipped))
}

/// Run the corpus through vanilla's own NBT reader+writer (via the oracle tool) and get its outputs.
fn vanilla_reference(
    version: &str,
    items: &[Item],
    mode: &str,
) -> anyhow::Result<Vec<Result<Vec<u8>, String>>> {
    let (install, java) = crate::oracle::install_and_java(version)?;
    let agent = crate::oracle::build(version)?;
    let mut cp = zenith_install::libraries::game_classpath(&install)?;
    cp.push(agent);
    let dir = zenith_install::cache_dir().join("parity/nbt-roundtrip");
    fs::create_dir_all(&dir)?;
    let (corpus, results) = (dir.join("corpus.bin"), dir.join("results.bin"));
    {
        let mut w = std::io::BufWriter::new(fs::File::create(&corpus)?);
        for it in items {
            w.write_all(&u32::try_from(it.payload.len())?.to_be_bytes())?;
            w.write_all(&it.payload)?;
        }
        w.flush()?;
    }
    let out = std::process::Command::new(&java.java)
        .arg("-Xmx4G")
        .arg("-cp")
        .arg(zenith_install::libraries::join_classpath(&cp))
        .arg("dev.zenith.oracle.NbtRoundtrip")
        .arg(&corpus)
        .arg(&results)
        .arg(mode)
        .output()
        .context("run vanilla NBT round-trip")?;
    anyhow::ensure!(
        out.status.success(),
        "vanilla round-trip failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let raw = fs::read(&results)?;
    // The corpus holds game data: keep it local and short-lived.
    let _ = fs::remove_file(&corpus);
    let _ = fs::remove_file(&results);
    let (mut at, mut parsed) = (0usize, Vec::with_capacity(items.len()));
    while at < raw.len() {
        let status = raw[at];
        let len = u32::from_be_bytes(raw[at + 1..at + 5].try_into()?) as usize;
        let body = raw[at + 5..at + 5 + len].to_vec();
        at += 5 + len;
        parsed.push(if status == 0 {
            Ok(body)
        } else {
            Err(String::from_utf8_lossy(&body).into_owned())
        });
    }
    anyhow::ensure!(
        parsed.len() == items.len(),
        "vanilla returned {} results for {} items",
        parsed.len(),
        items.len()
    );
    Ok(parsed)
}

/// Read every corpus item with zenith-nbt, write it back, and compare with vanilla's own re-save of the same bytes.
pub fn nbt_roundtrip(version: &str, extra: &[PathBuf], write: bool) -> anyhow::Result<bool> {
    let (items, structures, skipped) = collect_corpus(version, extra)?;
    let reference = vanilla_reference(version, &items, "resave")?;
    let (mut passed, mut failures) = (0u64, Vec::new());
    for (item, vanilla) in items.iter().zip(&reference) {
        // Vanilla re-saves a root compound with an empty name, so Zenith's re-save does the same.
        let ours =
            read_named(&mut &item.payload[..], &mut Accounter::unlimited()).and_then(|(_, tag)| {
                let mut again = Vec::with_capacity(item.payload.len());
                write_named(&mut again, "", &tag).map(|()| again)
            });
        match (vanilla, ours) {
            (Ok(v), Ok(z)) if *v == z => passed += 1,
            (Ok(v), Ok(z)) => failures.push(format!(
                "{}: differs from vanilla at offset {}",
                item.label,
                first_difference(&z, v)
            )),
            (Err(_), Err(_)) => passed += 1,
            (Err(msg), Ok(_)) => failures.push(format!(
                "{}: vanilla rejects ({msg}), Zenith accepts",
                item.label
            )),
            (Ok(_), Err(e)) => failures.push(format!(
                "{}: Zenith rejects ({e}), vanilla accepts",
                item.label
            )),
        }
    }
    let total = items.len() as u64;
    println!(
        "nbt-roundtrip {version}: {passed}/{total} identical to vanilla's own re-save ({structures} jar structures, {} world items, {skipped} chunks skipped as non-zlib)",
        items.len() - structures
    );
    for f in failures.iter().take(15) {
        println!("  FAIL {f}");
    }
    if write {
        SuiteResult::new("nbt-roundtrip", version, passed, total)
            .write(&repo_root().join("progress/parity"))?;
    }
    Ok(passed == total)
}

/// `snbt-format`: Zenith's single-line SNBT printer must produce exactly the text vanilla prints for every corpus item.
pub fn snbt_format(version: &str, extra: &[PathBuf], write: bool) -> anyhow::Result<bool> {
    let (items, _, _) = collect_corpus(version, extra)?;
    let reference = vanilla_reference(version, &items, "snbt")?;
    let (mut passed, mut failures) = (0u64, Vec::new());
    for (item, vanilla) in items.iter().zip(&reference) {
        let ours = read_named(&mut &item.payload[..], &mut Accounter::unlimited())
            .and_then(|(_, tag)| zenith_nbt::to_snbt(&tag));
        match (vanilla, ours) {
            (Ok(v), Ok(z)) if v.as_slice() == z.as_bytes() => passed += 1,
            (Ok(v), Ok(z)) => {
                let at = first_difference(z.as_bytes(), v);
                let ctx = |b: &[u8]| {
                    String::from_utf8_lossy(&b[at.saturating_sub(40)..(at + 40).min(b.len())])
                        .into_owned()
                };
                failures.push(format!(
                    "{}: at {at}\n      vanilla: {}\n      zenith:  {}",
                    item.label,
                    ctx(v),
                    ctx(z.as_bytes())
                ));
            }
            (Err(_), Err(_)) => passed += 1,
            (Err(m), Ok(_)) => failures.push(format!("{}: vanilla rejects ({m})", item.label)),
            (Ok(_), Err(e)) => failures.push(format!("{}: Zenith fails ({e})", item.label)),
        }
    }
    let total = items.len() as u64;
    println!("snbt-format {version}: {passed}/{total} identical to vanilla's printed text");
    let show = std::env::var("ZENITH_PARITY_SHOW")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8);
    for f in failures.iter().take(show) {
        println!("  FAIL {f}");
    }
    if write {
        SuiteResult::new("snbt-format", version, passed, total)
            .write(&repo_root().join("progress/parity"))?;
    }
    Ok(passed == total)
}

// ── rng-streams ──────────────────────────────────────────────────────────────

use zenith_math::random::position_hash;
use zenith_math::{LegacyRandom, PositionalRandomFactory, RandomSource, XoroshiroRandom};

const RNG_BOUNDS: [i32; 13] = [
    1,
    2,
    3,
    7,
    10,
    16,
    100,
    255,
    256,
    1000,
    1 << 30,
    i32::MAX,
    1_000_000_007,
];
const RNG_COORDS: [(i32, i32, i32); 8] = [
    (0, 0, 0),
    (1, 2, 3),
    (-1, -64, -1),
    (1000, 64, -1000),
    (30_000_000, 319, -30_000_000),
    (i32::MAX, 0, i32::MIN),
    (-123_456, 7, 654_321),
    (686, -10, 2_147_483),
];
const RNG_NAMES: [&str; 5] = [
    "",
    "minecraft:terrain",
    "octave_-7",
    "é✓🎲",
    "minecraft:aquifer",
];

fn rng_seeds() -> Vec<i64> {
    let mut v = vec![
        0,
        1,
        -1,
        42,
        12345,
        i64::MIN,
        i64::MAX,
        0x5DEE_CE66D,
        -7_046_029_254_386_353_131,
    ];
    let mut s: i64 = 1;
    for _ in 0..100 {
        s = s
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        v.push(s);
    }
    v
}

/// The shared script. Must stay identical to `tools/oracle/.../RngStreams.java`.
fn rng_script<R: RandomSource>(mut r: R, seed: i64, out: &mut Vec<(u64, &'static str)>) {
    let mut w = |v: i64, label: &'static str| out.push((v as u64, label));
    for _ in 0..16 {
        w(r.next_int().into(), "nextInt");
    }
    for _ in 0..2 {
        for b in RNG_BOUNDS {
            w(r.next_int_bounded(b).into(), "nextInt(bound)");
        }
    }
    for _ in 0..8 {
        w(r.next_long(), "nextLong");
    }
    for _ in 0..8 {
        w(i64::from(r.next_boolean()), "nextBoolean");
    }
    for _ in 0..16 {
        w((r.next_float().to_bits() as i32).into(), "nextFloat");
    }
    for _ in 0..16 {
        w(r.next_double().to_bits() as i64, "nextDouble");
    }
    for _ in 0..16 {
        w(r.next_gaussian().to_bits() as i64, "nextGaussian");
    }
    r.set_seed(seed.wrapping_mul(31).wrapping_add(7));
    for _ in 0..4 {
        w(r.next_int().into(), "nextInt after setSeed");
    }
    for _ in 0..2 {
        w(
            r.next_gaussian().to_bits() as i64,
            "nextGaussian after setSeed",
        );
    }
    let mut f = r.fork();
    for _ in 0..8 {
        w(f.next_long(), "fork.nextLong");
    }
    for _ in 0..2 {
        w(r.next_long(), "parent after fork");
    }
    let p = r.fork_positional();
    for (x, y, z) in RNG_COORDS {
        let mut q = p.at(x, y, z);
        for _ in 0..3 {
            w(q.next_long(), "positional.at nextLong");
        }
        w(q.next_double().to_bits() as i64, "positional.at nextDouble");
    }
    for name in RNG_NAMES {
        let mut q = p.from_hash_of(name);
        for _ in 0..3 {
            w(q.next_long(), "positional.fromHashOf nextLong");
        }
    }
    for s in [seed, 0] {
        let mut q = p.from_seed(s);
        for _ in 0..3 {
            w(q.next_long(), "positional.fromSeed nextLong");
        }
    }
    r.consume_count(5);
    for _ in 0..2 {
        w(r.next_long(), "after consumeCount(5)");
    }
}

/// `rng-streams`: zenith-math's random sources must produce the same values as vanilla's for the shared script.
pub fn rng_streams(version: &str, write: bool) -> anyhow::Result<bool> {
    let mut ours: Vec<(u64, &'static str)> = Vec::new();
    let mut tags: Vec<(usize, &'static str, i64)> = Vec::new(); // (start index, kind, seed)
    for (kind, legacy) in [("legacy", true), ("xoroshiro", false)] {
        for seed in rng_seeds() {
            tags.push((ours.len(), kind, seed));
            if legacy {
                rng_script(LegacyRandom::new(seed), seed, &mut ours);
            } else {
                rng_script(XoroshiroRandom::new(seed), seed, &mut ours);
            }
        }
    }
    tags.push((ours.len(), "position-hash", 0));
    for (x, y, z) in RNG_COORDS {
        ours.push((position_hash(x, y, z) as u64, "Mth.getSeed"));
    }

    let (install, java) = crate::oracle::install_and_java(version)?;
    let agent = crate::oracle::build(version)?;
    let mut cp = zenith_install::libraries::game_classpath(&install)?;
    cp.push(agent);
    let dir = zenith_install::cache_dir().join("parity/rng-streams");
    fs::create_dir_all(&dir)?;
    let out_file = dir.join("vanilla.bin");
    let out = std::process::Command::new(&java.java)
        .arg("-cp")
        .arg(zenith_install::libraries::join_classpath(&cp))
        .arg("dev.zenith.oracle.RngStreams")
        .arg(&out_file)
        .output()
        .context("run vanilla rng streams")?;
    anyhow::ensure!(
        out.status.success(),
        "vanilla rng streams failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let raw = fs::read(&out_file)?;
    let vanilla: Vec<u64> = raw
        .chunks_exact(8)
        .map(|c| u64::from_be_bytes(c.try_into().expect("8 bytes")))
        .collect();
    anyhow::ensure!(
        vanilla.len() == ours.len(),
        "value count differs: vanilla {} vs zenith {}",
        vanilla.len(),
        ours.len()
    );

    let mut passed = 0u64;
    let mut failures: Vec<String> = Vec::new();
    for (i, ((z, label), v)) in ours.iter().zip(&vanilla).enumerate() {
        if z == v {
            passed += 1;
        } else if failures.len() < 12 {
            let (_, kind, seed) = tags
                .iter()
                .rev()
                .find(|t| t.0 <= i)
                .copied()
                .unwrap_or((0, "?", 0));
            failures.push(format!(
                "#{i} {kind} seed {seed} {label}: vanilla {v:#018x} vs zenith {z:#018x}"
            ));
        }
    }
    let total = ours.len() as u64;
    println!("rng-streams {version}: {passed}/{total} values identical to vanilla");
    for f in &failures {
        println!("  FAIL {f}");
    }
    if write {
        SuiteResult::new("rng-streams", version, passed, total)
            .write(&repo_root().join("progress/parity"))?;
    }
    Ok(passed == total)
}
