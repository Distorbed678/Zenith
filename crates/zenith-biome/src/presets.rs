//! Procedural builders for vanilla multi-noise presets (`minecraft:overworld` and
//! `minecraft:nether`) and codec for `minecraft:worldgen/multi_noise_biome_source_parameter_list`.
//!
//! Implements Section 2 (parameter-list registry schema) and Section 6 of
//! `docs/specs/worldgen/biomes-and-climate.md`.

use zenith_codec::codec::{Codec, DataResult, IdentifierCodec, RecordReader, RecordWriter};
use zenith_codec::json::Json;
use zenith_registry::Identifier;

use crate::climate::{Parameter, ParameterPoint};
use crate::rtree::ParameterList;

/// Expected entry count of the 26.3 Overworld preset (`22 + 7568 + 4 = 7594`).
pub const OVERWORLD_PRESET_ENTRY_COUNT: usize = 7_594;

/// Expected entry count of the 26.3 Nether preset (`5`).
pub const NETHER_PRESET_ENTRY_COUNT: usize = 5;

/// Built-in `minecraft:worldgen/multi_noise_biome_source_parameter_list` presets in 26.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MultiNoisePreset {
    /// `minecraft:overworld` (7,594 entries).
    Overworld,
    /// `minecraft:nether` (5 entries).
    Nether,
}

impl MultiNoisePreset {
    /// Canonical registry identifier for this preset.
    pub fn id(self) -> Identifier {
        match self {
            Self::Overworld => {
                Identifier::with_default_namespace("overworld").expect("valid identifier")
            }
            Self::Nether => Identifier::with_default_namespace("nether").expect("valid identifier"),
        }
    }

    /// Looks up a built-in preset by identifier (`minecraft:overworld` or `minecraft:nether`).
    pub fn from_id(id: &Identifier) -> Option<Self> {
        if id.namespace() != "minecraft" {
            return None;
        }
        match id.path() {
            "overworld" => Some(Self::Overworld),
            "nether" => Some(Self::Nether),
            _ => None,
        }
    }

    /// Generates the exact ordered `(ParameterPoint, Identifier)` list for this preset.
    pub fn entries(self) -> Vec<(ParameterPoint, Identifier)> {
        match self {
            Self::Overworld => overworld_preset_entries(),
            Self::Nether => nether_preset_entries(),
        }
    }

    /// Compiles this preset into its 19-way 7D R-tree [`ParameterList`].
    pub fn build_parameter_list(self) -> ParameterList<Identifier> {
        ParameterList::new(self.entries()).expect("built-in presets are always non-empty")
    }
}

/// JSON codec for a `minecraft:worldgen/multi_noise_biome_source_parameter_list` entry:
/// `{"preset": "minecraft:overworld" | "minecraft:nether"}`.
#[derive(Debug, Clone, Copy, Default)]
pub struct MultiNoisePresetCodec;

impl Codec<MultiNoisePreset> for MultiNoisePresetCodec {
    fn decode(&self, input: &Json) -> DataResult<MultiNoisePreset> {
        let mut r = RecordReader::new(input);
        let preset_id = r.required("preset", &IdentifierCodec);
        r.finish(|| preset_id).and_then(|id| {
            MultiNoisePreset::from_id(&id)
                .map(DataResult::Success)
                .unwrap_or_else(|| {
                    DataResult::error(format!("Unknown multi-noise biome preset: {id}"))
                })
        })
    }

    fn encode(&self, value: &MultiNoisePreset) -> DataResult<Json> {
        let mut w = RecordWriter::new();
        w.field("preset", &IdentifierCodec, &value.id());
        w.finish()
    }
}

/// Builds the exact 5-entry Nether preset parameter list (`minecraft:nether`, spec §6.1).
pub fn nether_preset_entries() -> Vec<(ParameterPoint, Identifier)> {
    let z = Parameter::point(0.0_f32);
    let mk = |t: f32, h: f32, offset: f32, name: &str| {
        (
            ParameterPoint::new(Parameter::point(t), Parameter::point(h), z, z, z, z, offset),
            mc(name),
        )
    };
    vec![
        mk(0.0, 0.0, 0.0, "nether_wastes"),
        mk(0.0, -0.5, 0.0, "soul_sand_valley"),
        mk(0.4, 0.0, 0.0, "crimson_forest"),
        mk(0.0, 0.5, 0.375, "warped_forest"),
        mk(-0.5, 0.0, 0.175, "basalt_deltas"),
    ]
}

