//! Dimension seeding (`RandomState`), noise instantiation, and router/aquifer sampling
//! (`docs/specs/worldgen/density-functions.md` §4–§5).

use std::collections::HashMap;
use std::sync::Arc;

use zenith_math::noise::{NETHER_TEMPERATURE_ID, NETHER_VEGETATION_ID, end_island_noise};
use zenith_math::{
    BlendedNoise, BlendedNoiseParameters, Interval, LegacyPositionalRandomFactory, LegacyRandom,
    NormalNoise, PositionalRandomFactory, RandomSource, SimplexNoise,
    XoroshiroPositionalRandomFactory, XoroshiroRandom,
};
use zenith_registry::Identifier;

use crate::ast::{DensityFunction, NoiseHolder};
use crate::compiler::{CompileBackend, CompileError, CompiledSampler, DensityFunctionCompiler};
use crate::data::WorldgenData;
use crate::settings::{AquiferConfig, NoiseGeneratorSettings, RouterField};
use crate::volume::{DensityVolume, SamplerContext};

/// Algorithm-erased random source produced by [`PositionalFactory::at`].
#[derive(Debug, Clone)]
pub enum SeededRandom {
    /// Legacy 48-bit LCG random source.
    Legacy(LegacyRandom),
    /// Xoroshiro128++ random source.
    Xoroshiro(XoroshiroRandom),
}

impl SeededRandom {
    /// Returns a uniformly distributed integer in `0..bound`.
    #[inline]
    pub fn next_int_bounded(&mut self, bound: i32) -> i32 {
        match self {
            Self::Legacy(r) => r.next_int_bounded(bound),
            Self::Xoroshiro(r) => r.next_int_bounded(bound),
        }
    }

    /// Returns a single-precision float in `[0.0, 1.0)`.
    #[inline]
    pub fn next_float(&mut self) -> f32 {
        match self {
            Self::Legacy(r) => r.next_float(),
            Self::Xoroshiro(r) => r.next_float(),
        }
    }

    /// Returns a double-precision float in `[0.0, 1.0)`.
    #[inline]
    pub fn next_double(&mut self) -> f64 {
        match self {
            Self::Legacy(r) => r.next_double(),
            Self::Xoroshiro(r) => r.next_double(),
        }
    }

    /// Returns the next boolean.
    #[inline]
    pub fn next_boolean(&mut self) -> bool {
        match self {
            Self::Legacy(r) => r.next_boolean(),
            Self::Xoroshiro(r) => r.next_boolean(),
        }
    }
}

/// Algorithm-erased positional random factory (`LEGACY` 48-bit LCG or `XOROSHIRO` xoroshiro128++).
#[derive(Debug, Clone)]
pub enum PositionalFactory {
    /// Legacy 48-bit LCG positional factory.
    Legacy(LegacyPositionalRandomFactory),
    /// Xoroshiro128++ positional factory.
    Xoroshiro(XoroshiroPositionalRandomFactory),
}

impl PositionalFactory {
    /// Forks a named positional sub-factory via `from_hash_of(name).fork_positional()`.
    pub fn fork_named(&self, name: &str) -> Self {
        match self {
            Self::Legacy(f) => Self::Legacy(f.from_hash_of(name).fork_positional()),
            Self::Xoroshiro(f) => Self::Xoroshiro(f.from_hash_of(name).fork_positional()),
        }
    }

    /// Creates a random source from the hash of `name` (`rootRandom.fromHashOf(name)`).
    pub fn from_hash_of(&self, name: &str) -> SeededRandom {
        match self {
            Self::Legacy(f) => SeededRandom::Legacy(f.from_hash_of(name)),
            Self::Xoroshiro(f) => SeededRandom::Xoroshiro(f.from_hash_of(name)),
        }
    }

    /// Creates a random source for 3D block/grid coordinates `(x, y, z)`.
    #[inline]
    pub fn at(&self, x: i32, y: i32, z: i32) -> SeededRandom {
        match self {
            Self::Legacy(f) => SeededRandom::Legacy(f.at(x, y, z)),
            Self::Xoroshiro(f) => SeededRandom::Xoroshiro(f.at(x, y, z)),
        }
    }
}

struct SeedingBackend {
    world_seed: i64,
    use_legacy_random: bool,
    root_random: PositionalFactory,
    noise_instances: HashMap<Identifier, Arc<NormalNoise>>,
}

