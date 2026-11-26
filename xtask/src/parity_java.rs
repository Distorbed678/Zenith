//! `java-hashset`: `JavaBlockPosSet` (the `java.util.HashSet<BlockPos>` emulation) against fixtures produced by the
//! user's own JDK. The generator (`tools/javamap-fixtures/HashSetOrder.java`) uses the JDK only, no game code.

use std::io::{BufReader, Read};
use std::process::Stdio;

use anyhow::Context;
use zenith_features::{BlockPos, JavaBlockPosSet};
use zenith_parity::SuiteResult;

use crate::parity::vanilla_command;
use crate::repo_root;

/// Fixed seed and sequence count (override the count with `ZENITH_HASHSET_COUNT` for quick runs).
const SEED: i64 = 0x5EED_4A5E;
const COUNT: u32 = 40_000;
const MAGIC: i32 = 0x4853_4F31;
const KIND_NAMES: [&str; 5] = ["patch", "dense", "growth", "bfs", "interleave"];

struct Stream<R: Read>(R);

impl<R: Read> Stream<R> {
    fn bytes<const N: usize>(&mut self) -> anyhow::Result<[u8; N]> {
        let mut b = [0u8; N];
        self.0
            .read_exact(&mut b)
            .context("fixture stream ended early")?;
        Ok(b)
    }
    fn i8(&mut self) -> anyhow::Result<i8> {
        Ok(i8::from_be_bytes(self.bytes()?))
    }
    fn i32(&mut self) -> anyhow::Result<i32> {
        Ok(i32::from_be_bytes(self.bytes()?))
    }
    fn i64(&mut self) -> anyhow::Result<i64> {
        Ok(i64::from_be_bytes(self.bytes()?))
    }
    fn pos(&mut self) -> anyhow::Result<BlockPos> {
        Ok(BlockPos::new(self.i32()?, self.i32()?, self.i32()?))
    }
}

