//! The per-block table (`docs/specs/blocks/block-table.md`), the sound-type table (`sound-types.md`) and the map
//! colour table (`state-facts.md` §4.1). The spec documents are the single source of truth: they are embedded at
//! compile time and parsed once.

use std::collections::HashMap;
use std::sync::OnceLock;

const BLOCK_TABLE: &str = include_str!("../../../docs/specs/blocks/block-table.md");
const SOUND_TABLE: &str = include_str!("../../../docs/specs/blocks/sound-types.md");
const STATE_FACTS: &str = include_str!("../../../docs/specs/blocks/state-facts.md");

/// How a block's map colour is picked from its state (`state-facts.md` §3.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapColorRule {
    /// Colour name used when no condition applies.
    pub base: String,
    /// `(condition, colour name)` used when the condition holds.
    pub alt: Option<(MapColorCond, String)>,
}

/// Per-state conditions of [`MapColorRule`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapColorCond {
    /// `axis` is `x` or `z`.
    AxisHorizontal,
    /// `part = head`.
    Head,
    /// `age >= 6`.
    AgeAtLeast6,
    /// `waterlogged = true`.
    Waterlogged,
}

/// How a block's light emission is derived from its state (`state-facts.md` §3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightRule {
    /// A constant.
    Const(u8),
    /// `lit ? n : 0`.
    Lit(u8),
    /// `lit ? 3 * candles : 0`.
    LitCandles,
    /// `berries ? 14 : 0`.
    Berries,
    /// `any face property ? 7 : 0`.
    AnyFace,
    /// The `level` property.
    Level,
    /// `waterlogged ? 3 + 3 * pickles : 0`.
    SeaPickle,
    /// `floor(charges / 4 * 15)` in single precision.
    RespawnAnchor,
    /// By `trial_spawner_state`.
    TrialSpawner,
    /// By `vault_state`.
    Vault,
}

/// Sound-type selection for a block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SoundRule {
    /// One named sound type.
    Fixed(String),
    /// `cracked ? A : B` (decorated pot).
    Cracked(String, String),
}

/// Redstone-conductor predicate (`cond:` flag).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConductorRule {
    /// Collision shape is a full block.
    Default,
    /// Never.
    Never,
    /// Always.
    Always,
}

/// Suffocation predicate (`suff:` flag).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuffocationRule {
    /// In `#causes_suffocation` and the collision shape is a full block.
    Default,
    /// Never.
    Never,
    /// Always.
    Always,
    /// Shulker box: true when there is no block entity.
    Shulker,
    /// Piston: not extended.
    Piston,
}

/// View-blocking predicate (`view:` flag).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewRule {
    /// The suffocation predicate.
    Default,
    /// Stored occlusion shape intersects the probe box (stairs, slabs).
    Plane,
    /// Snow layers ≥ 8.
    Snow8,
}

/// Position of the post-process hook (`pp:` flag).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostProcess {
    /// None.
    None,
    /// The position itself.
    Own,
    /// The position above.
    Above,
}

/// Emissive-rendering predicate (`emis:` flag).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmissiveRule {
    /// Never.
    Never,
    /// Always.
    Always,
    /// Sculk sensors in the `active` phase.
    SensorActive,
}

/// Offset type of a block (`off:` flag).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffsetType {
    /// No offset function.
    None,
    /// Horizontal offset.
    Xz,
    /// Horizontal and vertical offset.
    Xyz,
}