impl SeedingBackend {
    fn new(world_seed: i64, use_legacy_random: bool) -> Self {
        let root_random = if use_legacy_random {
            PositionalFactory::Legacy(LegacyRandom::new(world_seed).fork_positional())
        } else {
            PositionalFactory::Xoroshiro(XoroshiroRandom::new(world_seed).fork_positional())
        };
        Self {
            world_seed,
            use_legacy_random,
            root_random,
            noise_instances: HashMap::new(),
        }
    }
}

impl CompileBackend for SeedingBackend {
    fn create_noise_sampler(
        &mut self,
        holder: &NoiseHolder,
    ) -> Result<Arc<NormalNoise>, CompileError> {
        let Some(id) = holder.id() else {
            // Spec §4 step 2 / Edge Case #11: inline NormalNoise fails at RandomState compile time.
            return Err(CompileError::UnregisteredInlineNoise);
        };
        let key_str = id.to_string();
        if key_str == NETHER_TEMPERATURE_ID {
            let n =
                NormalNoise::from_nether_world_seed(self.world_seed, 0, holder.params().clone())?;
            return Ok(Arc::new(n));
        }
        if key_str == NETHER_VEGETATION_ID {
            let n =
                NormalNoise::from_nether_world_seed(self.world_seed, 1, holder.params().clone())?;
            return Ok(Arc::new(n));
        }
        if let Some(existing) = self.noise_instances.get(id) {
            return Ok(Arc::clone(existing));
        }
        let noise = match &self.root_random {
            PositionalFactory::Legacy(f) => {
                let mut rng = f.from_hash_of(&key_str);
                NormalNoise::new(&mut rng, holder.params().clone())
            }
            PositionalFactory::Xoroshiro(f) => {
                let mut rng = f.from_hash_of(&key_str);
                NormalNoise::new(&mut rng, holder.params().clone())
            }
        };
        let arc = Arc::new(noise);
        self.noise_instances.insert(id.clone(), Arc::clone(&arc));
        Ok(arc)
    }

    fn create_blended_noise(&mut self, params: BlendedNoiseParameters) -> BlendedNoise {
        // Spec §4 step 3 / Edge Case #16: legacy + minecraft:terrain bypasses rootRandom.
        if self.use_legacy_random {
            BlendedNoise::from_legacy_world_seed(self.world_seed, params)
        } else {
            match &self.root_random {
                PositionalFactory::Legacy(f) => BlendedNoise::from_positional_factory(f, params),
                PositionalFactory::Xoroshiro(f) => BlendedNoise::from_positional_factory(f, params),
            }
        }
    }

    fn create_end_island_noise(&mut self) -> SimplexNoise {
        // Spec §4 step 4 / Edge Case #15: always LegacyRandom(worldSeed).
        end_island_noise(self.world_seed)
    }
}

/// Seeded dimension terrain state holding compiled `NoiseRouter` and `AquiferConfig` samplers.
#[derive(Debug)]
pub struct RandomState {
    seed: i64,
    settings: NoiseGeneratorSettings,
    root_random: PositionalFactory,
    noise_instances: HashMap<Identifier, Arc<NormalNoise>>,
    router_samplers: [CompiledSampler; 8],
    aquifer_samplers: Option<[CompiledSampler; AquiferConfig::COUNT]>,
    cache_count: usize,
}

impl RandomState {
    /// Seeds a dimension's [`RandomState`] from `data`, `noise_settings`, and `seed`
    /// (`docs/specs/worldgen/density-functions.md` §4).
    pub fn new(
        data: &WorldgenData,
        noise_settings: &Identifier,
        seed: i64,
    ) -> Result<Self, CompileError> {
        let settings = data
            .noise_settings(noise_settings)
            .ok_or_else(|| CompileError::UnknownNoiseSettings {
                id: noise_settings.clone(),
            })?
            .clone();

        Self::from_settings(settings, seed)
    }

    /// Seeds a [`RandomState`] directly from an owned [`NoiseGeneratorSettings`] and `seed`.
    pub fn from_settings(
        settings: NoiseGeneratorSettings,
        seed: i64,
    ) -> Result<Self, CompileError> {
        let (rs, _) = Self::from_settings_with_extra(settings, seed, &[])?;
        Ok(rs)
    }