/// Compiles the `minecraft:nether` preset into its [`ParameterList`].
pub fn nether_parameter_list() -> ParameterList<Identifier> {
    MultiNoisePreset::Nether.build_parameter_list()
}

/// Builds the exact 7,594-entry Overworld preset parameter list (`minecraft:overworld`, spec §6.2).
pub fn overworld_preset_entries() -> Vec<(ParameterPoint, Identifier)> {
    let builder = OverworldBuilder::new();
    let mut out = Vec::with_capacity(OVERWORLD_PRESET_ENTRY_COUNT);
    builder.emit_all(&mut out);
    debug_assert_eq!(out.len(), OVERWORLD_PRESET_ENTRY_COUNT);
    out
}

/// Compiles the `minecraft:overworld` preset into its [`ParameterList`].
pub fn overworld_parameter_list() -> ParameterList<Identifier> {
    MultiNoisePreset::Overworld.build_parameter_list()
}

#[inline]
fn mc(path: &str) -> Identifier {
    Identifier::with_default_namespace(path).expect("built-in biome identifier must be valid")
}

#[inline]
fn span(lo: f32, hi: f32) -> Parameter {
    Parameter::span(lo, hi).expect("built-in parameter span must have lo <= hi")
}

#[inline]
fn span_p(a: Parameter, b: Parameter) -> Parameter {
    Parameter::span_params(a, b).expect("built-in parameter span must have a.min <= b.max")
}

const OCEANS: [[&str; 5]; 2] = [
    [
        "deep_frozen_ocean",
        "deep_cold_ocean",
        "deep_ocean",
        "deep_lukewarm_ocean",
        "warm_ocean",
    ],
    [
        "frozen_ocean",
        "cold_ocean",
        "ocean",
        "lukewarm_ocean",
        "warm_ocean",
    ],
];

const MIDDLE: [[&str; 5]; 5] = [
    [
        "snowy_plains",
        "snowy_plains",
        "snowy_plains",
        "snowy_taiga",
        "taiga",
    ],
    [
        "plains",
        "plains",
        "forest",
        "taiga",
        "old_growth_spruce_taiga",
    ],
    [
        "flower_forest",
        "plains",
        "forest",
        "birch_forest",
        "dark_forest",
    ],
    ["savanna", "savanna", "forest", "jungle", "jungle"],
    ["desert", "desert", "desert", "desert", "desert"],
];

const MIDDLE_VARIANT: [[Option<&str>; 5]; 5] = [
    [Some("ice_spikes"), None, Some("snowy_taiga"), None, None],
    [
        Some("dappled_forest"),
        None,
        None,
        None,
        Some("old_growth_pine_taiga"),
    ],
    [
        Some("sunflower_plains"),
        None,
        None,
        Some("old_growth_birch_forest"),
        None,
    ],
    [
        None,
        None,
        Some("plains"),
        Some("sparse_jungle"),
        Some("bamboo_jungle"),
    ],
    [None, None, None, None, None],
];

const PLATEAU: [[&str; 5]; 5] = [
    [
        "snowy_plains",
        "snowy_plains",
        "snowy_plains",
        "snowy_taiga",
        "snowy_taiga",
    ],
    [
        "meadow",
        "meadow",
        "forest",
        "taiga",
        "old_growth_spruce_taiga",
    ],
    ["meadow", "meadow", "meadow", "meadow", "pale_garden"],
    [
        "savanna_plateau",
        "savanna_plateau",
        "forest",
        "forest",
        "jungle",
    ],
    [
        "badlands",
        "badlands",
        "badlands",
        "wooded_badlands",
        "wooded_badlands",
    ],
];

const PLATEAU_VARIANT: [[Option<&str>; 5]; 5] = [
    [Some("ice_spikes"), None, None, None, None],
    [
        Some("cherry_grove"),
        None,
        Some("meadow"),
        Some("meadow"),
        Some("old_growth_pine_taiga"),
    ],
    [
        Some("cherry_grove"),
        Some("cherry_grove"),
        Some("forest"),
        Some("birch_forest"),
        None,
    ],
    [None, None, None, None, None],
    [
        Some("eroded_badlands"),
        Some("eroded_badlands"),
        None,
        None,
        None,
    ],
];

