//! Smeared Perlin noise ("y-fuzz" variant) used by blended terrain noise.

use crate::noise::primitives::{
    DEFAULT_OFFSET_SCALE, GRADIENT, GradientNoiseState, Interval, Noise, VolumeGrid, floor_to_i32,
    grad_dot_f32, grad_dot_xz_f32, lerp3, smoothstep, wrap,
};
use crate::random::RandomSource;

/// Single-precision shift epsilon `1.0e-7_f32` (`0x33D6_BF95`), widened to `f64`
/// (`1.0000000116860974e-7`).
pub const SMEAR_EPSILON: f64 = f32::from_bits(0x33D6_BF95) as f64;

/// Single-octave Perlin noise with quantized Y-axis displacement ("y-fuzz").
///
/// The Y fade weight uses the undisplaced fractional coordinate `f32(y_r)`, whereas the eight
/// corner gradient dot products use the quantized displaced coordinate `y_smear`.
#[derive(Debug, Clone, PartialEq)]
pub struct SmearedPerlinNoise {
    state: GradientNoiseState,
    y_smear_scale: f64,
}

impl SmearedPerlinNoise {
    /// Constructs a new smeared Perlin octave from `rng` with Y-smear step size `y_smear_scale`.
    pub fn new<R: RandomSource>(rng: &mut R, y_smear_scale: f64) -> Self {
        Self {
            state: GradientNoiseState::new(rng, DEFAULT_OFFSET_SCALE),
            y_smear_scale,
        }
    }

    /// Constructs a smeared Perlin octave from an existing [`GradientNoiseState`] and step size.
    pub const fn from_state(state: GradientNoiseState, y_smear_scale: f64) -> Self {
        Self {
            state,
            y_smear_scale,
        }
    }

    /// Returns the underlying gradient-noise state.
    #[inline]
    pub const fn state(&self) -> &GradientNoiseState {
        &self.state
    }

    /// Returns the Y-smear step size `q_y`.
    #[inline]
    pub const fn y_smear_scale(&self) -> f64 {
        self.y_smear_scale
    }

    /// Returns the parity configuration summary string for this octave.
    pub fn parity_config_string(&self) -> String {
        self.state.parity_config_string()
    }

    /// Computes the conservative symmetric interval `[-r, r]` for a given step size `y_smear_scale`,
    /// where `r = f32(|q_y| + 2.0)`.
    #[inline]
    pub fn range_for_scale(y_smear_scale: f64) -> Interval {
        Interval::symmetric((y_smear_scale.abs() + 2.0) as f32)
    }

    /// Conservative `f32` output interval `[-f32(|q_y| + 2.0), +f32(|q_y| + 2.0)]`.
    #[inline]
    pub fn range(&self) -> Interval {
        Self::range_for_scale(self.y_smear_scale)
    }

    #[inline]
    fn compute_y_smear(&self, y_in: f64, yr_f64: f64) -> f32 {
        let limit = if y_in >= 0.0 && y_in < yr_f64 {
            y_in
        } else {
            yr_f64
        };
        let delta_y = f64::from(floor_to_i32(limit / self.y_smear_scale + SMEAR_EPSILON))
            * self.y_smear_scale;
        (yr_f64 - delta_y) as f32
    }