    /// Seeds a [`RandomState`] and compiles `extra_functions` (such as `minecraft:ore_vein`
    /// density/richness/filler_gap functions) through the same [`DensityFunctionCompiler`] so
    /// deduplicated `cache(...)` cells and noise instances are shared seamlessly.
    pub(crate) fn from_settings_with_extra(
        settings: NoiseGeneratorSettings,
        seed: i64,
        extra_functions: &[&DensityFunction],
    ) -> Result<(Self, Vec<CompiledSampler>), CompileError> {
        let mut backend = SeedingBackend::new(seed, settings.legacy_random_source());
        let mut compiler = DensityFunctionCompiler::new();

        let router = settings.noise_router();
        let router_samplers = [
            compiler.get_sampler(&router.temperature, &mut backend)?,
            compiler.get_sampler(&router.vegetation, &mut backend)?,
            compiler.get_sampler(&router.continents, &mut backend)?,
            compiler.get_sampler(&router.erosion, &mut backend)?,
            compiler.get_sampler(&router.depth, &mut backend)?,
            compiler.get_sampler(&router.ridges, &mut backend)?,
            compiler.get_sampler(&router.chunk_surface_level, &mut backend)?,
            compiler.get_sampler(&router.final_density, &mut backend)?,
        ];

        let aquifer_samplers = match settings.aquifers() {
            Some(aq) => Some([
                compiler.get_sampler(&aq.barrier, &mut backend)?,
                compiler.get_sampler(&aq.fluid_level_floodedness, &mut backend)?,
                compiler.get_sampler(&aq.fluid_level_spread, &mut backend)?,
                compiler.get_sampler(&aq.lava, &mut backend)?,
                compiler.get_sampler(&aq.exclusion, &mut backend)?,
                compiler.get_sampler(&aq.surface_level, &mut backend)?,
            ]),
            None => None,
        };

        let mut extra_samplers = Vec::with_capacity(extra_functions.len());
        for f in extra_functions {
            extra_samplers.push(compiler.get_sampler(f, &mut backend)?);
        }

        let rs = Self {
            seed,
            settings,
            root_random: backend.root_random,
            noise_instances: backend.noise_instances,
            router_samplers,
            aquifer_samplers,
            cache_count: compiler.cache_count(),
        };
        Ok((rs, extra_samplers))
    }

    /// Instantiates or returns a cached `NormalNoise` for `id`, seeded via
    /// `rootRandom.fromHashOf(id.toString())` (matching `RandomState.getOrCreateNoise`).
    pub fn instantiate_noise(
        &self,
        id: &Identifier,
        params: &zenith_math::NormalNoiseParameters,
    ) -> Result<Arc<NormalNoise>, CompileError> {
        if let Some(existing) = self.noise_instances.get(id) {
            return Ok(Arc::clone(existing));
        }
        let key_str = id.to_string();
        if key_str == NETHER_TEMPERATURE_ID {
            let n = NormalNoise::from_nether_world_seed(self.seed, 0, params.clone())?;
            return Ok(Arc::new(n));
        }
        if key_str == NETHER_VEGETATION_ID {
            let n = NormalNoise::from_nether_world_seed(self.seed, 1, params.clone())?;
            return Ok(Arc::new(n));
        }
        let noise = match &self.root_random {
            PositionalFactory::Legacy(f) => {
                let mut rng = f.from_hash_of(&key_str);
                NormalNoise::new(&mut rng, params.clone())
            }
            PositionalFactory::Xoroshiro(f) => {
                let mut rng = f.from_hash_of(&key_str);
                NormalNoise::new(&mut rng, params.clone())
            }
        };
        Ok(Arc::new(noise))
    }

    /// Samples all six climate channels (`Temperature`, `Vegetation`, `Continents`, `Erosion`,
    /// `Depth`, `Ridges`) over `volume` using one shared caching [`SamplerContext`].
    pub fn sample_climate_volume(&self, volume: DensityVolume) -> [Vec<f32>; 6] {
        let mut ctx = self.new_sampler_context(true);
        let len = volume.len();
        let fields = [
            RouterField::Temperature,
            RouterField::Vegetation,
            RouterField::Continents,
            RouterField::Erosion,
            RouterField::Depth,
            RouterField::Ridges,
        ];
        std::array::from_fn(|i| {
            let mut buf = vec![0.0_f32; len];
            self.router_samplers[fields[i].index()].sample_volume(&mut ctx, &mut buf, volume);
            buf
        })
    }

    /// Compiles a standalone `DensityFunction` with a fresh seeded backend (useful for tests
    /// and micro-benchmarks on individual density functions).
    pub fn compile_function(
        func: &DensityFunction,
        seed: i64,
        use_legacy_random: bool,
    ) -> Result<(CompiledSampler, usize), CompileError> {
        let mut backend = SeedingBackend::new(seed, use_legacy_random);
        let mut compiler = DensityFunctionCompiler::new();
        let sampler = compiler.get_sampler(func, &mut backend)?;
        Ok((sampler, compiler.cache_count()))
    }