const SHATTERED: [[Option<&str>; 5]; 5] = [
    [
        Some("windswept_gravelly_hills"),
        Some("windswept_gravelly_hills"),
        Some("windswept_hills"),
        Some("windswept_forest"),
        Some("windswept_forest"),
    ],
    [
        Some("windswept_gravelly_hills"),
        Some("windswept_gravelly_hills"),
        Some("windswept_hills"),
        Some("windswept_forest"),
        Some("windswept_forest"),
    ],
    [
        Some("windswept_hills"),
        Some("windswept_hills"),
        Some("windswept_hills"),
        Some("windswept_forest"),
        Some("windswept_forest"),
    ],
    [None, None, None, None, None],
    [None, None, None, None, None],
];

struct OverworldBuilder {
    full: Parameter,
    t: [Parameter; 5],
    unfrozen: Parameter,
    h: [Parameter; 5],
    e: [Parameter; 7],
    c_mushroom: Parameter,
    c_deep_ocean: Parameter,
    c_ocean: Parameter,
    c_coast: Parameter,
    c_inland: Parameter,
    c_near: Parameter,
    c_mid: Parameter,
    c_far: Parameter,
}

impl OverworldBuilder {
    fn new() -> Self {
        let t = [
            span(-1.0, -0.45),
            span(-0.45, -0.15),
            span(-0.15, 0.2),
            span(0.2, 0.55),
            span(0.55, 1.0),
        ];
        let unfrozen = span_p(t[1], t[4]);
        let h = [
            span(-1.0, -0.35),
            span(-0.35, -0.1),
            span(-0.1, 0.1),
            span(0.1, 0.3),
            span(0.3, 1.0),
        ];
        let e = [
            span(-1.0, -0.78),
            span(-0.78, -0.375),
            span(-0.375, -0.2225),
            span(-0.2225, 0.05),
            span(0.05, 0.45),
            span(0.45, 0.55),
            span(0.55, 1.0),
        ];
        Self {
            full: span(-1.0, 1.0),
            t,
            unfrozen,
            h,
            e,
            c_mushroom: span(-1.2, -1.05),
            c_deep_ocean: span(-1.05, -0.455),
            c_ocean: span(-0.455, -0.19),
            c_coast: span(-0.19, -0.11),
            c_inland: span(-0.11, 0.55),
            c_near: span(-0.11, 0.03),
            c_mid: span(0.03, 0.3),
            c_far: span(0.3, 1.0),
        }
    }