/// One row of `block-table.md` with the flags decoded.
#[derive(Debug, Clone)]
pub struct BlockInfo {
    /// Registry (protocol) index.
    pub index: u32,
    /// Block id without namespace.
    pub id: String,
    /// Number of states.
    pub states: u32,
    /// Game class name.
    pub class: String,
    /// Shape family key without its parenthesised variant (e.g. `skull`).
    pub family: String,
    /// The parenthesised variant of the family key (e.g. `creeper`, `h=7,w=10`), empty when none.
    pub variant: String,
    /// Map colour rule.
    pub map_color: MapColorRule,
    /// Destroy time.
    pub hardness: f32,
    /// Explosion resistance (as listed; clamped to ≥ 0 when read).
    pub resistance: f32,
    /// Sound type rule.
    pub sound: SoundRule,
    /// Note-block instrument name.
    pub instrument: String,
    /// Light emission rule.
    pub light: LightRule,
    /// `nc`: no collision (also cannot occlude).
    pub no_collision: bool,
    /// `no`: cannot occlude.
    pub no_occlusion: bool,
    /// `rt`: random ticks property.
    pub random_ticks: bool,
    /// `tool`: requires the correct tool.
    pub requires_tool: bool,
    /// `lava`: ignited by lava.
    pub ignited_by_lava: bool,
    /// `repl`: replaceable.
    pub replaceable: bool,
    /// `liq`: legacy liquid.
    pub liquid: bool,
    /// `air`.
    pub air: bool,
    /// `solid+`: force legacy solid on.
    pub force_solid_on: bool,
    /// `solid-`: force legacy solid off.
    pub force_solid_off: bool,
    /// `dyn`: dynamic shape.
    pub dynamic_shape: bool,
    /// `noparticles`.
    pub no_terrain_particles: bool,
    /// `cond:`.
    pub conductor: ConductorRule,
    /// `suff:`.
    pub suffocation: SuffocationRule,
    /// `view:`.
    pub view: ViewRule,
    /// `pp:`.
    pub post_process: PostProcess,
    /// `emis:`.
    pub emissive: EmissiveRule,
    /// `off:`.
    pub offset: OffsetType,
    /// Friction (default 0.6).
    pub friction: f32,
    /// Speed factor (default 1.0).
    pub speed_factor: f32,
    /// Jump factor (default 1.0).
    pub jump_factor: f32,
    /// Bounce restitution (default 0).
    pub bounce_restitution: f32,
    /// Fall-distance reduction (default 0).
    pub fall_distance_reduction: f32,
    /// Push reaction name (default `PUSH_PULL`).
    pub push_reaction: String,
}

/// One named sound type (`sound-types.md`).
#[derive(Debug, Clone)]
pub struct SoundTypeInfo {
    /// Upper-case name.
    pub name: String,
    /// Volume.
    pub volume: f32,
    /// Pitch.
    pub pitch: f32,
    /// Event ids with the `minecraft:` namespace: break, step, place, hit, fall.
    pub events: [String; 5],
}

/// One map colour (`state-facts.md` §4.1).
#[derive(Debug, Clone)]
pub struct MapColorInfo {
    /// Id (0..=61).
    pub id: u8,
    /// Name.
    pub name: String,
    /// `0xRRGGBB`.
    pub rgb: u32,
}

/// The parsed tables.
#[derive(Debug)]
pub struct Tables {
    /// Blocks in registry order.
    pub blocks: Vec<BlockInfo>,
    /// Sound types in table order.
    pub sounds: Vec<SoundTypeInfo>,
    /// Map colours by id.
    pub colors: Vec<MapColorInfo>,
    sound_index: HashMap<String, usize>,
    color_index: HashMap<String, usize>,
    block_index: HashMap<String, usize>,
}

impl Tables {
    /// The process-wide parsed tables.
    pub fn get() -> &'static Tables {
        static T: OnceLock<Tables> = OnceLock::new();
        T.get_or_init(|| Tables::parse().expect("embedded block tables are well formed"))
    }

    /// Looks up a block row by id (without namespace).
    pub fn block(&self, id: &str) -> Option<&BlockInfo> {
        self.block_index.get(id).map(|&i| &self.blocks[i])
    }

    /// Index of a sound type by name.
    pub fn sound_index(&self, name: &str) -> Option<usize> {
        self.sound_index.get(name).copied()
    }

    /// Index of a map colour by name.
    pub fn color_index(&self, name: &str) -> Option<usize> {
        self.color_index.get(name).copied()
    }

    fn parse() -> Result<Tables, String> {
        let blocks = parse_blocks()?;
        let sounds = parse_sounds()?;
        let colors = parse_colors()?;
        let sound_index = sounds
            .iter()
            .enumerate()
            .map(|(i, s)| (s.name.clone(), i))
            .collect();
        let color_index = colors
            .iter()
            .enumerate()
            .map(|(i, c)| (c.name.clone(), i))
            .collect();
        let block_index = blocks
            .iter()
            .enumerate()
            .map(|(i, b)| (b.id.clone(), i))
            .collect();
        Ok(Tables {
            blocks,
            sounds,
            colors,
            sound_index,
            color_index,
            block_index,
        })
    }
}

fn cells(line: &str) -> Vec<&str> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .collect()
}

fn parse_color_rule(s: &str) -> Result<MapColorRule, String> {
    let Some((base, rest)) = s.split_once(" (") else {
        return Ok(MapColorRule {
            base: s.to_owned(),
            alt: None,
        });
    };
    let rest = rest.trim_end_matches(')');
    let (cond, color) = rest
        .split_once(": ")
        .ok_or_else(|| format!("map colour rule {s:?}"))?;
    let cond = match cond {
        "axis=x/z" => MapColorCond::AxisHorizontal,
        "head" => MapColorCond::Head,
        "age ≥ 6" => MapColorCond::AgeAtLeast6,
        "waterlogged" => MapColorCond::Waterlogged,
        other => return Err(format!("unknown map colour condition {other:?}")),
    };
    Ok(MapColorRule {
        base: base.to_owned(),
        alt: Some((cond, color.to_owned())),
    })
}