    /// World seed used to initialize this state.
    #[inline]
    pub const fn seed(&self) -> i64 {
        self.seed
    }

    /// The `NoiseGeneratorSettings` backing this state.
    #[inline]
    pub const fn settings(&self) -> &NoiseGeneratorSettings {
        &self.settings
    }

    /// Number of deduplicated `cache` cells allocated across the compiled samplers.
    #[inline]
    pub const fn cache_count(&self) -> usize {
        self.cache_count
    }

    /// Root positional random factory (`algorithm.newInstance(worldSeed).forkPositional()`).
    #[inline]
    pub const fn root_random(&self) -> &PositionalFactory {
        &self.root_random
    }

    /// Derives a named positional random factory (`rootRandom.fromHashOf(name).forkPositional()`).
    pub fn positional_random_factory(&self, name: &Identifier) -> PositionalFactory {
        self.root_random.fork_named(&name.to_string())
    }

    /// Looks up a compiled `NormalNoise` instance instantiated during compilation.
    pub fn noise_instance(&self, id: &Identifier) -> Option<&Arc<NormalNoise>> {
        self.noise_instances.get(id)
    }

    /// Creates a fresh [`SamplerContext`] sized for this state's cache cells.
    pub fn new_sampler_context(&self, enable_caches: bool) -> SamplerContext {
        if enable_caches {
            SamplerContext::caching(self.cache_count)
        } else {
            SamplerContext::uncached()
        }
    }

    /// Returns the declared conservative `f32` [`Interval`] of router field `field`.
    #[inline]
    pub fn router_range(&self, field: RouterField) -> Interval {
        self.router_samplers[field.index()].range()
    }

    /// Returns the declared conservative `f32` [`Interval`] of aquifer function `index` (`0..6`),
    /// or `None` if aquifers are disabled or `index >= 6`.
    #[inline]
    pub fn aquifer_range(&self, index: usize) -> Option<Interval> {
        self.aquifer_samplers
            .as_ref()
            .and_then(|arr| arr.get(index))
            .map(CompiledSampler::range)
    }

    /// Uncached point evaluation of a `NoiseRouter` field at block `(x, y, z)`.
    ///
    /// Matches vanilla `RandomState.sampleBlockValueUncached`.
    #[inline]
    pub fn sample_router(&self, field: RouterField, x: i32, y: i32, z: i32) -> f32 {
        let mut ctx = SamplerContext::uncached();
        self.router_samplers[field.index()].sample_value(&mut ctx, x, y, z)
    }

    /// Point evaluation of a `NoiseRouter` field using a caller-owned [`SamplerContext`].
    #[inline]
    pub fn sample_router_with_context(
        &self,
        ctx: &mut SamplerContext,
        field: RouterField,
        x: i32,
        y: i32,
        z: i32,
    ) -> f32 {
        self.router_samplers[field.index()].sample_value(ctx, x, y, z)
    }

    /// Uncached point evaluation of an `AquiferConfig` density function at block `(x, y, z)`.
    ///
    /// Field index order matches `docs/specs/worldgen/density-functions.md` §5.3:
    /// - `0`: `barrier`
    /// - `1`: `fluid_level_floodedness`
    /// - `2`: `fluid_level_spread`
    /// - `3`: `lava`
    /// - `4`: `exclusion`
    /// - `5`: `surface_level`
    ///
    /// Returns `None` when `settings` has no `aquifers` config or `index >= 6`.
    #[inline]
    pub fn sample_aquifer(&self, index: usize, x: i32, y: i32, z: i32) -> Option<f32> {
        let sampler = self.aquifer_samplers.as_ref()?.get(index)?;
        let mut ctx = SamplerContext::uncached();
        Some(sampler.sample_value(&mut ctx, x, y, z))
    }

    /// Point evaluation of an `AquiferConfig` function using a caller-owned [`SamplerContext`].
    #[inline]
    pub fn sample_aquifer_with_context(
        &self,
        ctx: &mut SamplerContext,
        index: usize,
        x: i32,
        y: i32,
        z: i32,
    ) -> Option<f32> {
        let sampler = self.aquifer_samplers.as_ref()?.get(index)?;
        Some(sampler.sample_value(ctx, x, y, z))
    }

