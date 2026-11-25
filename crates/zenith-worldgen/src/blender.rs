//! Old-chunk terrain blending interface (`Blender`, `docs/specs/worldgen/density-functions.md` §9).
//!
//! For freshly generated worlds (no pre-existing old chunks with `BlendingData`), `Blender::empty()`
//! contributes the exact identity (`blend_alpha = 1.0f`, `blend_offset = 0.0f`,
//! `blend_density(x) = x`). Full old-chunk `BlendingData` blending will plug into these hooks in a
//! later milestone.

use std::sync::Arc;

use crate::volume::{BlenderHook, ContextSampler, DensityVolume, SamplerContext};

/// Old-chunk terrain blender attached to a [`NoiseChunk`](crate::NoiseChunk).
#[derive(Debug, Clone, Default)]
pub struct Blender {
    hooks: Option<ActiveBlenderHooks>,
}

#[derive(Debug, Clone)]
struct ActiveBlenderHooks {
    alpha: Arc<dyn ContextSampler>,
    offset: Arc<dyn ContextSampler>,
    density: Arc<dyn BlenderHook>,
}

impl Blender {
    /// Empty blender (`Blender.EMPTY`): identity contribution for freshly generated worlds.
    #[inline]
    pub const fn empty() -> Self {
        Self { hooks: None }
    }

    /// Constructs an active blender from precomputed `alpha`, `offset`, and `blend_density` hooks.
    pub fn active(
        alpha: Arc<dyn ContextSampler>,
        offset: Arc<dyn ContextSampler>,
        density: Arc<dyn BlenderHook>,
    ) -> Self {
        Self {
            hooks: Some(ActiveBlenderHooks {
                alpha,
                offset,
                density,
            }),
        }
    }

    /// Returns `true` when no old-chunk blending data is present (`Blender.EMPTY`).
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.hooks.is_none()
    }

    /// Attaches active blending hooks to `ctx` (no-op when `self.is_empty()`).
    pub fn attach_to_context(&self, ctx: SamplerContext, _volume: DensityVolume) -> SamplerContext {
        match &self.hooks {
            Some(h) => ctx.with_blender(
                Arc::clone(&h.alpha),
                Arc::clone(&h.offset),
                Arc::clone(&h.density),
            ),
            None => ctx,
        }
    }
}