fn parse_light(s: &str) -> Result<LightRule, String> {
    if let Ok(n) = s.parse::<u8>() {
        return Ok(LightRule::Const(n));
    }
    if let Some(n) = s.strip_prefix("lit?").and_then(|r| r.strip_suffix(":0"))
        && let Ok(n) = n.parse::<u8>()
    {
        return Ok(LightRule::Lit(n));
    }
    Ok(match s {
        "lit?3*candles:0" => LightRule::LitCandles,
        "berries?14:0" => LightRule::Berries,
        "anyface?7:0" => LightRule::AnyFace,
        "=level" => LightRule::Level,
        "waterlogged?3+3*pickles:0" => LightRule::SeaPickle,
        "floor(charge/4*15)" => LightRule::RespawnAnchor,
        "by trial_spawner_state" => LightRule::TrialSpawner,
        "by vault_state" => LightRule::Vault,
        other => return Err(format!("unknown light rule {other:?}")),
    })
}

fn parse_blocks() -> Result<Vec<BlockInfo>, String> {
    let mut out = Vec::new();
    for line in BLOCK_TABLE.lines() {
        let c = cells(line);
        if c.len() != 11 || c[0].parse::<u32>().is_err() {
            continue;
        }
        let (family_key, variant) = match c[4].split_once('(') {
            Some((f, v)) => (f, v.trim_end_matches(')')),
            None => (c[4], ""),
        };
        let (hard, res) = c[6]
            .split_once(" / ")
            .ok_or_else(|| format!("hardness cell {:?}", c[6]))?;
        let sound = match c[7].strip_prefix("cracked? ") {
            Some(rest) => {
                let (a, b) = rest.split_once(" : ").ok_or("cracked sound rule")?;
                SoundRule::Cracked(a.to_owned(), b.to_owned())
            }
            None => SoundRule::Fixed(c[7].to_owned()),
        };
        let mut info = BlockInfo {
            index: c[0].parse().map_err(|_| "index")?,
            id: c[1].to_owned(),
            states: c[2].parse().map_err(|_| "states")?,
            class: c[3].to_owned(),
            family: family_key.to_owned(),
            variant: variant.to_owned(),
            map_color: parse_color_rule(c[5])?,
            hardness: hard.parse().map_err(|_| format!("hardness {hard:?}"))?,
            resistance: res.parse().map_err(|_| format!("resistance {res:?}"))?,
            sound,
            instrument: c[8].to_owned(),
            light: parse_light(c[9])?,
            no_collision: false,
            no_occlusion: false,
            random_ticks: false,
            requires_tool: false,
            ignited_by_lava: false,
            replaceable: false,
            liquid: false,
            air: false,
            force_solid_on: false,
            force_solid_off: false,
            dynamic_shape: false,
            no_terrain_particles: false,
            conductor: ConductorRule::Default,
            suffocation: SuffocationRule::Default,
            view: ViewRule::Default,
            post_process: PostProcess::None,
            emissive: EmissiveRule::Never,
            offset: OffsetType::None,
            friction: 0.6,
            speed_factor: 1.0,
            jump_factor: 1.0,
            bounce_restitution: 0.0,
            fall_distance_reduction: 0.0,
            push_reaction: "PUSH_PULL".to_owned(),
        };
        for tok in c[10].split_whitespace() {
            let num = |t: &str| t.parse::<f32>().map_err(|_| format!("flag {tok:?}"));
            match tok {
                "nc" => info.no_collision = true,
                "no" => info.no_occlusion = true,
                "rt" => info.random_ticks = true,
                "tool" => info.requires_tool = true,
                "lava" => info.ignited_by_lava = true,
                "repl" => info.replaceable = true,
                "liq" => info.liquid = true,
                "air" => info.air = true,
                "solid+" => info.force_solid_on = true,
                "solid-" => info.force_solid_off = true,
                "dyn" => info.dynamic_shape = true,
                "noparticles" => info.no_terrain_particles = true,
                "cond:never" => info.conductor = ConductorRule::Never,
                "cond:always" => info.conductor = ConductorRule::Always,
                "suff:never" => info.suffocation = SuffocationRule::Never,
                "suff:always" => info.suffocation = SuffocationRule::Always,
                "suff:shulker" => info.suffocation = SuffocationRule::Shulker,
                "suff:piston" => info.suffocation = SuffocationRule::Piston,
                "view:plane" => info.view = ViewRule::Plane,
                "view:snow8" => info.view = ViewRule::Snow8,
                "pp:self" => info.post_process = PostProcess::Own,
                "pp:above" => info.post_process = PostProcess::Above,
                "emis:always" => info.emissive = EmissiveRule::Always,
                "emis:sensor-active" => info.emissive = EmissiveRule::SensorActive,
                "off:xz" => info.offset = OffsetType::Xz,
                "off:xyz" => info.offset = OffsetType::Xyz,
                t if t.starts_with("fr=") => info.friction = num(&t[3..])?,
                t if t.starts_with("spd=") => info.speed_factor = num(&t[4..])?,
                t if t.starts_with("jmp=") => info.jump_factor = num(&t[4..])?,
                t if t.starts_with("bnc=") => info.bounce_restitution = num(&t[4..])?,
                t if t.starts_with("fdr=") => info.fall_distance_reduction = num(&t[4..])?,
                t if t.starts_with("push:") => info.push_reaction = t[5..].to_owned(),
                other => return Err(format!("unknown flag {other:?} on block {}", info.id)),
            }
        }
        out.push(info);
    }
    if out.is_empty() {
        return Err("no block rows found".to_owned());
    }
    Ok(out)
}

