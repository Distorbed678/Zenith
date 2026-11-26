//! `cargo xtask parity block-facts`: every static fact of every block state (ADR 0009, `docs/specs/blocks/`,
//! format `docs/specs/parity/block-facts.md`) from the game's own classes vs `zenith-blocks`.
//!
//! The vanilla side is the `BlockFacts` oracle tool. Its dump is game data, so it is written under the cache folder,
//! read once and deleted. One check is counted per (state, field group).

use std::collections::BTreeMap;
use std::fs;

use anyhow::{Context, bail};
use zenith_blocks::{
    BlockFactTags, BlockFacts, BlockTransforms, Direction, Mirror, OffsetType, Rotation, ShapeKind,
    SupportType, TransformRule, VoxelShape, flag, table::Tables,
};
use zenith_codec::log::null_log;
use zenith_codec::resources::ZipPackResources;
use zenith_parity::SuiteResult;
use zenith_registry::{BlockStateTable, Identifier};
use zenith_worldgen::BlockTags;

use crate::parity::vanilla_command;
use crate::repo_root;

const PROBES: [[i32; 3]; 3] = [[0, 0, 0], [1, 0, 3], [-7, 64, 13]];

/// A shape as dumped: empty, or the bounds followed by every box (bit patterns of the six doubles).
type DShape = Vec<[u64; 6]>;

struct Reader<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> anyhow::Result<&'a [u8]> {
        let end = self
            .p
            .checked_add(n)
            .filter(|&e| e <= self.d.len())
            .context("dump truncated")?;
        let s = &self.d[self.p..end];
        self.p = end;
        Ok(s)
    }
    fn u8(&mut self) -> anyhow::Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> anyhow::Result<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into()?))
    }
    fn u32(&mut self) -> anyhow::Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into()?))
    }
    fn i32(&mut self) -> anyhow::Result<i32> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into()?))
    }
    fn u64(&mut self) -> anyhow::Result<u64> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into()?))
    }
    fn str(&mut self) -> anyhow::Result<String> {
        let n = usize::from(self.u16()?);
        Ok(String::from_utf8(self.take(n)?.to_vec())?)
    }
    fn boxed(&mut self) -> anyhow::Result<[u64; 6]> {
        let mut b = [0; 6];
        for v in &mut b {
            *v = self.u64()?;
        }
        Ok(b)
    }
    fn shape(&mut self) -> anyhow::Result<DShape> {
        let n = usize::from(self.u16()?);
        if n == 0 {
            return Ok(Vec::new());
        }
        let mut s = Vec::with_capacity(n + 1);
        for _ in 0..=n {
            s.push(self.boxed()?);
        }
        Ok(s)
    }
}

/// The vanilla record of one state.
#[derive(Default)]
struct Rec {
    id: u32,
    block: String,
    props: Vec<(String, String)>,
    flags: u32,
    /// light_block, light_emission, map id.
    small: [u8; 3],
    map_rgb: u32,
    push: String,
    instrument: String,
    render: u8,
    offset_type: u8,
    offsets: Option<[[u64; 3]; 3]>,
    post: Option<[i32; 3]>,
    /// destroy, explosion, friction, speed, jump, bounce, fall reduction, shade (bit patterns).
    floats: [u32; 8],
    sound_vol: u32,
    sound_pitch: u32,
    sound_events: [String; 5],
    fluid_id: String,
    fluid_amount: u8,
    fluid_flags: u8,
    fluid_height: u32,
    /// occlusion, collision, outline, visual, interaction, support at P0.
    shapes: [DShape; 6],
    pos_mask: u8,
    /// P1 then P2: collision, outline, visual, interaction, support.
    pos_shapes: Vec<DShape>,
    sturdy: [u8; 3],
    coll_face_full: u8,
    face_occ: [DShape; 6],
}