/// The order digest the generator records (FNV-1a over x, y, z of every element in iteration order).
fn digest(set: &JavaBlockPosSet) -> i64 {
    let mut d: u64 = 0xcbf2_9ce4_8422_2325;
    for p in set.iter() {
        for v in [p.x, p.y, p.z] {
            d ^= u64::from(v as u32);
            d = d.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    d as i64
}

#[derive(Default, Clone, Copy)]
struct Tally {
    total: u64,
    identical: u64,
    nondeterministic: u64,
    nondeterministic_but_equal: u64,
    failed: u64,
}

/// Replays one sequence; `Err` carries what differed first.
fn replay<R: Read>(
    s: &mut Stream<R>,
    log: &mut Vec<String>,
) -> anyhow::Result<(i8, Result<(), String>, u32)> {
    let kind = s.i8()?;
    let ops = s.i32()?;
    let mut set = JavaBlockPosSet::new();
    let mut failure: Option<String> = None;
    for i in 0..ops {
        match s.i8()? {
            0 => {
                let pos = s.pos()?;
                log.push(format!("insert {} {} {}", pos.x, pos.y, pos.z));
                set.insert(pos);
            }
            1 => {
                let (jdk_pos, jdk_digest, jdk_len) = (s.pos()?, s.i64()?, s.i32()?);
                log.push("pop".to_owned());
                let ours = set.pop_first();
                if failure.is_none() {
                    if ours != Some(jdk_pos) {
                        failure = Some(format!("op {i}: pop-first gave {ours:?}, JDK {jdk_pos:?}"));
                    } else if set.len() as i32 != jdk_len || digest(&set) != jdk_digest {
                        failure = Some(format!("op {i}: order after pop-first differs"));
                    }
                }
            }
            2 => {
                let (pos, removed, jdk_digest, jdk_len) = (s.pos()?, s.i8()?, s.i64()?, s.i32()?);
                log.push(format!("remove {} {} {}", pos.x, pos.y, pos.z));
                let ours = set.remove(pos);
                if failure.is_none() {
                    if ours != (removed != 0) {
                        failure = Some(format!("op {i}: remove({pos:?}) gave {ours}"));
                    } else if set.len() as i32 != jdk_len || digest(&set) != jdk_digest {
                        failure = Some(format!("op {i}: order after remove differs"));
                    }
                }
            }
            t => anyhow::bail!("unknown op type {t}"),
        }
    }
    let n = s.i32()?;
    let mut expected = Vec::with_capacity(n.max(0) as usize);
    for _ in 0..n {
        expected.push(s.pos()?);
    }
    if failure.is_none() {
        let got: Vec<BlockPos> = set.iter().collect();
        if got != expected {
            let at = got
                .iter()
                .zip(&expected)
                .position(|(a, b)| a != b)
                .unwrap_or(got.len().min(expected.len()));
            failure = Some(format!(
                "final order differs at element {at} of {} (ours {}, JDK {n})",
                got.len(),
                got.len()
            ));
        }
    }
    Ok((kind, failure.map_or(Ok(()), Err), set.identity_tie_breaks()))
}

pub fn java_hashset(version: &str, write: bool) -> anyhow::Result<bool> {
    let count = std::env::var("ZENITH_HASHSET_COUNT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(COUNT);
    let (_, java) = crate::oracle::install_and_java(version)?;
    let source = repo_root().join("tools/javamap-fixtures/HashSetOrder.java");
    let mut child = vanilla_command(&java.java)?
        .arg("-Xmx1G")
        .arg(&source)
        .arg(SEED.to_string())
        .arg(count.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .context("run the JDK fixture generator")?;
    let mut stream = Stream(BufReader::with_capacity(
        1 << 20,
        child.stdout.take().context("generator stdout")?,
    ));

    anyhow::ensure!(stream.i32()? == MAGIC, "unexpected fixture header");
    let (seed, n) = (stream.i64()?, stream.i32()?);
    anyhow::ensure!(seed == SEED && n == count as i32, "fixture header mismatch");

    let mut per_kind = [Tally::default(); KIND_NAMES.len()];
    let mut failures = Vec::new();
    for index in 0..n {
        let mut log = Vec::new();
        let (kind, outcome, ties) = replay(&mut stream, &mut log)?;
        let t = &mut per_kind[kind as usize];
        t.total += 1;
        match (outcome, ties) {
            (Ok(()), 0) => t.identical += 1,
            (Ok(()), _) => {
                t.nondeterministic += 1;
                t.nondeterministic_but_equal += 1;
            }
            (Err(_), ties) if ties > 0 => t.nondeterministic += 1,
            (Err(why), _) => {
                t.failed += 1;
                // Debug aid: ZENITH_HASHSET_DUMP=<dir> writes every failing sequence's operations as text.
                if let Ok(dir) = std::env::var("ZENITH_HASHSET_DUMP") {
                    std::fs::create_dir_all(&dir)?;
                    std::fs::write(format!("{dir}/seq{index}.txt"), log.join("\n") + "\n")?;
                }
                failures.push(format!(
                    "sequence {index} ({}): {why}",
                    KIND_NAMES[kind as usize]
                ));
            }
        }
    }
    let status = child.wait()?;
    anyhow::ensure!(status.success(), "the JDK fixture generator failed");

    let sum = |f: fn(&Tally) -> u64| per_kind.iter().map(f).sum::<u64>();
    let (identical, nondet, nondet_equal, failed) = (
        sum(|t| t.identical),
        sum(|t| t.nondeterministic),
        sum(|t| t.nondeterministic_but_equal),
        sum(|t| t.failed),
    );
    let total = identical + failed;
    println!(
        "java-hashset {version}: {identical}/{total} sequences identical to the JDK's HashSet \
         ({nondet} more are nondeterministic in the JDK: identity tie-break, {nondet_equal} of them equal anyway)"
    );
    for (name, t) in KIND_NAMES.iter().zip(&per_kind) {
        println!(
            "  {name:<10} {:>6} sequences: {} identical, {} nondeterministic, {} failed",
            t.total, t.identical, t.nondeterministic, t.failed
        );
    }
    for f in failures.iter().take(15) {
        println!("  FAIL {f}");
    }
    if write {
        SuiteResult::new("java-hashset", version, identical, total)
            .write(&repo_root().join("progress/parity"))?;
    }
    Ok(failed == 0)
}
