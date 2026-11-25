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

fn hex(b: &[u8]) -> String {
    b.iter()
        .map(|x| format!("{x:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn first_difference(a: &[u8], b: &[u8]) -> usize {
    a.iter()
        .zip(b)
        .position(|(x, y)| x != y)
        .unwrap_or(a.len().min(b.len()))
}

/// Hand-made edge cases that real worlds rarely contain (our own generated data, built at run time). Every case is
/// run through vanilla too, so rejections must match as well as successful re-saves.
fn synthetic_items() -> Vec<Item> {
    fn name(out: &mut Vec<u8>, s: &[u8]) {
        out.extend_from_slice(&(s.len() as u16).to_be_bytes());
        out.extend_from_slice(s);
    }
    fn root(ty: u8, body: &[u8]) -> Vec<u8> {
        let mut v = vec![ty];
        name(&mut v, b"");
        v.extend_from_slice(body);
        v
    }
    fn entry(out: &mut Vec<u8>, ty: u8, key: &[u8], payload: &[u8]) {
        out.push(ty);
        name(out, key);
        out.extend_from_slice(payload);
    }
    fn nested_compounds(depth: usize) -> Vec<u8> {
        // root compound containing `depth - 1` nested compounds under key "n"
        let mut body = Vec::new();
        for _ in 1..depth {
            body.push(10);
            name(&mut body, b"n");
        }
        body.extend(std::iter::repeat_n(0u8, depth));
        root(10, &body)
    }
    let mut items: Vec<(String, Vec<u8>)> = Vec::new();

    // Non-canonical NaNs and negative zero (vanilla writes canonical NaNs).
    let mut c = Vec::new();
    for (k, bits) in [
        (&b"f_neg_nan"[..], 0xFFC0_0000u32),
        (b"f_snan", 0x7F80_0001),
        (b"f_payload", 0x7FC0_0123),
        (b"f_neg_zero", 0x8000_0000),
    ] {
        entry(&mut c, 5, k, &bits.to_be_bytes());
    }
    for (k, bits) in [
        (&b"d_neg_nan"[..], 0xFFF8_0000_0000_0000u64),
        (b"d_snan", 0x7FF0_0000_0000_0001),
        (b"d_neg_zero", 0x8000_0000_0000_0000),
    ] {
        entry(&mut c, 6, k, &bits.to_be_bytes());
    }
    c.push(0);
    items.push(("nan-and-zero".into(), root(10, &c)));

    // Depth boundary around vanilla's limit of 512.
    for d in [510, 511, 512, 513] {
        items.push((format!("depth-{d}"), nested_compounds(d)));
    }

    // List of compounds where one element already has the wrapper shape ({"": value}).
    let mut l = vec![10];
    l.extend_from_slice(&2i32.to_be_bytes());
    entry(&mut l, 3, b"", &7i32.to_be_bytes());
    l.push(0);
    entry(&mut l, 3, b"", &8i32.to_be_bytes());
    entry(&mut l, 1, b"x", &[1]);
    l.push(0);
    let mut c = Vec::new();
    entry(&mut c, 9, b"wrapped", &l);
    // Empty lists declared with a non-End element type, and with End.
    let mut empty_int = vec![3];
    empty_int.extend_from_slice(&0i32.to_be_bytes());
    entry(&mut c, 9, b"empty_int", &empty_int);
    let mut empty_end = vec![0];
    empty_end.extend_from_slice(&0i32.to_be_bytes());
    entry(&mut c, 9, b"empty_end", &empty_end);
    // Duplicate key: the later value wins.
    entry(&mut c, 3, b"dup", &1i32.to_be_bytes());
    entry(&mut c, 3, b"dup", &2i32.to_be_bytes());
    c.push(0);
    items.push(("lists-and-duplicates".into(), root(10, &c)));

    // Strings: NUL (C0 80), an astral character as two 3-byte surrogates, a lone surrogate.
    let mut c = Vec::new();
    for (k, mutf8) in [
        (&b"nul"[..], &[0x61u8, 0xC0, 0x80, 0x62][..]),
        (b"astral", &[0xED, 0xA0, 0xBD, 0xED, 0xB8, 0x80]),
        (b"lone", &[0xED, 0xA0, 0x80]),
    ] {
        let mut v = (mutf8.len() as u16).to_be_bytes().to_vec();
        v.extend_from_slice(mutf8);
        entry(&mut c, 8, k, &v);
    }
    c.push(0);
    items.push(("strings".into(), root(10, &c)));

    // Malformed input that both sides must reject.
    let mut bad = Vec::new();
    entry(&mut bad, 8, b"s", &[0, 4, 0xF0, 0x9F, 0x98, 0x80]); // 4-byte UTF-8 is invalid in modified UTF-8
    bad.push(0);
    items.push(("reject-4byte-utf8".into(), root(10, &bad)));
    let mut bad = vec![0];
    bad.extend_from_slice(&3i32.to_be_bytes());
    let mut c = Vec::new();
    entry(&mut c, 9, b"l", &bad); // End-typed list with elements
    c.push(0);
    items.push(("reject-end-list".into(), root(10, &c)));
    let mut c = Vec::new();
    entry(&mut c, 13, b"t", &[]); // unknown tag type
    c.push(0);
    items.push(("reject-unknown-type".into(), root(10, &c)));
    let mut c = Vec::new();
    entry(&mut c, 11, b"a", &(-1i32).to_be_bytes()); // negative array length
    c.push(0);
    items.push(("reject-negative-length".into(), root(10, &c)));

    // Non-compound roots.
    let mut l = vec![8];
    l.extend_from_slice(&1i32.to_be_bytes());
    l.extend_from_slice(&[0, 2, b'h', b'i']);
    items.push(("root-list".into(), root(9, &l)));
    items.push(("root-string".into(), root(8, &[0, 2, b'o', b'k'])));

    items
        .into_iter()
        .map(|(label, payload)| Item {
            label: format!("synthetic:{label}"),
            payload,
        })
        .collect()
}

/// Gather the corpus: jar structures, then world files, capped at `MAX_ITEMS`, then the synthetic edge cases.
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

    items.extend(synthetic_items());
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
            (Ok(v), Ok(z)) => {
                let detail = if item.label.starts_with("synthetic:") {
                    format!("\n      vanilla: {}\n      zenith:  {}", hex(v), hex(&z))
                } else {
                    String::new()
                };
                failures.push(format!(
                    "{}: differs from vanilla at offset {}{detail}",
                    item.label,
                    first_difference(&z, v)
                ))
            }
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
            .and_then(|(_, tag)| zenith_nbt::to_snbt_utf16(&tag))
            .map(|units| {
                units
                    .iter()
                    .flat_map(|u| u.to_be_bytes())
                    .collect::<Vec<u8>>()
            });
        match (vanilla, ours) {
            (Ok(v), Ok(z)) if *v == z => passed += 1,
            (Ok(v), Ok(z)) => {
                // Both sides are UTF-16BE: show ~40 characters around the first difference, decoded.
                let at = first_difference(&z, v) / 2 * 2;
                let ctx = |b: &[u8]| {
                    let window = &b[at.saturating_sub(80).min(b.len())..(at + 80).min(b.len())];
                    let units: Vec<u16> = window
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|c| u16::from_be_bytes(*c))
                        .collect();
                    String::from_utf16_lossy(&units)
                };
                failures.push(format!(
                    "{}: at UTF-16 unit {}\n      vanilla: {}\n      zenith:  {}",
                    item.label,
                    at / 2,
                    ctx(v),
                    ctx(&z)
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
        .as_chunks::<8>()
        .0
        .iter()
        .map(|c| u64::from_be_bytes(*c))
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

// ── noise-samples ────────────────────────────────────────────────────────────

use zenith_math::noise::{
    NormalNoise, NormalNoiseParameters, PerlinNoise, SimplexNoise, SmearedPerlinNoise,
    build_legacy_fbm, build_smeared_fbm,
};

const NOISE_SEEDS: [i64; 5] = [0, 1, 42, -7_046_029_254_386_353_131, 1_234_567_890_123];
const NOISE_SMEARS: [f64; 3] = [80.0, 8.0, 1.0];
const NOISE_PARITY: [(i32, &[f64]); 4] = [
    (-7, &[1.0, 1.0]),
    (-10, &[1.5, 0.0, 1.0, 0.0, 0.0, 0.0]),
    (-4, &[1.0]),
    (-3, &[1.0, 1.0, 0.0, 1.0, 1.0]),
];
const NOISE_FBM: [(i32, &[f64]); 2] = [(-7, &[1.0, 1.0]), (-3, &[1.0, 0.0, 0.5])];
const NOISE_SMEARED_FBM: [i32; 2] = [-15, -7];

/// Identical to `NoiseSamples.points()` in the oracle tool.
fn noise_points() -> Vec<[f64; 3]> {
    let mut v = vec![
        [0.0, 0.0, 0.0],
        [0.5, 0.25, -0.75],
        [1.0, 2.0, 3.0],
        [-1.5, 64.0, 1000.25],
        [12345.678, -63.9, 98765.4321],
        [16_777_215.5, 0.0, -16_777_215.5],
        [16_777_216.0, 1.0, -16_777_216.0],
        [3.0e7, 100.0, -3.0e7],
        [1e-9, 1e-9, 1e-9],
        [-0.0, 0.0, -0.0],
    ];
    let mut s: i64 = 0x2545_F491_4F6C_DD1D;
    for _ in 0..200 {
        let mut p = [0.0; 3];
        for c in &mut p {
            s = s
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let u = ((s as u64) >> 11) as f64 * 2f64.powi(-53);
            *c = (u * 2.0 - 1.0) * 1.0e6;
        }
        v.push(p);
    }
    v
}

/// Sentinel written for every sample of a noise whose construction was rejected.
const REJECTED: u64 = u64::MAX;

fn fbits(v: f32) -> u64 {
    i64::from(v.to_bits() as i32) as u64
}

/// The shared case list for one (generator kind, seed). Must stay identical to `NoiseSamples.main`.
fn noise_cases<R: RandomSource>(
    mk: &dyn Fn(i64) -> R,
    seed: i64,
    pts: &[[f64; 3]],
    out: &mut Vec<(u64, &'static str)>,
) -> anyhow::Result<()> {
    let pn = PerlinNoise::new(&mut mk(seed));
    for p in pts {
        out.push((fbits(pn.sample(p[0], p[1], p[2])), "perlin 3d"));
        out.push((fbits(pn.sample_2d(p[0], p[2])), "perlin 2d"));
        let mut d = [0f32; 3];
        out.push((
            fbits(pn.sample_with_derivative(p[0], p[1], p[2], &mut d)),
            "perlin derivative value",
        ));
        for v in d {
            out.push((fbits(v), "perlin derivative"));
        }
    }
    let sn = SimplexNoise::new(&mut mk(seed));
    for p in pts {
        out.push((fbits(sn.sample_2d(p[0], p[2])), "simplex 2d"));
        out.push((fbits(sn.sample(p[0], p[1], p[2])), "simplex 3d"));
    }
    for smear in NOISE_SMEARS {
        let mn = SmearedPerlinNoise::new(&mut mk(seed), smear);
        for p in pts {
            out.push((fbits(mn.sample(p[0], p[1], p[2])), "smeared perlin"));
        }
    }
    // A construction that fails writes REJECTED for each of its samples (vanilla does the same on an exception),
    // so "both reject" counts as agreement and a one-sided rejection shows up as a divergence.
    let val = |v: Option<f32>| v.map_or(REJECTED, fbits);
    for (first, amps) in NOISE_PARITY {
        let std = NormalNoise::from_parity(&mut mk(seed), first, amps.to_vec()).ok();
        let nether = NormalNoiseParameters::parity(first, amps.to_vec())
            .and_then(|params| NormalNoise::new_legacy_nether(&mut mk(seed), params))
            .ok();
        for p in pts {
            out.push((
                val(std.as_ref().map(|n| n.sample(p[0], p[1], p[2]))),
                "normal noise",
            ));
            out.push((
                val(nether.as_ref().map(|n| n.sample(p[0], p[1], p[2]))),
                "normal noise (legacy nether)",
            ));
        }
    }
    for (first, amps) in NOISE_FBM {
        let stack = build_legacy_fbm(&mut mk(seed), first, amps).ok();
        for p in pts {
            out.push((
                val(stack.as_ref().map(|n| n.sample(p[0], p[1], p[2]))),
                "legacy fbm",
            ));
        }
    }
    for first in NOISE_SMEARED_FBM {
        let stack = build_smeared_fbm(&mut mk(seed), first, 80.0, 1.0).ok();
        for p in pts {
            out.push((
                val(stack.as_ref().map(|n| n.sample(p[0], p[1], p[2]))),
                "smeared fbm",
            ));
        }
    }
    Ok(())
}

/// `noise-samples`: zenith-math's noises must produce bit-identical floats to vanilla's at the shared points.
pub fn noise_samples(version: &str, write: bool) -> anyhow::Result<bool> {
    let pts = noise_points();
    let mut ours: Vec<(u64, &'static str)> = Vec::new();
    let mut tags: Vec<(usize, &'static str, i64)> = Vec::new();
    for legacy in [false, true] {
        for seed in NOISE_SEEDS {
            tags.push((
                ours.len(),
                if legacy { "legacy" } else { "xoroshiro" },
                seed,
            ));
            if legacy {
                noise_cases(&|s| LegacyRandom::new(s), seed, &pts, &mut ours)?;
            } else {
                noise_cases(&|s| XoroshiroRandom::new(s), seed, &pts, &mut ours)?;
            }
        }
    }

    let (install, java) = crate::oracle::install_and_java(version)?;
    let agent = crate::oracle::build(version)?;
    let mut cp = zenith_install::libraries::game_classpath(&install)?;
    cp.push(agent);
    let dir = zenith_install::cache_dir().join("parity/noise-samples");
    fs::create_dir_all(&dir)?;
    let out_file = dir.join("vanilla.bin");
    let out = std::process::Command::new(&java.java)
        .arg("-cp")
        .arg(zenith_install::libraries::join_classpath(&cp))
        .arg("dev.zenith.oracle.NoiseSamples")
        .arg(&out_file)
        .output()
        .context("run vanilla noise samples")?;
    anyhow::ensure!(
        out.status.success(),
        "vanilla noise samples failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let raw = fs::read(&out_file)?;
    let vanilla: Vec<u64> = raw
        .as_chunks::<8>()
        .0
        .iter()
        .map(|c| u64::from_be_bytes(*c))
        .collect();
    anyhow::ensure!(
        vanilla.len() == ours.len(),
        "value count differs: vanilla {} vs zenith {}",
        vanilla.len(),
        ours.len()
    );

    let mut passed = 0u64;
    let mut by_label: std::collections::BTreeMap<&str, (u64, u64)> =
        std::collections::BTreeMap::new();
    let mut failures: Vec<String> = Vec::new();
    for (i, ((z, label), v)) in ours.iter().zip(&vanilla).enumerate() {
        let e = by_label.entry(label).or_default();
        e.1 += 1;
        if z == v {
            passed += 1;
            e.0 += 1;
        } else if failures.len() < 10 {
            let (_, kind, seed) = tags
                .iter()
                .rev()
                .find(|t| t.0 <= i)
                .copied()
                .unwrap_or((0, "?", 0));
            let (fv, fz) = (f32::from_bits(*v as u32), f32::from_bits(*z as u32));
            failures.push(format!(
                "#{i} {kind} seed {seed} {label}: vanilla {fv:e} vs zenith {fz:e}"
            ));
        }
    }
    let total = ours.len() as u64;
    println!("noise-samples {version}: {passed}/{total} floats bit-identical to vanilla");
    for (label, (p, t)) in &by_label {
        println!("  {label:<32} {p}/{t}");
    }
    for f in &failures {
        println!("  FAIL {f}");
    }
    if write {
        SuiteResult::new("noise-samples", version, passed, total)
            .write(&repo_root().join("progress/parity"))?;
    }
    Ok(passed == total)
}

// ── blockstate-ids ───────────────────────────────────────────────────────────

/// `blockstate-ids`: zenith-registry computes every global block-state ID from the specified enumeration rule
/// (alphabetical properties, `true` before `false`, last property fastest, registration order); each must equal the
/// ID in vanilla's own data-generator report (ingested from the user's jar by `zenith-data ingest`).
pub fn blockstate_ids(version: &str, write: bool) -> anyhow::Result<bool> {
    let tables = zenith_data::Tables::load(&zenith_data::version_dir(version).join("tables.bin"))
        .context("no cached tables; run `cargo run -p zenith-data -- ingest` first")?;
    let table = zenith_registry::block_state::BlockStateTable::from_tables(&tables)
        .map_err(|e| anyhow::anyhow!("build block-state table: {e}"))?;
    let (matched, total) = table.verify_against(&tables);
    println!(
        "blockstate-ids {version}: {matched}/{total} computed state IDs equal vanilla's report"
    );
    for m in table.diff_against(&tables, 10) {
        println!("  FAIL {m:?}");
    }
    if write {
        SuiteResult::new("blockstate-ids", version, matched as u64, total as u64)
            .write(&repo_root().join("progress/parity"))?;
    }
    Ok(matched == total)
}

// ── anvil-io ─────────────────────────────────────────────────────────────────

/// How many region files to sample (deterministic: sorted by path).
const ANVIL_MAX_REGIONS: usize = 48;

fn anvil_regions(extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in world_roots(extra) {
        walk(&root, &mut files);
    }
    let mut regions: Vec<PathBuf> = files
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "mca"))
        .filter(|p| {
            let parent = p
                .parent()
                .and_then(|d| d.file_name())
                .map(|n| n.to_string_lossy().into_owned());
            matches!(parent.as_deref(), Some("region" | "entities" | "poi"))
        })
        .filter(|p| fs::metadata(p).is_ok_and(|m| m.len() >= 8192))
        .collect();
    regions.sort();
    regions.truncate(ANVIL_MAX_REGIONS);
    regions
}

/// Copy a region file plus every external `.mcc` beside it, so neither engine ever opens (and pads) the original.
fn copy_region(src: &Path, dst_dir: &Path) -> anyhow::Result<PathBuf> {
    fs::create_dir_all(dst_dir)?;
    let dst = dst_dir.join(src.file_name().context("region name")?);
    fs::copy(src, &dst)?;
    if let Some(dir) = src.parent() {
        for e in fs::read_dir(dir)?.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with("c.") && name.ends_with(".mcc") {
                fs::copy(e.path(), dst_dir.join(&name))?;
            }
        }
    }
    Ok(dst)
}

/// `anvil-io`: Zenith reads every chunk of real vanilla region files and writes its own copy; vanilla then reads the
/// original and Zenith's copy with its own region code and compares chunks with its own equality. A second check
/// compares the sector layout of Zenith's copy with the copy vanilla writes in the same order.
pub fn anvil_io(version: &str, extra: &[PathBuf], write: bool) -> anyhow::Result<bool> {
    use zenith_storage::{CompressionId, RegionFile, RegionFileOptions, RegionPos};
    let work = zenith_install::cache_dir().join("parity/anvil-io");
    if work.exists() {
        fs::remove_dir_all(&work)?;
    }
    fs::create_dir_all(&work)?;
    let options = RegionFileOptions {
        sync_writes: false,
        compression: CompressionId::Zlib,
    };

    let mut pairs = String::new();
    let mut jobs: Vec<(PathBuf, PathBuf, PathBuf)> = Vec::new(); // (source, zenith copy, vanilla copy)
    let mut zenith_errors: Vec<String> = Vec::new();
    for (i, src) in anvil_regions(extra).into_iter().enumerate() {
        let name = src
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Some(region) = RegionPos::parse_filename(&name) else {
            continue;
        };
        let dir = work.join(i.to_string());
        let for_zenith = copy_region(&src, &dir.join("orig-zenith"))?;
        let for_vanilla = copy_region(&src, &dir.join("orig-vanilla"))?;
        fs::create_dir_all(dir.join("zenith"))?;
        fs::create_dir_all(dir.join("vanilla"))?;
        let zen_path = dir.join("zenith").join(&name);
        let van_path = dir.join("vanilla").join(&name);

        let mut reader = RegionFile::open(&for_zenith, for_zenith.parent().expect("dir"), options)?;
        let mut writer = RegionFile::open(&zen_path, zen_path.parent().expect("dir"), options)?;
        for slot in 0..1024u32 {
            let pos = region.chunk_pos((slot & 31) as u8, (slot >> 5) as u8);
            match reader.read_chunk(pos) {
                Ok(Some(chunk)) => {
                    if let Err(e) = writer.write_chunk(pos, &chunk) {
                        zenith_errors.push(format!("{} slot {slot}: write: {e}", src.display()));
                    }
                }
                Ok(None) => {}
                Err(e) => zenith_errors.push(format!("{} slot {slot}: read: {e}", src.display())),
            }
        }
        reader.close()?;
        writer.close()?;
        pairs.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\n",
            for_vanilla.display(),
            zen_path.display(),
            van_path.display(),
            region.x,
            region.z
        ));
        jobs.push((src, zen_path, van_path));
    }

    let pairs_file = work.join("pairs.tsv");
    fs::write(&pairs_file, pairs)?;
    let flags_file = work.join("flags.bin");
    let (install, java) = crate::oracle::install_and_java(version)?;
    let agent = crate::oracle::build(version)?;
    let mut cp = zenith_install::libraries::game_classpath(&install)?;
    cp.push(agent);
    let out = std::process::Command::new(&java.java)
        .arg("-cp")
        .arg(zenith_install::libraries::join_classpath(&cp))
        .arg("dev.zenith.oracle.RegionCheck")
        .arg(&pairs_file)
        .arg(&flags_file)
        .output()
        .context("run vanilla region check")?;
    anyhow::ensure!(
        out.status.success(),
        "vanilla region check failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let flags = fs::read(&flags_file)?;
    anyhow::ensure!(flags.len() == jobs.len() * 1024, "unexpected flag count");

    let (mut chunk_pass, mut chunk_total, mut layout_pass, mut identical_files) =
        (0u64, 0u64, 0u64, 0usize);
    let mut failures = Vec::new();
    for (j, (src, zen, van)) in jobs.iter().enumerate() {
        for (slot, &b) in flags[j * 1024..(j + 1) * 1024].iter().enumerate() {
            if b & 1 == 0 {
                continue; // no chunk in the original
            }
            chunk_total += 1;
            if b & 2 != 0 && b & 4 != 0 {
                chunk_pass += 1;
            } else if failures.len() < 10 {
                failures.push(format!(
                    "{} slot {slot}: zenith copy {}",
                    src.display(),
                    if b & 2 == 0 { "missing" } else { "differs" }
                ));
            }
        }
        let (zb, vb) = (fs::read(zen)?, fs::read(van)?);
        let layout_same = zb.len() == vb.len() && zb.get(..4096) == vb.get(..4096);
        if layout_same {
            layout_pass += 1;
        } else if failures.len() < 10 {
            failures.push(format!(
                "{}: sector layout differs (zenith {} bytes, vanilla {} bytes)",
                src.display(),
                zb.len(),
                vb.len()
            ));
        }
        // Informational: identical apart from the timestamp table (bytes 4096..8192)?
        if layout_same && zb.get(8192..) == vb.get(8192..) {
            identical_files += 1;
        }
    }
    let regions = jobs.len() as u64;
    println!(
        "anvil-io {version}: chunks round-tripped {chunk_pass}/{chunk_total}, sector layouts identical {layout_pass}/{regions} \
         (files byte-identical apart from timestamps: {identical_files}/{regions})"
    );
    for e in zenith_errors.iter().take(5) {
        println!("  ZENITH ERROR {e}");
    }
    for f in &failures {
        println!("  FAIL {f}");
    }
    // Copies of game data: keep nothing, unless explicitly debugging (ZENITH_PARITY_KEEP=1, local only).
    if std::env::var("ZENITH_PARITY_KEEP").as_deref() != Ok("1") {
        let _ = fs::remove_dir_all(&work);
    }
    let (passed, total) = (chunk_pass + layout_pass, chunk_total + regions);
    if write {
        SuiteResult::new("anvil-io", version, passed, total)
            .write(&repo_root().join("progress/parity"))?;
    }
    Ok(passed == total && zenith_errors.is_empty())
}

// ── math-helpers ─────────────────────────────────────────────────────────────

/// Sentinel for a helper call that vanilla rejects (throws). Must match `MathHelpers.REJECTED`.
const MH_REJECTED: u64 = 0x7EEE_DEAD_BEEF_0001;
const MH_ALPHAS: [f64; 7] = [0.0, 1.0, 0.5, -0.5, 1.5, 0.25, f64::NAN];
const MH_ENDS: [(f64, f64); 6] = [
    (0.0, 1.0),
    (1.0, 0.0),
    (-5.5, 3.25),
    (1e9, -1e9),
    (0.1, 0.2),
    (3.0e7, 3.0e7 + 1.0),
];

// 3.14159 is a deliberate probe input (it must match the Java list), not an approximation of π.
#[allow(clippy::approx_constant)]
fn mh_doubles() -> Vec<f64> {
    let mut v = vec![
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.5,
        -0.5,
        1e-9,
        -1e-9,
        1e-300,
        3.0e9,
        -3.0e9,
        1e19,
        -1e19,
        1e300,
        180.0,
        -180.0,
        359.9,
        720.25,
        3.14159,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        2_147_483_647.5,
        -2_147_483_648.5,
        9.3e18,
        -9.3e18,
    ];
    let scales = [1.0, 100.0, 1.0e4, 1.0e7, 1.0e10];
    let mut s: i64 = 0x51AB_C0DE_1234_5678;
    for i in 0..120 {
        s = s
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let u = ((s as u64) >> 11) as f64 * 2f64.powi(-53);
        v.push((u * 2.0 - 1.0) * scales[i % scales.len()]);
    }
    v
}

fn mh_ints() -> Vec<i32> {
    let mut v = vec![
        0,
        1,
        -1,
        2,
        3,
        7,
        8,
        1023,
        1024,
        1025,
        1 << 30,
        (1 << 30) + 1,
        i32::MAX,
        i32::MIN,
        -2,
        -1024,
    ];
    let mut s: i64 = 0x0DDB_A11C_AFE5_EED5;
    for _ in 0..60 {
        s = s
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        v.push(((s as u64) >> 32) as i32);
    }
    v
}

/// The shared probe list. Must stay in the same order as `MathHelpers.main`.
fn mh_probes() -> Vec<(u64, &'static str)> {
    use zenith_math::helpers::{angles, color, interp, packing, roots, rounding, trig};
    let fb = |v: f32| i64::from(v.to_bits() as i32) as u64;
    let db = |v: f64| v.to_bits();
    let ib = |v: i32| i64::from(v) as u64;
    // black_box keeps the compiler from constant-folding the probes: Rust doesn't guarantee NaN sign/payload in
    // compile-time evaluation, while the running game (and Zenith at runtime) use the hardware's rules.
    let (d, ints) = (
        std::hint::black_box(mh_doubles()),
        std::hint::black_box(mh_ints()),
    );
    let f: Vec<f32> = d.iter().map(|&x| x as f32).collect();
    let mut o: Vec<(u64, &'static str)> = Vec::new();
    for &x in &d {
        o.push((fb(trig::sin(x)), "sin"));
    }
    for &x in &d {
        o.push((fb(trig::cos(x)), "cos"));
    }
    for i in 0..d.len() {
        o.push((db(trig::atan2(d[i], d[(i * 7 + 3) % d.len()])), "atan2"));
    }
    for &x in &d {
        o.push((db(roots::fast_inv_sqrt(x)), "fastInvSqrt"));
    }
    for &x in &f {
        o.push((fb(roots::fast_inv_cube_root(x)), "fastInvCubeRoot"));
    }
    for &x in &f {
        o.push((fb(roots::inv_sqrt_f32(x)), "invSqrt(float)"));
    }
    for &x in &d {
        o.push((db(roots::inv_sqrt_f64(x)), "invSqrt(double)"));
    }
    for &x in &f {
        o.push((ib(rounding::floor_f32(x)), "floor(float)"));
    }
    for &x in &d {
        o.push((ib(rounding::floor_f64(x)), "floor(double)"));
    }
    for &x in &d {
        o.push((rounding::lfloor(x) as u64, "lfloor"));
    }
    for &x in &f {
        o.push((ib(rounding::ceil_f32(x)), "ceil(float)"));
    }
    for &x in &d {
        o.push((ib(rounding::ceil_f64(x)), "ceil(double)"));
    }
    for &x in &f {
        o.push((fb(rounding::frac_f32(x)), "frac(float)"));
    }
    for &x in &d {
        o.push((db(rounding::frac_f64(x)), "frac(double)"));
    }
    for &x in &ints {
        o.push((ib(angles::wrap_degrees_i32(x)), "wrapDegrees(int)"));
    }
    for &x in &f {
        o.push((fb(angles::wrap_degrees_f32(x)), "wrapDegrees(float)"));
    }
    for &x in &d {
        o.push((db(angles::wrap_degrees_f64(x)), "wrapDegrees(double)"));
    }
    for a in std::hint::black_box(MH_ALPHAS) {
        for (e0, e1) in std::hint::black_box(MH_ENDS) {
            o.push((
                fb(interp::lerp_f32(a as f32, e0 as f32, e1 as f32)),
                "lerp(float)",
            ));
        }
    }
    for a in std::hint::black_box(MH_ALPHAS) {
        for (e0, e1) in std::hint::black_box(MH_ENDS) {
            o.push((db(interp::lerp_f64(a, e0, e1)), "lerp(double)"));
        }
    }
    for a in std::hint::black_box(MH_ALPHAS) {
        for (e0, e1) in std::hint::black_box(MH_ENDS) {
            o.push((db(interp::clamped_lerp_f64(a, e0, e1)), "clampedLerp"));
        }
    }
    for a in std::hint::black_box(MH_ALPHAS) {
        for (e0, e1) in std::hint::black_box(MH_ENDS) {
            o.push((db(interp::inverse_lerp_f64(a, e0, e1)), "inverseLerp"));
        }
    }
    for &x in &f {
        o.push((fb(interp::smoothstep(x)), "smoothstep"));
    }
    for i in 0..ints.len() {
        let m = ints[(i * 5 + 1) % ints.len()];
        o.push((
            if m == 0 {
                MH_REJECTED
            } else {
                ib(rounding::positive_modulo_i32(ints[i], m))
            },
            "positiveModulo",
        ));
    }
    for &x in &ints {
        o.push((ib(rounding::ceillog2(x)), "ceillog2"));
    }
    for &x in &ints {
        o.push((ib(rounding::log2(x)), "log2"));
    }
    for &x in &ints {
        o.push((
            ib(rounding::smallest_encompassing_power_of_two(x)),
            "smallestEncompassingPowerOfTwo",
        ));
    }
    for i in 0..60 {
        let k = f64::from(i);
        let (h, s, v) = (
            ((k * 0.618034) % 1.0) as f32,
            ((k * 0.414214) % 1.0) as f32,
            ((k * 0.732051) % 1.0) as f32,
        );
        o.push((ib(color::hsv_to_rgb(h, s, v)), "hsvToRgb"));
    }
    for &x in &d {
        o.push((ib(rounding::sign(x)), "sign"));
    }
    for i in 0..ints.len() {
        let packed = packing::block_as_long(
            ints[i],
            ints[(i * 3 + 1) % ints.len()],
            ints[(i * 11 + 2) % ints.len()],
        );
        o.push((packed as u64, "BlockPos.asLong"));
    }
    let mut s: i64 = 0x0B10_C0B0_5EED_0001;
    for _ in 0..80 {
        s = s
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        o.push((ib(packing::block_get_x(s)), "BlockPos.getX"));
        o.push((ib(packing::block_get_y(s)), "BlockPos.getY"));
        o.push((ib(packing::block_get_z(s)), "BlockPos.getZ"));
    }
    o
}

/// `math-helpers`: zenith-math's helpers must equal the game's own helpers bit for bit on the shared probe list.
pub fn math_helpers(version: &str, write: bool) -> anyhow::Result<bool> {
    let ours = mh_probes();
    let (install, java) = crate::oracle::install_and_java(version)?;
    let agent = crate::oracle::build(version)?;
    let mut cp = zenith_install::libraries::game_classpath(&install)?;
    cp.push(agent);
    let dir = zenith_install::cache_dir().join("parity/math-helpers");
    fs::create_dir_all(&dir)?;
    let out_file = dir.join("vanilla.bin");
    let out = std::process::Command::new(&java.java)
        .arg("-cp")
        .arg(zenith_install::libraries::join_classpath(&cp))
        .arg("dev.zenith.oracle.MathHelpers")
        .arg(&out_file)
        .output()
        .context("run vanilla math helpers")?;
    anyhow::ensure!(
        out.status.success(),
        "vanilla math helpers failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let raw = fs::read(&out_file)?;
    let vanilla: Vec<u64> = raw
        .as_chunks::<8>()
        .0
        .iter()
        .map(|c| u64::from_be_bytes(*c))
        .collect();
    anyhow::ensure!(
        vanilla.len() == ours.len(),
        "value count differs: vanilla {} vs zenith {}",
        vanilla.len(),
        ours.len()
    );
    let mut by_label: std::collections::BTreeMap<&str, (u64, u64)> =
        std::collections::BTreeMap::new();
    let mut failures = Vec::new();
    let mut passed = 0u64;
    for (i, ((z, label), v)) in ours.iter().zip(&vanilla).enumerate() {
        let e = by_label.entry(label).or_default();
        e.1 += 1;
        if z == v {
            passed += 1;
            e.0 += 1;
        } else if failures.len() < 12 {
            failures.push(format!(
                "#{i} {label}: vanilla {v:#018x} vs zenith {z:#018x}"
            ));
        }
    }
    let total = ours.len() as u64;
    println!("math-helpers {version}: {passed}/{total} values identical to vanilla");
    for (label, (p, t)) in &by_label {
        if p != t {
            println!("  {label:<34} {p}/{t}");
        }
    }
    for f in &failures {
        println!("  FAIL {f}");
    }
    if write {
        SuiteResult::new("math-helpers", version, passed, total)
            .write(&repo_root().join("progress/parity"))?;
    }
    Ok(passed == total)
}