fn parse_dump(raw: &[u8]) -> anyhow::Result<(u32, Vec<Rec>)> {
    let mut r = Reader { d: raw, p: 0 };
    anyhow::ensure!(r.take(4)? == b"ZBF1", "bad magic");
    anyhow::ensure!(r.u16()? == 1, "unsupported version");
    let total = r.u32()?;
    let start = r.u32()?;
    let count = r.u32()?;
    for p in PROBES {
        for c in p {
            anyhow::ensure!(r.i32()? == c, "probe position mismatch");
        }
    }
    let view = r.boxed()?;
    anyhow::ensure!(
        view == [
            0,
            0,
            0,
            1.0f64.to_bits(),
            1.0f64.to_bits(),
            1.0f64.to_bits()
        ],
        "view probe box"
    );
    let mut out = Vec::with_capacity(count as usize);
    for expect in start..start + count {
        let mut rec = Rec {
            id: r.u32()?,
            ..Rec::default()
        };
        anyhow::ensure!(rec.id == expect, "state id out of order");
        rec.block = r.str()?;
        for _ in 0..r.u8()? {
            let k = r.str()?;
            let v = r.str()?;
            rec.props.push((k, v));
        }
        rec.flags = r.u32()?;
        rec.small[0] = r.u8()?;
        rec.small[1] = r.u8()?;
        rec.small[2] = r.u8()?;
        rec.map_rgb = r.u32()?;
        rec.push = r.str()?;
        rec.instrument = r.str()?;
        rec.render = r.u8()?;
        rec.offset_type = r.u8()?;
        if rec.offset_type != 0 {
            let mut o = [[0u64; 3]; 3];
            for row in &mut o {
                for v in row {
                    *v = r.u64()?;
                }
            }
            rec.offsets = Some(o);
        }
        if r.u8()? == 1 {
            rec.post = Some([r.i32()?, r.i32()?, r.i32()?]);
        }
        for f in &mut rec.floats {
            *f = r.u32()?;
        }
        rec.sound_vol = r.u32()?;
        rec.sound_pitch = r.u32()?;
        for e in &mut rec.sound_events {
            *e = r.str()?;
        }
        rec.fluid_id = r.str()?;
        rec.fluid_amount = r.u8()?;
        rec.fluid_flags = r.u8()?;
        rec.fluid_height = r.u32()?;
        for s in &mut rec.shapes {
            *s = r.shape()?;
        }
        rec.pos_mask = r.u8()?;
        if rec.pos_mask != 0 {
            for _ in 0..10 {
                rec.pos_shapes.push(r.shape()?);
            }
        }
        for m in &mut rec.sturdy {
            *m = r.u8()?;
        }
        rec.coll_face_full = r.u8()?;
        for s in &mut rec.face_occ {
            *s = r.shape()?;
        }
        out.push(rec);
    }
    anyhow::ensure!(r.p == raw.len(), "trailing bytes in dump");
    Ok((total, out))
}

fn shape_bits(s: &VoxelShape) -> DShape {
    let Some(b) = s.bounds() else {
        return Vec::new();
    };
    let bits = |a: &zenith_blocks::Aabb| a.to_array().map(f64::to_bits);
    let mut v = vec![bits(&b)];
    v.extend(s.boxes().iter().map(bits));
    v
}

fn show_shape(s: &DShape) -> String {
    if s.is_empty() {
        return "EMPTY".to_owned();
    }
    let f = |a: &[u64; 6]| {
        let v: Vec<String> = a
            .iter()
            .map(|&b| format!("{}", f64::from_bits(b) * 16.0))
            .collect();
        format!("({})", v.join(","))
    };
    let boxes: Vec<String> = s[1..].iter().map(f).collect();
    format!(
        "bounds {} boxes {} [{}px]",
        f(&s[0]),
        s.len() - 1,
        boxes.join(" ")
    )
}

fn f32s(bits: u32) -> String {
    format!("{}", f32::from_bits(bits))
}

struct Failure {
    group: &'static str,
    state: String,
    detail: String,
}

#[derive(Default)]
struct Report {
    checks: BTreeMap<&'static str, (u64, u64)>,
    fields: BTreeMap<String, u64>,
    failures: Vec<Failure>,
}

impl Report {
    /// Records one check; `diffs` are `(field, vanilla, zenith)` triples.
    fn check(
        &mut self,
        group: &'static str,
        state: &dyn Fn() -> String,
        diffs: Vec<(String, String, String)>,
    ) {
        let e = self.checks.entry(group).or_default();
        e.1 += 1;
        if diffs.is_empty() {
            e.0 += 1;
            return;
        }
        for (field, ..) in &diffs {
            *self.fields.entry(format!("{group}.{field}")).or_default() += 1;
        }
        let detail = diffs
            .iter()
            .map(|(f, v, z)| format!("{f}: vanilla {v} | zenith {z}"))
            .collect::<Vec<_>>()
            .join("; ");
        self.failures.push(Failure {
            group,
            state: state(),
            detail,
        });
    }
}