    /// Samples 3D smeared Perlin noise at `(x_in, y_in, z_in)`.
    ///
    /// Note that the clamp test `0.0 <= y_in < y_r` uses the unwrapped, unshifted `y_in`.
    pub fn sample(&self, x_in: f64, y_in: f64, z_in: f64) -> f32 {
        let x = wrap(x_in) + self.state.xo();
        let y = wrap(y_in) + self.state.yo();
        let z = wrap(z_in) + self.state.zo();

        let xi = floor_to_i32(x);
        let yi = floor_to_i32(y);
        let zi = floor_to_i32(z);

        let xr = (x - f64::from(xi)) as f32;
        let yr_f64 = y - f64::from(yi);
        let zr = (z - f64::from(zi)) as f32;

        let y_smear = self.compute_y_smear(y_in, yr_f64);

        let hx0 = self.state.hash(xi);
        let hx1 = self.state.hash(xi.wrapping_add(1));
        let h00 = self.state.hash(hx0.wrapping_add(yi));
        let h01 = self.state.hash(hx0.wrapping_add(yi).wrapping_add(1));
        let h10 = self.state.hash(hx1.wrapping_add(yi));
        let h11 = self.state.hash(hx1.wrapping_add(yi).wrapping_add(1));

        let g000 = GRADIENT[(self.state.hash(h00.wrapping_add(zi)) & 15) as usize];
        let g100 = GRADIENT[(self.state.hash(h10.wrapping_add(zi)) & 15) as usize];
        let g010 = GRADIENT[(self.state.hash(h01.wrapping_add(zi)) & 15) as usize];
        let g110 = GRADIENT[(self.state.hash(h11.wrapping_add(zi)) & 15) as usize];
        let g001 = GRADIENT[(self.state.hash(h00.wrapping_add(zi).wrapping_add(1)) & 15) as usize];
        let g101 = GRADIENT[(self.state.hash(h10.wrapping_add(zi).wrapping_add(1)) & 15) as usize];
        let g011 = GRADIENT[(self.state.hash(h01.wrapping_add(zi).wrapping_add(1)) & 15) as usize];
        let g111 = GRADIENT[(self.state.hash(h11.wrapping_add(zi).wrapping_add(1)) & 15) as usize];

        let d000 = grad_dot_f32(g000, xr, y_smear, zr);
        let d100 = grad_dot_f32(g100, xr - 1.0_f32, y_smear, zr);
        let d010 = grad_dot_f32(g010, xr, y_smear - 1.0_f32, zr);
        let d110 = grad_dot_f32(g110, xr - 1.0_f32, y_smear - 1.0_f32, zr);
        let d001 = grad_dot_f32(g001, xr, y_smear, zr - 1.0_f32);
        let d101 = grad_dot_f32(g101, xr - 1.0_f32, y_smear, zr - 1.0_f32);
        let d011 = grad_dot_f32(g011, xr, y_smear - 1.0_f32, zr - 1.0_f32);
        let d111 = grad_dot_f32(g111, xr - 1.0_f32, y_smear - 1.0_f32, zr - 1.0_f32);

        let ax = smoothstep(xr);
        let ay = smoothstep(yr_f64 as f32);
        let az = smoothstep(zr);

        lerp3(ax, ay, az, d000, d100, d010, d110, d001, d101, d011, d111)
    }

    /// Samples 2D smeared Perlin noise on the XZ plane: delegates to
    /// `sample(wrap(x_in), 0.0, wrap(y_in))`.
    #[inline]
    pub fn sample_2d(&self, x_in: f64, y_in: f64) -> f32 {
        self.sample(wrap(x_in), 0.0, wrap(y_in))
    }