fn parse_sounds() -> Result<Vec<SoundTypeInfo>, String> {
    let mut out = Vec::new();
    for line in SOUND_TABLE.lines() {
        let c = cells(line);
        if c.len() != 8 || c[1].parse::<f32>().is_err() {
            continue;
        }
        let ev = |s: &str| format!("minecraft:{s}");
        out.push(SoundTypeInfo {
            name: c[0].to_owned(),
            volume: c[1].parse().map_err(|_| "volume")?,
            pitch: c[2].parse().map_err(|_| "pitch")?,
            events: [ev(c[3]), ev(c[4]), ev(c[5]), ev(c[6]), ev(c[7])],
        });
    }
    if out.is_empty() {
        return Err("no sound rows found".to_owned());
    }
    Ok(out)
}

fn parse_colors() -> Result<Vec<MapColorInfo>, String> {
    let mut out: Vec<MapColorInfo> = Vec::new();
    for line in STATE_FACTS.lines() {
        let c = cells(line);
        if c.len() != 6 || c[0].parse::<u8>().is_err() || c[2].len() != 6 {
            continue;
        }
        for pair in c.chunks(3) {
            out.push(MapColorInfo {
                id: pair[0].parse().map_err(|_| "colour id")?,
                name: pair[1].to_owned(),
                rgb: u32::from_str_radix(pair[2], 16).map_err(|_| "colour rgb")?,
            });
        }
    }
    out.sort_by_key(|c| c.id);
    if out.is_empty() || out.iter().enumerate().any(|(i, c)| usize::from(c.id) != i) {
        return Err("map colour table is not contiguous".to_owned());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_parse() {
        let t = Tables::get();
        assert_eq!(t.blocks.len(), 1286);
        assert_eq!(
            t.blocks.iter().map(|b| u64::from(b.states)).sum::<u64>(),
            35_723
        );
        assert_eq!(t.sounds.len(), 131);
        assert_eq!(t.colors.len(), 62);
        assert_eq!(t.color_index("GLOW_LICHEN"), Some(61));
        assert_eq!(t.colors[11].rgb, 0x70_70_70);
        let stone = t.block("stone").unwrap();
        assert_eq!(stone.family, "cube");
        assert!(stone.requires_tool);
        assert_eq!(stone.hardness, 1.5);
        let cluster = t.block("amethyst_cluster").unwrap();
        assert_eq!(
            (cluster.family.as_str(), cluster.variant.as_str()),
            ("amethyst_cluster", "h=7,w=10")
        );
        // Every referenced sound type and colour exists.
        for b in &t.blocks {
            match &b.sound {
                SoundRule::Fixed(s) => assert!(t.sound_index(s).is_some(), "{} {s}", b.id),
                SoundRule::Cracked(a, c) => {
                    assert!(t.sound_index(a).is_some() && t.sound_index(c).is_some())
                }
            }
            assert!(
                t.color_index(&b.map_color.base).is_some(),
                "{} {}",
                b.id,
                b.map_color.base
            );
            if let Some((_, alt)) = &b.map_color.alt {
                assert!(t.color_index(alt).is_some());
            }
        }
    }
}
