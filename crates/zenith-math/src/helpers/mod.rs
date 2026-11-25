//! Java-exact 26.3 math, easing, color, packing, and geometry helpers.
//!
//! Independent clean-room implementation built from `docs/specs/math/java-math-helpers.md`
//! (sections 1–9), `docs/specs/math/random.md`, `docs/specs/math/noise.md`, and public IEEE-754,
//! Java SE `Math`, MurmurHash3, De Bruijn, Newton-Raphson, and easing specifications.
//!
//! # Submodules
//!
//! - [`intrinsics`]: Centralized wrappers for JVM-intrinsic-sensitive functions (`sin`, `cos`,
//!   `asin`, `atan2`, `log`, `exp`, `pow`).
//! - [`trig`]: 65,536-entry `f32` sine lookup table (`sin`, `cos`) and 257-entry table-plus-Taylor
//!   `atan2`.
//! - [`roots`]: `sqrt`, `inv_sqrt_f32`/`f64`, `fast_inv_sqrt`, and `fast_inv_cube_root`.
//! - [`rounding`]: Floor/ceil/frac/clamp/sign/equal, positive modulo, ceil-div, quantization,
//!   power-of-two/log2 bit-math, binary search, [`Divisor`], and [`OutFromOrigin`].
//! - [`interp`]: Linear/bilinear/trilinear/integer/discrete interpolation, remapping, smoothstep,
//!   Catmull-Rom, triangle wave, all 32 [`Ease`] curves, [`CubicBezier`], [`CubicSpline`],
//!   [`SmoothDouble`], and [`BinaryAnimator`].
//! - [`angles`]: Degree wrapping, shortest-arc stepping, rotational lerp, byte angle packing,
//!   [`SegmentedAnglePrecision`], and [`Quaternionf`] twist projection.
//! - [`hash`]: MurmurHash3 `fmix32`, quadratic 64-bit LCG mixer, chunk/vector hashes, bounded
//!   RNG helpers, `wobble`, and version-4 [`Uuid128`] stamping.
//! - [`color`]: 32-bit ARGB/ABGR ops, 10-bit sRGB/linear LUTs, integer alpha/linear blending,
//!   HSV conversion, and 4-bit/8-bit packed light coordinates.
//! - [`packing`]: 64-bit `BlockPos`, `SectionPos`, and `ChunkPos` packing plus `QuartPos` shifts.
//! - [`geom`]: [`Vec3i`], [`Vec3`], [`Vec2`], `LpVec3` network quantization, [`Aabb`],
//!   [`AabbBuilder`], slab segment clipping, and SAT [`ray_intersects_aabb`].

pub mod angles;
pub mod color;
pub mod geom;
pub mod hash;
pub mod interp;
pub mod intrinsics;
pub mod packing;
pub mod roots;
pub mod rounding;
pub mod trig;

pub use angles::{Quaternionf, SegmentedAnglePrecision};
pub use geom::{Aabb, AabbBuilder, Axis, Direction, LpVec3DecodeError, Vec2, Vec3, Vec3i};
pub use hash::Uuid128;
pub use interp::{
    BinaryAnimator, CubicBezier, CubicSpline, Ease, EasingType, IdentityCoordinate, SmoothDouble,
    SplineCoordinate, SplineError,
};
pub use rounding::{Divisor, OutFromOrigin};