    fn emit_all(&self, out: &mut Vec<(ParameterPoint, Identifier)>) {
        // Phase 1: Off-coast biomes (22 entries).
        self.emit_surface(
            out,
            self.full,
            self.full,
            self.c_mushroom,
            self.full,
            self.full,
            "mushroom_fields",
        );
        for (t, &temp) in self.t.iter().enumerate() {
            self.emit_surface(
                out,
                temp,
                self.full,
                self.c_deep_ocean,
                self.full,
                self.full,
                OCEANS[0][t],
            );
            self.emit_surface(
                out,
                temp,
                self.full,
                self.c_ocean,
                self.full,
                self.full,
                OCEANS[1][t],
            );
        }

        // Phase 2: Inland biomes across 13 ordered weirdness slices (7,568 entries).
        self.emit_mid_slice(out, span(-1.0, -0.93333334));
        self.emit_high_slice(out, span(-0.93333334, -0.7666667));
        self.emit_peaks_slice(out, span(-0.7666667, -0.56666666));
        self.emit_high_slice(out, span(-0.56666666, -0.4));
        self.emit_mid_slice(out, span(-0.4, -0.26666668));
        self.emit_low_slice(out, span(-0.26666668, -0.05));
        self.emit_valleys_slice(out, span(-0.05, 0.05));
        self.emit_low_slice(out, span(0.05, 0.26666668));
        self.emit_mid_slice(out, span(0.26666668, 0.4));
        self.emit_high_slice(out, span(0.4, 0.56666666));
        self.emit_peaks_slice(out, span(0.56666666, 0.7666667));
        self.emit_high_slice(out, span(0.7666667, 0.93333334));
        self.emit_mid_slice(out, span(0.93333334, 1.0));

        // Phase 3: Underground biomes (4 entries).
        self.emit_underground(
            out,
            self.full,
            self.full,
            span(0.8, 1.0),
            self.full,
            self.full,
            "dripstone_caves",
        );
        self.emit_underground(
            out,
            self.full,
            span(0.7, 1.0),
            self.full,
            self.full,
            self.full,
            "lush_caves",
        );
        self.emit_underground(
            out,
            self.full,
            self.full,
            span_p(self.c_coast, self.c_inland),
            span_p(self.e[5], self.e[6]),
            span(-1.1, -0.85),
            "sulfur_caves",
        );
        self.emit_bottom(
            out,
            self.full,
            self.full,
            self.full,
            span_p(self.e[0], self.e[1]),
            self.full,
            "deep_dark",
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_surface(
        &self,
        out: &mut Vec<(ParameterPoint, Identifier)>,
        t: Parameter,
        h: Parameter,
        c: Parameter,
        e: Parameter,
        w: Parameter,
        biome: &str,
    ) {
        let id = mc(biome);
        out.push((
            ParameterPoint::new(t, h, c, e, Parameter::point(0.0), w, 0.0),
            id.clone(),
        ));
        out.push((
            ParameterPoint::new(t, h, c, e, Parameter::point(1.0), w, 0.0),
            id,
        ));
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_underground(
        &self,
        out: &mut Vec<(ParameterPoint, Identifier)>,
        t: Parameter,
        h: Parameter,
        c: Parameter,
        e: Parameter,
        w: Parameter,
        biome: &str,
    ) {
        out.push((
            ParameterPoint::new(t, h, c, e, span(0.2, 0.9), w, 0.0),
            mc(biome),
        ));
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_bottom(
        &self,
        out: &mut Vec<(ParameterPoint, Identifier)>,
        t: Parameter,
        h: Parameter,
        c: Parameter,
        e: Parameter,
        w: Parameter,
        biome: &str,
    ) {
        out.push((
            ParameterPoint::new(t, h, c, e, Parameter::point(1.1), w, 0.0),
            mc(biome),
        ));
    }

    fn emit_peaks_slice(&self, out: &mut Vec<(ParameterPoint, Identifier)>, w: Parameter) {
        let c_coast_far = span_p(self.c_coast, self.c_far);
        let c_coast_near = span_p(self.c_coast, self.c_near);
        let c_mid_far = span_p(self.c_mid, self.c_far);
        let e23 = span_p(self.e[2], self.e[3]);

        for t in 0..5 {
            let tp = self.t[t];
            for h in 0..5 {
                let hp = self.h[h];
                let middle = pick_middle(t, h, w);
                let middle_or_badlands = pick_middle_or_badlands(t, h, w);
                let middle_or_badlands_or_slope = pick_middle_or_badlands_or_slope(t, h, w);
                let plateau = pick_plateau(t, h, w);
                let shattered = pick_shattered(t, h, w);
                let windswept_or_shattered = maybe_windswept_savanna(t, h, w, shattered);
                let peak = pick_peak(t, h, w);

                self.emit_surface(out, tp, hp, c_coast_far, self.e[0], w, peak);
                self.emit_surface(
                    out,
                    tp,
                    hp,
                    c_coast_near,
                    self.e[1],
                    w,
                    middle_or_badlands_or_slope,
                );
                self.emit_surface(out, tp, hp, c_mid_far, self.e[1], w, peak);
                self.emit_surface(out, tp, hp, c_coast_near, e23, w, middle);
                self.emit_surface(out, tp, hp, c_mid_far, self.e[2], w, plateau);
                self.emit_surface(out, tp, hp, self.c_mid, self.e[3], w, middle_or_badlands);
                self.emit_surface(out, tp, hp, self.c_far, self.e[3], w, plateau);
                self.emit_surface(out, tp, hp, c_coast_far, self.e[4], w, middle);
                self.emit_surface(
                    out,
                    tp,
                    hp,
                    c_coast_near,
                    self.e[5],
                    w,
                    windswept_or_shattered,
                );
                self.emit_surface(out, tp, hp, c_mid_far, self.e[5], w, shattered);
                self.emit_surface(out, tp, hp, c_coast_far, self.e[6], w, middle);
            }
        }
    }

    fn emit_high_slice(&self, out: &mut Vec<(ParameterPoint, Identifier)>, w: Parameter) {
        let c_coast_far = span_p(self.c_coast, self.c_far);
        let c_coast_near = span_p(self.c_coast, self.c_near);
        let c_mid_far = span_p(self.c_mid, self.c_far);
        let e01 = span_p(self.e[0], self.e[1]);
        let e23 = span_p(self.e[2], self.e[3]);

        for t in 0..5 {
            let tp = self.t[t];
            for h in 0..5 {
                let hp = self.h[h];
                let middle = pick_middle(t, h, w);
                let middle_or_badlands = pick_middle_or_badlands(t, h, w);
                let middle_or_badlands_or_slope = pick_middle_or_badlands_or_slope(t, h, w);
                let plateau = pick_plateau(t, h, w);
                let slope = pick_slope(t, h, w);
                let shattered = pick_shattered(t, h, w);
                let windswept_or_middle = maybe_windswept_savanna(t, h, w, middle);
                let peak = pick_peak(t, h, w);

                self.emit_surface(out, tp, hp, self.c_coast, e01, w, middle);
                self.emit_surface(out, tp, hp, self.c_near, self.e[0], w, slope);
                self.emit_surface(out, tp, hp, c_mid_far, self.e[0], w, peak);
                self.emit_surface(
                    out,
                    tp,
                    hp,
                    self.c_near,
                    self.e[1],
                    w,
                    middle_or_badlands_or_slope,
                );
                self.emit_surface(out, tp, hp, c_mid_far, self.e[1], w, slope);
                self.emit_surface(out, tp, hp, c_coast_near, e23, w, middle);
                self.emit_surface(out, tp, hp, c_mid_far, self.e[2], w, plateau);
                self.emit_surface(out, tp, hp, self.c_mid, self.e[3], w, middle_or_badlands);
                self.emit_surface(out, tp, hp, self.c_far, self.e[3], w, plateau);
                self.emit_surface(out, tp, hp, c_coast_far, self.e[4], w, middle);
                self.emit_surface(out, tp, hp, c_coast_near, self.e[5], w, windswept_or_middle);
                self.emit_surface(out, tp, hp, c_mid_far, self.e[5], w, shattered);
                self.emit_surface(out, tp, hp, c_coast_far, self.e[6], w, middle);
            }
        }
    }

    fn emit_mid_slice(&self, out: &mut Vec<(ParameterPoint, Identifier)>, w: Parameter) {
        let neg_w = w.max < 0;
        let c_near_far = span_p(self.c_near, self.c_far);
        let c_near_mid = span_p(self.c_near, self.c_mid);
        let c_coast_near = span_p(self.c_coast, self.c_near);
        let c_coast_far = span_p(self.c_coast, self.c_far);
        let c_mid_far = span_p(self.c_mid, self.c_far);

        // Pre-loop (3 calls).
        self.emit_surface(
            out,
            self.full,
            self.full,
            self.c_coast,
            span_p(self.e[0], self.e[2]),
            w,
            "stony_shore",
        );
        self.emit_surface(
            out,
            span_p(self.t[1], self.t[2]),
            self.full,
            c_near_far,
            self.e[6],
            w,
            "swamp",
        );
        self.emit_surface(
            out,
            span_p(self.t[3], self.t[4]),
            self.full,
            c_near_far,
            self.e[6],
            w,
            "mangrove_swamp",
        );

        for t in 0..5 {
            let tp = self.t[t];
            for h in 0..5 {
                let hp = self.h[h];
                let middle = pick_middle(t, h, w);
                let middle_or_badlands = pick_middle_or_badlands(t, h, w);
                let middle_or_badlands_or_slope = pick_middle_or_badlands_or_slope(t, h, w);
                let slope = pick_slope(t, h, w);
                let plateau = pick_plateau(t, h, w);
                let beach = pick_beach(t);
                let shattered_coast = pick_shattered_coast(t, h, w);
                let windswept_or_middle = maybe_windswept_savanna(t, h, w, middle);
                let shattered = pick_shattered(t, h, w);

                self.emit_surface(out, tp, hp, c_near_far, self.e[0], w, slope);
                self.emit_surface(
                    out,
                    tp,
                    hp,
                    c_near_mid,
                    self.e[1],
                    w,
                    middle_or_badlands_or_slope,
                );
                self.emit_surface(
                    out,
                    tp,
                    hp,
                    self.c_far,
                    self.e[1],
                    w,
                    if t == 0 { slope } else { plateau },
                );
                self.emit_surface(out, tp, hp, self.c_near, self.e[2], w, middle);
                self.emit_surface(out, tp, hp, self.c_mid, self.e[2], w, middle_or_badlands);
                self.emit_surface(out, tp, hp, self.c_far, self.e[2], w, plateau);
                self.emit_surface(out, tp, hp, c_coast_near, self.e[3], w, middle);
                self.emit_surface(out, tp, hp, c_mid_far, self.e[3], w, middle_or_badlands);
                if neg_w {
                    self.emit_surface(out, tp, hp, self.c_coast, self.e[4], w, beach);
                    self.emit_surface(out, tp, hp, c_near_far, self.e[4], w, middle);
                } else {
                    self.emit_surface(out, tp, hp, c_coast_far, self.e[4], w, middle);
                }
                self.emit_surface(out, tp, hp, self.c_coast, self.e[5], w, shattered_coast);
                self.emit_surface(out, tp, hp, self.c_near, self.e[5], w, windswept_or_middle);
                self.emit_surface(out, tp, hp, c_mid_far, self.e[5], w, shattered);
                self.emit_surface(
                    out,
                    tp,
                    hp,
                    self.c_coast,
                    self.e[6],
                    w,
                    if neg_w { beach } else { middle },
                );
                if t == 0 {
                    self.emit_surface(out, tp, hp, c_near_far, self.e[6], w, middle);
                }
            }
        }
    }

    fn emit_low_slice(&self, out: &mut Vec<(ParameterPoint, Identifier)>, w: Parameter) {
        let c_near_far = span_p(self.c_near, self.c_far);
        let c_mid_far = span_p(self.c_mid, self.c_far);
        let e01 = span_p(self.e[0], self.e[1]);
        let e23 = span_p(self.e[2], self.e[3]);
        let e34 = span_p(self.e[3], self.e[4]);

        // Pre-loop (3 calls).
        self.emit_surface(
            out,
            self.full,
            self.full,
            self.c_coast,
            span_p(self.e[0], self.e[2]),
            w,
            "stony_shore",
        );
        self.emit_surface(
            out,
            span_p(self.t[1], self.t[2]),
            self.full,
            c_near_far,
            self.e[6],
            w,
            "swamp",
        );
        self.emit_surface(
            out,
            span_p(self.t[3], self.t[4]),
            self.full,
            c_near_far,
            self.e[6],
            w,
            "mangrove_swamp",
        );

        for t in 0..5 {
            let tp = self.t[t];
            for h in 0..5 {
                let hp = self.h[h];
                let middle = pick_middle(t, h, w);
                let middle_or_badlands = pick_middle_or_badlands(t, h, w);
                let middle_or_badlands_or_slope = pick_middle_or_badlands_or_slope(t, h, w);
                let beach = pick_beach(t);
                let windswept_or_middle = maybe_windswept_savanna(t, h, w, middle);
                let shattered_coast = pick_shattered_coast(t, h, w);

                self.emit_surface(out, tp, hp, self.c_near, e01, w, middle_or_badlands);
                self.emit_surface(out, tp, hp, c_mid_far, e01, w, middle_or_badlands_or_slope);
                self.emit_surface(out, tp, hp, self.c_near, e23, w, middle);
                self.emit_surface(out, tp, hp, c_mid_far, e23, w, middle_or_badlands);
                self.emit_surface(out, tp, hp, self.c_coast, e34, w, beach);
                self.emit_surface(out, tp, hp, c_near_far, self.e[4], w, middle);
                self.emit_surface(out, tp, hp, self.c_coast, self.e[5], w, shattered_coast);
                self.emit_surface(out, tp, hp, self.c_near, self.e[5], w, windswept_or_middle);
                self.emit_surface(out, tp, hp, c_mid_far, self.e[5], w, middle);
                self.emit_surface(out, tp, hp, self.c_coast, self.e[6], w, beach);
                if t == 0 {
                    self.emit_surface(out, tp, hp, c_near_far, self.e[6], w, middle);
                }
            }
        }
    }

    fn emit_valleys_slice(&self, out: &mut Vec<(ParameterPoint, Identifier)>, w: Parameter) {
        let neg_w = w.max < 0;
        let frozen = self.t[0];
        let e01 = span_p(self.e[0], self.e[1]);
        let e25 = span_p(self.e[2], self.e[5]);
        let c_coast_far = span_p(self.c_coast, self.c_far);
        let c_inland_far = span_p(self.c_inland, self.c_far);
        let c_mid_far = span_p(self.c_mid, self.c_far);

        // Pre-loop (11 calls).
        self.emit_surface(
            out,
            frozen,
            self.full,
            self.c_coast,
            e01,
            w,
            if neg_w { "stony_shore" } else { "frozen_river" },
        );
        self.emit_surface(
            out,
            self.unfrozen,
            self.full,
            self.c_coast,
            e01,
            w,
            if neg_w { "stony_shore" } else { "river" },
        );
        self.emit_surface(out, frozen, self.full, self.c_near, e01, w, "frozen_river");
        self.emit_surface(out, self.unfrozen, self.full, self.c_near, e01, w, "river");
        self.emit_surface(out, frozen, self.full, c_coast_far, e25, w, "frozen_river");
        self.emit_surface(out, self.unfrozen, self.full, c_coast_far, e25, w, "river");
        self.emit_surface(
            out,
            frozen,
            self.full,
            self.c_coast,
            self.e[6],
            w,
            "frozen_river",
        );
        self.emit_surface(
            out,
            self.unfrozen,
            self.full,
            self.c_coast,
            self.e[6],
            w,
            "river",
        );
        self.emit_surface(
            out,
            span_p(self.t[1], self.t[2]),
            self.full,
            c_inland_far,
            self.e[6],
            w,
            "swamp",
        );
        self.emit_surface(
            out,
            span_p(self.t[3], self.t[4]),
            self.full,
            c_inland_far,
            self.e[6],
            w,
            "mangrove_swamp",
        );
        self.emit_surface(
            out,
            frozen,
            self.full,
            c_inland_far,
            self.e[6],
            w,
            "frozen_river",
        );

        for t in 0..5 {
            let tp = self.t[t];
            for h in 0..5 {
                let hp = self.h[h];
                let biome = pick_middle_or_badlands(t, h, w);
                self.emit_surface(out, tp, hp, c_mid_far, e01, w, biome);
            }
        }
    }
}

#[inline]
fn pick_middle(t: usize, h: usize, w: Parameter) -> &'static str {
    if w.max < 0 {
        MIDDLE[t][h]
    } else {
        MIDDLE_VARIANT[t][h].unwrap_or(MIDDLE[t][h])
    }
}

#[inline]
fn pick_badlands(h: usize, w: Parameter) -> &'static str {
    if h < 2 {
        if w.max < 0 {
            "badlands"
        } else {
            "eroded_badlands"
        }
    } else if h < 3 {
        "badlands"
    } else {
        "wooded_badlands"
    }
}

#[inline]
fn pick_middle_or_badlands(t: usize, h: usize, w: Parameter) -> &'static str {
    if t == 4 {
        pick_badlands(h, w)
    } else {
        pick_middle(t, h, w)
    }
}

#[inline]
fn pick_plateau(t: usize, h: usize, w: Parameter) -> &'static str {
    if w.max >= 0
        && let Some(variant) = PLATEAU_VARIANT[t][h]
    {
        return variant;
    }
    PLATEAU[t][h]
}

