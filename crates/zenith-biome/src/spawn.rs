//! World-origin spawn chunk finder (`NoiseSpawnFinder` and `SpawnTargetPoint`, spec §4).

use zenith_codec::codec::{Codec, DataResult, string_map};
use zenith_codec::json::Json;
use zenith_math::helpers::intrinsics::{java_math_cos, java_math_sin};
use zenith_math::helpers::rounding::square_i64;
use zenith_registry::Identifier;

use crate::climate::{Parameter, ParameterCodec, quantize_coord};

/// Weight applied to climate fitness vs squared distance from world origin (`2048^2 = 4,194,304`).
pub const SPAWN_FITNESS_WEIGHT: i64 = 4_194_304;

/// One target point in `NoiseGeneratorSettings.spawn_target`: a map from density-function
/// identifier to desired [`Parameter`] interval (spec §4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnTargetPoint {
    /// Ordered `(density_function_id, parameter)` pairs.
    pub entries: Vec<(Identifier, Parameter)>,
}

impl SpawnTargetPoint {
    /// Constructs a [`SpawnTargetPoint`] from `(density_function_id, parameter)` pairs.
    pub fn new(entries: Vec<(Identifier, Parameter)>) -> Self {
        Self { entries }
    }
}

/// JSON codec for [`SpawnTargetPoint`] (`{"<density_function_id>": <Parameter>, ...}`).
#[derive(Debug, Clone, Copy, Default)]
pub struct SpawnTargetPointCodec;

impl Codec<SpawnTargetPoint> for SpawnTargetPointCodec {
    fn decode(&self, input: &Json) -> DataResult<SpawnTargetPoint> {
        string_map(ParameterCodec).decode(input).and_then(|pairs| {
            let mut entries = Vec::with_capacity(pairs.len());
            let mut errors = Vec::new();
            for (k, param) in pairs {
                match Identifier::parse(&k) {
                    Ok(id) => entries.push((id, param)),
                    Err(e) => errors.push(format!("Entry {k}: {e}")),
                }
            }
            if errors.is_empty() {
                DataResult::Success(SpawnTargetPoint { entries })
            } else {
                DataResult::error_with_partial(errors.join("; "), SpawnTargetPoint { entries })
            }
        })
    }

    fn encode(&self, value: &SpawnTargetPoint) -> DataResult<Json> {
        let pairs: Vec<(String, Parameter)> = value
            .entries
            .iter()
            .map(|(id, p)| (id.to_string(), *p))
            .collect();
        string_map(ParameterCodec).encode(&pairs)
    }
}

/// Builds the vanilla Overworld `spawn_target` list (2 target points, spec §4) for the given
/// Overworld density-function identifiers `(temperature, vegetation, continents, erosion)` plus
/// `minecraft:overworld/ridges`.
pub fn overworld_spawn_targets(
    temperature_fn: Identifier,
    vegetation_fn: Identifier,
    continents_fn: Identifier,
    erosion_fn: Identifier,
) -> Vec<SpawnTargetPoint> {
    let ridges_fn =
        Identifier::with_default_namespace("overworld/ridges").expect("valid identifier");
    let full = Parameter::span(-1.0, 1.0).expect("valid span");
    let inland = Parameter::span(-0.11, 1.0).expect("valid span");
    let neg_ridges = Parameter::span(-1.0, -0.16).expect("valid span");
    let pos_ridges = Parameter::span(0.16, 1.0).expect("valid span");
    vec![
        SpawnTargetPoint::new(vec![
            (temperature_fn.clone(), full),
            (vegetation_fn.clone(), full),
            (continents_fn.clone(), inland),
            (erosion_fn.clone(), full),
            (ridges_fn.clone(), neg_ridges),
        ]),
        SpawnTargetPoint::new(vec![
            (temperature_fn, full),
            (vegetation_fn, full),
            (continents_fn, inland),
            (erosion_fn, full),
            (ridges_fn, pos_ridges),
        ]),
    ]
}