    /// Column-cached volume accumulation over `grid` in `(z, x, y)` order.
    ///
    /// Uses `y_in = f64::from(block_y(iy)) * scale_y` (unwrapped, unshifted) for the smear clamp,
    /// undisplaced `smoothstep(yr_f64 as f32)` for `ay`, and `y_smear` for the Y dot-product
    /// additions.
    ///
    /// # Panics
    ///
    /// Panics if `buffer.len() < grid.len()`.
    pub fn accumulate_volume(
        &self,
        grid: &VolumeGrid,
        scale_xz: f64,
        scale_y: f64,
        amplitude: f32,
        buffer: &mut [f32],
    ) {
        assert!(
            buffer.len() >= grid.len(),
            "volume buffer length {} is smaller than grid size {}",
            buffer.len(),
            grid.len()
        );

        let mut idx = 0;
        for iz in 0..grid.size_z() {
            let z = wrap(f64::from(grid.block_z(iz)) * scale_xz) + self.state.zo();
            let zi = floor_to_i32(z);
            let zr = (z - f64::from(zi)) as f32;
            let zr1 = zr - 1.0_f32;
            let az = smoothstep(zr);

            for ix in 0..grid.size_x() {
                let x = wrap(f64::from(grid.block_x(ix)) * scale_xz) + self.state.xo();
                let xi = floor_to_i32(x);
                let xr = (x - f64::from(xi)) as f32;
                let xr1 = xr - 1.0_f32;
                let ax = smoothstep(xr);
                let hx0 = self.state.hash(xi);
                let hx1 = self.state.hash(xi.wrapping_add(1));

                let mut y_prev = i32::MIN;
                let mut dxz000 = 0.0_f32;
                let mut dxz100 = 0.0_f32;
                let mut dxz010 = 0.0_f32;
                let mut dxz110 = 0.0_f32;
                let mut dxz001 = 0.0_f32;
                let mut dxz101 = 0.0_f32;
                let mut dxz011 = 0.0_f32;
                let mut dxz111 = 0.0_f32;
                let mut gy000 = 0.0_f32;
                let mut gy100 = 0.0_f32;
                let mut gy010 = 0.0_f32;
                let mut gy110 = 0.0_f32;
                let mut gy001 = 0.0_f32;
                let mut gy101 = 0.0_f32;
                let mut gy011 = 0.0_f32;
                let mut gy111 = 0.0_f32;

                for iy in 0..grid.size_y() {
                    let y_in = f64::from(grid.block_y(iy)) * scale_y;
                    let y = wrap(y_in) + self.state.yo();
                    let yi = floor_to_i32(y);
                    let yr_f64 = y - f64::from(yi);
                    let ay = smoothstep(yr_f64 as f32);
                    let y_smear = self.compute_y_smear(y_in, yr_f64);
                    let y_smear1 = y_smear - 1.0_f32;

                    if yi != y_prev {
                        y_prev = yi;
                        let h00 = self.state.hash(hx0.wrapping_add(yi));
                        let h01 = self.state.hash(hx0.wrapping_add(yi).wrapping_add(1));
                        let h10 = self.state.hash(hx1.wrapping_add(yi));
                        let h11 = self.state.hash(hx1.wrapping_add(yi).wrapping_add(1));

                        let g000 = GRADIENT[(self.state.hash(h00.wrapping_add(zi)) & 15) as usize];
                        let g100 = GRADIENT[(self.state.hash(h10.wrapping_add(zi)) & 15) as usize];
                        let g010 = GRADIENT[(self.state.hash(h01.wrapping_add(zi)) & 15) as usize];
                        let g110 = GRADIENT[(self.state.hash(h11.wrapping_add(zi)) & 15) as usize];
                        let g001 = GRADIENT
                            [(self.state.hash(h00.wrapping_add(zi).wrapping_add(1)) & 15) as usize];
                        let g101 = GRADIENT
                            [(self.state.hash(h10.wrapping_add(zi).wrapping_add(1)) & 15) as usize];
                        let g011 = GRADIENT
                            [(self.state.hash(h01.wrapping_add(zi).wrapping_add(1)) & 15) as usize];
                        let g111 = GRADIENT
                            [(self.state.hash(h11.wrapping_add(zi).wrapping_add(1)) & 15) as usize];

                        dxz000 = grad_dot_xz_f32(g000, xr, zr);
                        dxz100 = grad_dot_xz_f32(g100, xr1, zr);
                        dxz010 = grad_dot_xz_f32(g010, xr, zr);
                        dxz110 = grad_dot_xz_f32(g110, xr1, zr);
                        dxz001 = grad_dot_xz_f32(g001, xr, zr1);
                        dxz101 = grad_dot_xz_f32(g101, xr1, zr1);
                        dxz011 = grad_dot_xz_f32(g011, xr, zr1);
                        dxz111 = grad_dot_xz_f32(g111, xr1, zr1);

                        gy000 = f32::from(g000[1]);
                        gy100 = f32::from(g100[1]);
                        gy010 = f32::from(g010[1]);
                        gy110 = f32::from(g110[1]);
                        gy001 = f32::from(g001[1]);
                        gy101 = f32::from(g101[1]);
                        gy011 = f32::from(g011[1]);
                        gy111 = f32::from(g111[1]);
                    }

                    let d000 = dxz000 + gy000 * y_smear;
                    let d100 = dxz100 + gy100 * y_smear;
                    let d010 = dxz010 + gy010 * y_smear1;
                    let d110 = dxz110 + gy110 * y_smear1;
                    let d001 = dxz001 + gy001 * y_smear;
                    let d101 = dxz101 + gy101 * y_smear;
                    let d011 = dxz011 + gy011 * y_smear1;
                    let d111 = dxz111 + gy111 * y_smear1;

                    let value = lerp3(ax, ay, az, d000, d100, d010, d110, d001, d101, d011, d111);
                    buffer[idx] += amplitude * value;
                    idx += 1;
                }
            }
        }
    }
}

impl Noise for SmearedPerlinNoise {
    #[inline]
    fn sample(&self, x: f64, y: f64, z: f64) -> f32 {
        SmearedPerlinNoise::sample(self, x, y, z)
    }

    #[inline]
    fn sample_2d(&self, x: f64, y: f64) -> f32 {
        SmearedPerlinNoise::sample_2d(self, x, y)
    }

    #[inline]
    fn range(&self) -> Interval {
        SmearedPerlinNoise::range(self)
    }

    #[inline]
    fn accumulate_volume(
        &self,
        grid: &VolumeGrid,
        scale_xz: f64,
        scale_y: f64,
        amplitude: f32,
        buffer: &mut [f32],
    ) {
        SmearedPerlinNoise::accumulate_volume(self, grid, scale_xz, scale_y, amplitude, buffer);
    }
}