#[inline]
fn pick_slope(t: usize, h: usize, w: Parameter) -> &'static str {
    if t >= 3 {
        pick_plateau(t, h, w)
    } else if h <= 1 {
        "snowy_slopes"
    } else {
        "grove"
    }
}

#[inline]
fn pick_middle_or_badlands_or_slope(t: usize, h: usize, w: Parameter) -> &'static str {
    if t == 0 {
        pick_slope(t, h, w)
    } else {
        pick_middle_or_badlands(t, h, w)
    }
}

#[inline]
fn pick_peak(t: usize, h: usize, w: Parameter) -> &'static str {
    if t <= 2 {
        if w.max < 0 {
            "jagged_peaks"
        } else {
            "frozen_peaks"
        }
    } else if t == 3 {
        "stony_peaks"
    } else {
        pick_badlands(h, w)
    }
}

#[inline]
fn pick_shattered(t: usize, h: usize, w: Parameter) -> &'static str {
    SHATTERED[t][h].unwrap_or_else(|| pick_middle(t, h, w))
}

#[inline]
fn maybe_windswept_savanna(t: usize, h: usize, w: Parameter, base: &'static str) -> &'static str {
    if t > 1 && h < 4 && w.max >= 0 {
        "windswept_savanna"
    } else {
        base
    }
}

#[inline]
fn pick_beach(t: usize) -> &'static str {
    if t == 0 {
        "snowy_beach"
    } else if t == 4 {
        "desert"
    } else {
        "beach"
    }
}