    /// Caching volume evaluation of `field` over chunk `(chunk_x, chunk_z)` across the full
    /// vertical range `[min_y, min_y + height)`.
    ///
    /// # Buffer layout
    ///
    /// The returned `Vec<f32>` has length `16 * height * 16` (or `0` when `height == 0`) with
    /// **Z outer, X middle, Y contiguous inner** indexing (`density-functions.md` "Inputs and state"):
    ///
    /// ```text
    /// index(ix, iy, iz) = iy + (ix + iz * 16) * height
    /// ```
    ///
    /// where `ix in 0..16`, `iy in 0..height`, `iz in 0..16` correspond to world block coordinates
    /// `((chunk_x << 4) + ix, min_y + iy, (chunk_z << 4) + iz)`.
    pub fn sample_volume(&self, field: RouterField, chunk_x: i32, chunk_z: i32) -> Vec<f32> {
        let height = self.settings.height();
        if height <= 0 {
            return Vec::new();
        }
        let vol = DensityVolume::unit(
            16,
            height as usize,
            16,
            chunk_x.wrapping_shl(4),
            self.settings.min_y(),
            chunk_z.wrapping_shl(4),
        )
        .expect("chunk volume dimensions are valid");
        self.sample_volume_for(field, vol, true)
    }

    /// Volume evaluation of `field` over an arbitrary [`DensityVolume`].
    ///
    /// Buffer layout is Z-outer, X-middle, Y-inner:
    /// `index(ix, iy, iz) = iy + (ix + iz * volume.size_x()) * volume.size_y()`.
    pub fn sample_volume_for(
        &self,
        field: RouterField,
        volume: DensityVolume,
        enable_caches: bool,
    ) -> Vec<f32> {
        let mut ctx = self.new_sampler_context(enable_caches);
        let mut out = vec![0.0_f32; volume.len()];
        self.router_samplers[field.index()].sample_volume(&mut ctx, &mut out, volume);
        out
    }

    /// Volume evaluation of `field` into a caller-provided buffer using a shared [`SamplerContext`].
    #[inline]
    pub fn sample_volume_with_context(
        &self,
        ctx: &mut SamplerContext,
        field: RouterField,
        volume: DensityVolume,
        out: &mut [f32],
    ) {
        self.router_samplers[field.index()].sample_volume(ctx, out, volume);
    }

    /// Volume evaluation of an `AquiferConfig` function (`0..6`) over an arbitrary [`DensityVolume`].
    pub fn sample_aquifer_volume(
        &self,
        index: usize,
        volume: DensityVolume,
        enable_caches: bool,
    ) -> Option<Vec<f32>> {
        let sampler = self.aquifer_samplers.as_ref()?.get(index)?;
        let mut ctx = self.new_sampler_context(enable_caches);
        let mut out = vec![0.0_f32; volume.len()];
        sampler.sample_volume(&mut ctx, &mut out, volume);
        Some(out)
    }

    /// Returns the compiled sampler for router `field`.
    #[inline]
    pub(crate) fn router_sampler(&self, field: RouterField) -> &CompiledSampler {
        &self.router_samplers[field.index()]
    }

    /// Returns the 6 compiled aquifer samplers if aquifers are enabled on these settings.
    #[inline]
    pub(crate) fn aquifer_samplers(&self) -> Option<&[CompiledSampler; AquiferConfig::COUNT]> {
        self.aquifer_samplers.as_ref()
    }

    /// Samples all six climate router channels at block `(x, y, z)` via the uncached point path.
    ///
    /// Returned array order is `[temperature, vegetation, continents, erosion, depth, ridges]`
    /// (matching `Climate.Sampler`: temperature, humidity, continentalness, erosion, depth, weirdness).
    #[inline]
    pub fn climate_point(&self, x: i32, y: i32, z: i32) -> [f32; 6] {
        let mut ctx = SamplerContext::uncached();
        [
            self.router_samplers[RouterField::Temperature.index()].sample_value(&mut ctx, x, y, z),
            self.router_samplers[RouterField::Vegetation.index()].sample_value(&mut ctx, x, y, z),
            self.router_samplers[RouterField::Continents.index()].sample_value(&mut ctx, x, y, z),
            self.router_samplers[RouterField::Erosion.index()].sample_value(&mut ctx, x, y, z),
            self.router_samplers[RouterField::Depth.index()].sample_value(&mut ctx, x, y, z),
            self.router_samplers[RouterField::Ridges.index()].sample_value(&mut ctx, x, y, z),
        ]
    }
}