fn diff<T: PartialEq>(
    out: &mut Vec<(String, String, String)>,
    field: &str,
    v: T,
    z: T,
    show: impl Fn(&T) -> String,
) {
    if v != z {
        out.push((field.to_owned(), show(&v), show(&z)));
    }
}

fn dir_mask(m: u8) -> String {
    let names: Vec<&str> = Direction::ALL
        .iter()
        .filter(|d| m & (1 << d.ordinal()) != 0)
        .map(|d| d.name())
        .collect();
    format!("{m:#08b}[{}]", names.join(","))
}

fn compare(vanilla: &[Rec], facts: &BlockFacts, blocks: &BlockStateTable) -> Report {
    let tables = Tables::get();
    let mut rep = Report::default();
    for rec in vanilla {
        let id = rec.id;
        let st = facts.state(id);
        let zs = blocks.state_by_id_exact(id as i32).expect("state exists");
        let desc = || {
            let props: Vec<String> = rec.props.iter().map(|(k, v)| format!("{k}={v}")).collect();
            format!("#{id} {}[{}]", rec.block, props.join(","))
        };

        // identity
        {
            let mut d = Vec::new();
            diff(
                &mut d,
                "block_id",
                rec.block.clone(),
                zs.block_id.to_string(),
                String::clone,
            );
            diff(
                &mut d,
                "props",
                rec.props.clone(),
                zs.properties.clone(),
                |p| format!("{p:?}"),
            );
            rep.check("identity", &desc, d);
        }
        // flags
        {
            let mut d = Vec::new();
            let x = rec.flags ^ st.flags;
            for (bit, name) in flag::NAMES.iter().enumerate() {
                if x & (1 << bit) != 0 {
                    d.push((
                        (*name).to_owned(),
                        (rec.flags >> bit & 1 == 1).to_string(),
                        (st.flags >> bit & 1 == 1).to_string(),
                    ));
                }
            }
            rep.check("flags", &desc, d);
        }
        // numbers
        {
            let mut d = Vec::new();
            diff(
                &mut d,
                "light_block",
                rec.small[0],
                st.light_block,
                u8::to_string,
            );
            diff(
                &mut d,
                "light_emission",
                rec.small[1],
                st.light_emission,
                u8::to_string,
            );
            diff(
                &mut d,
                "map_color_id",
                rec.small[2],
                st.map_color_id,
                u8::to_string,
            );
            diff(
                &mut d,
                "map_color_rgb",
                rec.map_rgb,
                st.map_color_rgb,
                |v| format!("{v:06X}"),
            );
            diff(
                &mut d,
                "push_reaction",
                rec.push.clone(),
                st.push_reaction.to_owned(),
                String::clone,
            );
            diff(
                &mut d,
                "instrument",
                rec.instrument.clone(),
                st.instrument.to_owned(),
                String::clone,
            );
            diff(
                &mut d,
                "render_shape",
                rec.render,
                st.render_shape,
                u8::to_string,
            );
            let zt = match st.offset_type {
                OffsetType::None => 0,
                OffsetType::Xz => 1,
                OffsetType::Xyz => 2,
            };
            diff(&mut d, "offset_type", rec.offset_type, zt, u8::to_string);
            if let Some(o) = rec.offsets {
                for (i, p) in PROBES.iter().enumerate() {
                    let z = facts.offset(id, *p).map(f64::to_bits);
                    diff(&mut d, &format!("offset_p{i}"), o[i], z, |b| {
                        format!("{:?}", b.map(f64::from_bits))
                    });
                }
            }
            diff(&mut d, "post_process", rec.post, st.post_process, |p| {
                format!("{p:?}")
            });
            let zf = [
                st.destroy_speed,
                st.explosion_resistance,
                st.friction,
                st.speed_factor,
                st.jump_factor,
                st.bounce_restitution,
                st.fall_distance_reduction,
                st.shade_brightness,
            ];
            let names = [
                "destroy_speed",
                "explosion_resistance",
                "friction",
                "speed_factor",
                "jump_factor",
                "bounce_restitution",
                "fall_distance_reduction",
                "shade_brightness",
            ];
            for i in 0..8 {
                diff(&mut d, names[i], rec.floats[i], zf[i].to_bits(), |b| {
                    f32s(*b)
                });
            }
            rep.check("numbers", &desc, d);
        }
        // sound
        {
            let mut d = Vec::new();
            let s = &tables.sounds[st.sound as usize];
            diff(&mut d, "volume", rec.sound_vol, s.volume.to_bits(), |b| {
                f32s(*b)
            });
            diff(&mut d, "pitch", rec.sound_pitch, s.pitch.to_bits(), |b| {
                f32s(*b)
            });
            for (i, n) in ["break", "step", "place", "hit", "fall"].iter().enumerate() {
                diff(
                    &mut d,
                    n,
                    rec.sound_events[i].clone(),
                    s.events[i].clone(),
                    String::clone,
                );
            }
            rep.check("sound", &desc, d);
        }
        // fluid
        {
            let mut d = Vec::new();
            let f = &st.fluid;
            diff(
                &mut d,
                "fluid_id",
                rec.fluid_id.clone(),
                f.kind.id().to_owned(),
                String::clone,
            );
            diff(&mut d, "amount", rec.fluid_amount, f.amount, u8::to_string);
            let zflags =
                u8::from(f.falling) | (u8::from(f.is_source) << 1) | (u8::from(f.is_empty()) << 2);
            diff(
                &mut d,
                "flags(falling|source|empty)",
                rec.fluid_flags,
                zflags,
                |v| format!("{v:#05b}"),
            );
            diff(
                &mut d,
                "own_height",
                rec.fluid_height,
                f.own_height.to_bits(),
                |b| f32s(*b),
            );
            rep.check("fluid", &desc, d);
        }
        // shapes
        let kinds = [
            ("occlusion", ShapeKind::Occlusion),
            ("collision", ShapeKind::Collision),
            ("outline", ShapeKind::Outline),
            ("visual", ShapeKind::Visual),
            ("interaction", ShapeKind::Interaction),
            ("support", ShapeKind::Support),
        ];
        for (i, (name, kind)) in kinds.iter().enumerate() {
            let mut d = Vec::new();
            let z = shape_bits(facts.shape_at_origin(id, *kind));
            diff(&mut d, "shape", rec.shapes[i].clone(), z, show_shape);
            rep.check(name, &desc, d);
        }
        // position variation
        {
            let mut d = Vec::new();
            let pos_kinds = [
                ShapeKind::Collision,
                ShapeKind::Outline,
                ShapeKind::Visual,
                ShapeKind::Interaction,
                ShapeKind::Support,
            ];
            let at = |p: [i32; 3]| -> Vec<DShape> {
                pos_kinds
                    .iter()
                    .map(|k| shape_bits(&facts.shape_at(id, *k, p)))
                    .collect()
            };
            let (z0, z1, z2) = (at(PROBES[0]), at(PROBES[1]), at(PROBES[2]));
            let mut zmask = 0u8;
            for i in 0..5 {
                if z0[i] != z1[i] || z0[i] != z2[i] {
                    zmask |= 1 << i;
                }
            }
            diff(&mut d, "pos_diff_mask", rec.pos_mask, zmask, |m| {
                format!("{m:#07b}")
            });
            if rec.pos_mask != 0 && zmask != 0 {
                let names = ["collision", "outline", "visual", "interaction", "support"];
                for i in 0..5 {
                    diff(
                        &mut d,
                        &format!("{}_p1", names[i]),
                        rec.pos_shapes[i].clone(),
                        z1[i].clone(),
                        show_shape,
                    );
                    diff(
                        &mut d,
                        &format!("{}_p2", names[i]),
                        rec.pos_shapes[5 + i].clone(),
                        z2[i].clone(),
                        show_shape,
                    );
                }
            }
            rep.check("shape_pos", &desc, d);
        }
        // face facts
        {
            let mut d = Vec::new();
            for (i, name) in ["full", "center", "rigid"].iter().enumerate() {
                diff(&mut d, name, rec.sturdy[i], st.face_sturdy[i], |m| {
                    dir_mask(*m)
                });
            }
            let _ = SupportType::ALL;
            rep.check("face_sturdy", &desc, d);
        }
        {
            let mut d = Vec::new();
            diff(
                &mut d,
                "collision_face_full",
                rec.coll_face_full,
                st.collision_face_full,
                |m| dir_mask(*m),
            );
            rep.check("collision_face_full", &desc, d);
        }
        {
            let mut d = Vec::new();
            for dir in Direction::ALL {
                let z = shape_bits(facts.face_occlusion_shape(id, dir));
                diff(
                    &mut d,
                    dir.name(),
                    rec.face_occ[dir.ordinal()].clone(),
                    z,
                    show_shape,
                );
            }
            rep.check("face_occlusion", &desc, d);
        }
    }
    rep
}