/// Executes `NoiseSpawnFinder::findSpawnPosition` (spec §4) and returns the chosen origin
/// chunk coordinates `(chunk_x, chunk_z)`.
///
/// If `targets` is empty, returns `(0, 0)` without invoking `sample_fn`.
pub fn find_spawn_chunk(
    targets: &[SpawnTargetPoint],
    mut sample_fn: impl FnMut(&Identifier, i32, i32, i32) -> f32,
) -> (i32, i32) {
    if targets.is_empty() {
        return (0, 0);
    }

    let mut eval = |block_x: i32, block_z: i32| -> ((i32, i32), i64) {
        let qx = (block_x >> 2) << 2;
        let qz = (block_z >> 2) << 2;
        let mut min_fitness = i64::MAX;
        for target in targets {
            let mut sum: i64 = 0;
            for (fn_id, param) in &target.entries {
                let v = quantize_coord(sample_fn(fn_id, qx, 0, qz));
                sum = sum.wrapping_add(square_i64(param.distance(v)));
            }
            if sum < min_fitness {
                min_fitness = sum;
            }
        }
        let total_fitness = min_fitness
            .wrapping_mul(SPAWN_FITNESS_WEIGHT)
            .wrapping_add(square_i64(i64::from(block_x)))
            .wrapping_add(square_i64(i64::from(block_z)));
        ((block_x, block_z), total_fitness)
    };

    let mut best = eval(0, 0);
    radial_search(&mut best, 2048.0_f32, 512.0_f32, &mut eval);
    radial_search(&mut best, 512.0_f32, 32.0_f32, &mut eval);
    (best.0.0 >> 4, best.0.1 >> 4)
}

fn radial_search(
    best: &mut ((i32, i32), i64),
    max_radius: f32,
    step: f32,
    eval: &mut impl FnMut(i32, i32) -> ((i32, i32), i64),
) {
    let origin = best.0;
    let mut angle: f32 = 0.0_f32;
    let mut radius: f32 = step;

    while radius <= max_radius {
        let x = origin
            .0
            .wrapping_add((java_math_sin(f64::from(angle)) * f64::from(radius)) as i32);
        let z = origin
            .1
            .wrapping_add((java_math_cos(f64::from(angle)) * f64::from(radius)) as i32);
        let cand = eval(x, z);
        if cand.1 < best.1 {
            *best = cand;
        }
        angle += step / radius;
        if f64::from(angle) > 2.0_f64 * std::f64::consts::PI {
            angle = 0.0_f32;
            radius += step;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zenith_codec::json::{ParseMode, parse};

    #[test]
    fn empty_spawn_target_returns_origin_chunk() {
        let chunk = find_spawn_chunk(&[], |_, _, _, _| panic!("must not sample"));
        assert_eq!(chunk, (0, 0));
    }

    #[test]
    fn spawn_target_point_codec_and_radial_search() {
        let j = parse(
            r#"{"minecraft:overworld/continents": [-0.11, 1.0], "minecraft:overworld/ridges": [0.16, 1.0]}"#,
            ParseMode::Data,
        )
        .unwrap();
        let target = SpawnTargetPointCodec.decode(&j).demand().unwrap();
        assert_eq!(target.entries.len(), 2);

        // Make only points in z in [480, 544] satisfy the target; origin (0, 0) fails badly.
        // Pass 1 (step 512) finds (0, 512), and pass 2 (step 32) refines closer to origin (z = 480).
        let chunk = find_spawn_chunk(&[target], |id, _x, _y, z| {
            if id.path() == "overworld/continents" {
                if (480..=544).contains(&z) { 0.2 } else { -1.0 }
            } else {
                0.5
            }
        });
        assert_eq!(chunk, (-3, 30));

        // Round-trip encode/decode for SpawnTargetPoint.
        let targets = overworld_spawn_targets(
            Identifier::with_default_namespace("overworld/temperature").unwrap(),
            Identifier::with_default_namespace("overworld/vegetation").unwrap(),
            Identifier::with_default_namespace("overworld/continents").unwrap(),
            Identifier::with_default_namespace("overworld/erosion").unwrap(),
        );
        assert_eq!(targets.len(), 2);
        let encoded = SpawnTargetPointCodec.encode(&targets[0]).demand().unwrap();
        let decoded = SpawnTargetPointCodec.decode(&encoded).demand().unwrap();
        assert_eq!(decoded, targets[0]);
    }
}