#[inline]
fn pick_shattered_coast(t: usize, h: usize, w: Parameter) -> &'static str {
    let base = if w.max >= 0 {
        pick_middle(t, h, w)
    } else {
        pick_beach(t)
    };
    maybe_windswept_savanna(t, h, w, base)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use zenith_codec::codec::{Codec, IdentifierCodec, RecordReader, list};
    use zenith_codec::json::{ParseMode, parse_bytes};

    use super::*;
    use crate::climate::ParameterPointCodec;

    fn load_report_entries(path: &Path) -> Vec<(ParameterPoint, Identifier)> {
        struct EntryCodec;
        impl Codec<(ParameterPoint, Identifier)> for EntryCodec {
            fn decode(&self, input: &Json) -> DataResult<(ParameterPoint, Identifier)> {
                let mut r = RecordReader::new(input);
                let params = r.required("parameters", &ParameterPointCodec);
                let biome = r.required("biome", &IdentifierCodec);
                r.finish(|| Some((params?, biome?)))
            }
            fn encode(&self, _: &(ParameterPoint, Identifier)) -> DataResult<Json> {
                unreachable!()
            }
        }

        let bytes = fs::read(path).expect("read biome parameters report");
        let json = parse_bytes(&bytes, ParseMode::Data).expect("parse biome parameters report");
        let mut r = RecordReader::new(&json);
        let biomes = r.required("biomes", &list(EntryCodec));
        r.finish(|| biomes)
            .demand()
            .expect("decode biome parameters report")
    }

    #[test]
    fn preset_entry_counts_and_codec() {
        let nether = nether_preset_entries();
        assert_eq!(nether.len(), NETHER_PRESET_ENTRY_COUNT);
        let overworld = overworld_preset_entries();
        assert_eq!(overworld.len(), OVERWORLD_PRESET_ENTRY_COUNT);

        // Build both R-trees cleanly.
        let _ = nether_parameter_list();
        let _ = overworld_parameter_list();

        // Preset JSON codec round-trip and unknown rejection.
        let j = parse_bytes(br#"{"preset": "minecraft:overworld"}"#, ParseMode::Data).unwrap();
        assert_eq!(
            MultiNoisePresetCodec.decode(&j).demand().unwrap(),
            MultiNoisePreset::Overworld
        );
        let bad = parse_bytes(br#"{"preset": "minecraft:unknown"}"#, ParseMode::Data).unwrap();
        assert!(MultiNoisePresetCodec.decode(&bad).demand().is_err());
    }

    #[test]
    fn nether_preset_matches_datagen_report_if_present() {
        let report_path = zenith_data::version_dir("26.3")
            .join("reports")
            .join("biome_parameters")
            .join("minecraft")
            .join("nether.json");
        if !report_path.is_file() {
            eprintln!(
                "NOTE: skipping nether preset report cross-check (absent: {})",
                report_path.display()
            );
            return;
        }
        let expected = load_report_entries(&report_path);
        let actual = nether_preset_entries();
        assert_eq!(actual.len(), expected.len());
        for (idx, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
            assert_eq!(a, e, "nether preset mismatch at index {idx}");
        }
    }

    #[test]
    fn overworld_preset_matches_datagen_report_if_present() {
        let report_path = zenith_data::version_dir("26.3")
            .join("reports")
            .join("biome_parameters")
            .join("minecraft")
            .join("overworld.json");
        if !report_path.is_file() {
            eprintln!(
                "NOTE: skipping overworld preset report cross-check (absent: {})",
                report_path.display()
            );
            return;
        }
        let expected = load_report_entries(&report_path);
        let actual = overworld_preset_entries();
        assert_eq!(actual.len(), expected.len());
        for (idx, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
            assert_eq!(a, e, "overworld preset mismatch at index {idx}");
        }
    }
}