fn run_vanilla(version: &str) -> anyhow::Result<Vec<u8>> {
    let (install, java) = crate::oracle::install_and_java(version)?;
    let agent = crate::oracle::build(version)?;
    let mut cp = zenith_install::libraries::game_classpath(&install)?;
    cp.push(agent);
    let dir = zenith_install::cache_dir().join("parity/block-facts");
    fs::create_dir_all(&dir)?;
    let out_file = dir.join("block-facts.zbf1");
    let out = vanilla_command(&java.java)?
        .arg("-Xmx2G")
        .arg("-cp")
        .arg(zenith_install::libraries::join_classpath(&cp))
        .arg("dev.zenith.oracle.BlockFacts")
        .arg(&out_file)
        .output()
        .context("run vanilla block facts")?;
    anyhow::ensure!(
        out.status.success(),
        "vanilla block facts failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let raw = fs::read(&out_file)?;
    let _ = fs::remove_file(&out_file); // game data: never keep it
    Ok(raw)
}

fn build_zenith(version: &str) -> anyhow::Result<(BlockStateTable, BlockFacts)> {
    let install = zenith_install::discover_installs(Some(version))
        .into_iter()
        .next()
        .context("no install")?;
    let tables = zenith_data::Tables::load(&zenith_data::version_dir(version).join("tables.bin"))
        .context("no cached tables; run `cargo run -p zenith-data -- ingest` first")?;
    let blocks = BlockStateTable::from_tables(&tables)
        .map_err(|e| anyhow::anyhow!("build block-state table: {e}"))?;
    let pack = ZipPackResources::open("vanilla", &install.client_jar, null_log());
    let tags =
        BlockTags::load(&pack, &blocks).map_err(|e| anyhow::anyhow!("load block tags: {e}"))?;
    let fact_tags = BlockFactTags::from_resolver(&blocks, |name| {
        Identifier::parse(name)
            .ok()
            .and_then(|id| tags.block_ids(&id))
    });
    let facts = BlockFacts::build(&blocks, &fact_tags)
        .map_err(|e| anyhow::anyhow!("build block facts: {e}"))?;
    Ok((blocks, facts))
}

/// `block-facts`: dump every state's facts from the game and compare each field group with `zenith-blocks`.
pub fn block_facts(
    version: &str,
    write: bool,
    limit: usize,
    only: Option<&str>,
) -> anyhow::Result<bool> {
    let (blocks, facts) = build_zenith(version)?;
    let raw = run_vanilla(version)?;
    let (total, vanilla) = parse_dump(&raw)?;
    drop(raw);
    if total as usize != facts.len() || vanilla.len() != facts.len() {
        bail!(
            "state count: vanilla dumped {} of {total}, zenith has {}",
            vanilla.len(),
            facts.len()
        );
    }
    // Self-check on the dump itself: the numbers the spec quotes for 26.3.
    let nonempty = |i: usize| vanilla.iter().filter(|r| !r.shapes[i].is_empty()).count();
    println!(
        "vanilla dump: {} states; non-empty shapes: outline {} collision {} interaction {} support {}; \
         position-dependent {}; offset blocks {}; total dumped boxes {}",
        vanilla.len(),
        nonempty(2),
        nonempty(1),
        nonempty(4),
        nonempty(5),
        vanilla.iter().filter(|r| r.pos_mask != 0).count(),
        vanilla.iter().filter(|r| r.offset_type != 0).count(),
        vanilla
            .iter()
            .map(|r| r
                .shapes
                .iter()
                .map(|s| s.len().saturating_sub(1))
                .sum::<usize>())
            .sum::<usize>(),
    );
    let rep = compare(&vanilla, &facts, &blocks);
    let (mut passed, mut count) = (0u64, 0u64);
    println!(
        "block-facts {version}: {} states, {} distinct shapes",
        facts.len(),
        facts.shape_count()
    );
    for (group, (ok, n)) in &rep.checks {
        println!("  {group:<22} {ok}/{n}");
        passed += ok;
        count += n;
    }
    println!("block-facts {version}: {passed}/{count} checks");
    if !rep.fields.is_empty() {
        println!("failing fields:");
        for (f, n) in &rep.fields {
            println!("  {f:<46} {n}");
        }
    }
    let mut shown = 0;
    for f in &rep.failures {
        if only.is_some_and(|o| !f.state.contains(o)) {
            continue;
        }
        if shown >= limit {
            break;
        }
        println!("  FAIL [{}] {}: {}", f.group, f.state, f.detail);
        shown += 1;
    }
    if write {
        SuiteResult::new("block-facts", version, passed, count)
            .write(&repo_root().join("progress/parity"))?;
    }
    Ok(passed == count)
}

// ---------------------------------------------------------------------------------------------------------------
// block-transforms: rotation and mirror of every block state (docs/specs/blocks/state-transforms.md, ZBT1).
// ---------------------------------------------------------------------------------------------------------------

/// One state's dumped results: `result[mirror * 4 + rotation]`.
struct TransformRec {
    id: u32,
    result: [u32; 12],
}

/// Decodes a `ZBT1` dump (`docs/specs/parity/block-transforms.md`), consuming the file exactly.
fn parse_transform_dump(raw: &[u8]) -> anyhow::Result<(u32, Vec<TransformRec>)> {
    const HEADER: usize = 18;
    const RECORD: usize = 52;
    anyhow::ensure!(raw.len() >= HEADER, "ZBT1 dump shorter than its header");
    anyhow::ensure!(&raw[..4] == b"ZBT1", "bad ZBT1 magic");
    let u16_at = |at: usize| u16::from_be_bytes([raw[at], raw[at + 1]]);
    let u32_at = |at: usize| u32::from_be_bytes([raw[at], raw[at + 1], raw[at + 2], raw[at + 3]]);
    anyhow::ensure!(u16_at(4) == 1, "unsupported ZBT1 version {}", u16_at(4));
    let total = u32_at(6);
    let start = u32_at(10);
    let count = u32_at(14) as usize;
    anyhow::ensure!(
        raw.len() == HEADER + RECORD * count,
        "ZBT1 size {} does not match {count} records",
        raw.len()
    );
    let mut recs = Vec::with_capacity(count);
    for i in 0..count {
        let at = HEADER + RECORD * i;
        let id = u32_at(at);
        anyhow::ensure!(
            id == start + i as u32,
            "ZBT1 record {i} has id {id}, expected {}",
            start + i as u32
        );
        let mut result = [0u32; 12];
        for (k, r) in result.iter_mut().enumerate() {
            *r = u32_at(at + 4 + 4 * k);
        }
        recs.push(TransformRec { id, result });
    }
    Ok((total, recs))
}

fn run_vanilla_transforms(version: &str) -> anyhow::Result<Vec<u8>> {
    let (install, java) = crate::oracle::install_and_java(version)?;
    let agent = crate::oracle::build(version)?;
    let mut cp = zenith_install::libraries::game_classpath(&install)?;
    cp.push(agent);
    let dir = zenith_install::cache_dir().join("parity/block-transforms");
    fs::create_dir_all(&dir)?;
    let out_file = dir.join("block-transforms.zbt1");
    let out = vanilla_command(&java.java)?
        .arg("-Xmx2G")
        .arg("-cp")
        .arg(zenith_install::libraries::join_classpath(&cp))
        .arg("dev.zenith.oracle.BlockTransforms")
        .arg(&out_file)
        .output()
        .context("run vanilla block transforms")?;
    anyhow::ensure!(
        out.status.success(),
        "vanilla block transforms failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let raw = fs::read(&out_file)?;
    let _ = fs::remove_file(&out_file); // game data: never keep it
    Ok(raw)
}

fn build_block_table(version: &str) -> anyhow::Result<BlockStateTable> {
    let tables = zenith_data::Tables::load(&zenith_data::version_dir(version).join("tables.bin"))
        .context("no cached tables; run `cargo run -p zenith-data -- ingest` first")?;
    BlockStateTable::from_tables(&tables)
        .map_err(|e| anyhow::anyhow!("build block-state table: {e}"))
}

fn describe_state(blocks: &BlockStateTable, id: u32) -> String {
    match blocks.state_by_id_exact(id as i32) {
        Some(s) => {
            let props: Vec<String> = s
                .properties
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect();
            format!("{}[{}] (#{id})", s.block_id, props.join(","))
        }
        None => format!("<unregistered #{id}>"),
    }
}

/// `block-transforms`: the 12 (mirror, rotation) results of every state from the game vs `zenith-blocks`.
pub fn block_transforms(version: &str, write: bool, limit: usize) -> anyhow::Result<bool> {
    let blocks = build_block_table(version)?;
    let transforms = BlockTransforms::build(&blocks)
        .map_err(|e| anyhow::anyhow!("build block transforms: {e}"))?;
    let raw = run_vanilla_transforms(version)?;
    let (total, vanilla) = parse_transform_dump(&raw)?;
    drop(raw);
    if total as usize != blocks.len() || vanilla.len() != blocks.len() {
        bail!(
            "state count: vanilla dumped {} of {total}, zenith has {}",
            vanilla.len(),
            blocks.len()
        );
    }
    let label = |col: usize| {
        format!(
            "{}+{}",
            Mirror::ALL[col / 4].name(),
            Rotation::ALL[col % 4].name()
        )
    };
    let mut col_ok = [0u64; 12];
    let mut col_fail = [0u64; 12];
    let mut rule_fail: BTreeMap<String, u64> = BTreeMap::new();
    let mut rule_checks: BTreeMap<String, u64> = BTreeMap::new();
    let mut changed = [0u64; 12];
    let mut failures: Vec<String> = Vec::new();
    for rec in &vanilla {
        let state = blocks
            .state_by_id_exact(rec.id as i32)
            .context("dump id outside the table")?;
        let rule = TransformRule::of_block(state.block_id.path())
            .map_or_else(|| "none".to_owned(), |r| format!("T{}", r.number()));
        for (col, &want) in rec.result.iter().enumerate() {
            let (m, r) = (Mirror::ALL[col / 4], Rotation::ALL[col % 4]);
            let got = transforms.transform(rec.id, m, r);
            *rule_checks.entry(rule.clone()).or_default() += 1;
            if want != rec.id {
                changed[col] += 1;
            }
            if got == want {
                col_ok[col] += 1;
                continue;
            }
            col_fail[col] += 1;
            *rule_fail.entry(rule.clone()).or_default() += 1;
            if failures.len() < limit {
                failures.push(format!(
                    "FAIL [{rule}] {} {}: vanilla {} | zenith {}",
                    describe_state(&blocks, rec.id),
                    label(col),
                    describe_state(&blocks, want),
                    describe_state(&blocks, got)
                ));
            }
        }
    }
    let passed: u64 = col_ok.iter().sum();
    let count: u64 = passed + col_fail.iter().sum::<u64>();
    println!(
        "vanilla dump: {} states; states changed: {}",
        vanilla.len(),
        (1..12)
            .map(|c| format!("{}={}", label(c), changed[c]))
            .collect::<Vec<_>>()
            .join(" ")
    );
    println!("block-transforms {version}: {} states", blocks.len());
    for col in 0..12 {
        println!(
            "  {:<34} {}/{}",
            label(col),
            col_ok[col],
            col_ok[col] + col_fail[col]
        );
    }
    println!("block-transforms {version}: {passed}/{count} checks");
    if !rule_fail.is_empty() {
        println!("failures per rule:");
        for (rule, n) in &rule_fail {
            println!("  {rule:<6} {n} of {}", rule_checks[rule]);
        }
    }
    for f in &failures {
        println!("  {f}");
    }
    if write {
        SuiteResult::new("block-transforms", version, passed, count)
            .write(&repo_root().join("progress/parity"))?;
    }
    Ok(passed == count)
}
