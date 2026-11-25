//! Improved Perlin noise (single octave): 3D sampling, 2D XZ sampling, analytical derivatives,
//! and column-cached volume accumulation.

use crate::noise::primitives::{
    DEFAULT_OFFSET_SCALE, GRADIENT, GradientNoiseState, Interval, Noise, VolumeGrid, floor_to_i32,
    grad_dot_f32, grad_dot_xz_f32, lerp2, lerp3, smoothstep, smoothstep_derivative, wrap,
};
use crate::random::RandomSource;

/// Empirical standard deviation constant of improved Perlin noise (`0x3FD1_4B5C_E385_AA29`).
pub const PERLIN_STD_DEV: f64 = f64::from_bits(0x3FD1_4B5C_E385_AA29);

/// Single-octave improved Perlin gradient noise.
///
/// World/lattice coordinate wrapping, origin offset addition, and cell floor subtraction happen
/// in `f64`; fractional offsets, gradient dots, quintic fade, trilinear interpolation, and
/// analytical derivatives are evaluated in `f32`.
#[derive(Debug, Clone, PartialEq)]
pub struct PerlinNoise {
    state: GradientNoiseState,
}

impl PerlinNoise {
    /// Constructs a new improved Perlin octave from `rng` using the default offset scale `256.0`.
    pub fn new<R: RandomSource>(rng: &mut R) -> Self {
        Self {
            state: GradientNoiseState::new(rng, DEFAULT_OFFSET_SCALE),
        }
    }

    /// Constructs a Perlin octave from an existing [`GradientNoiseState`].
    pub const fn from_state(state: GradientNoiseState) -> Self {
        Self { state }
    }

    /// Returns the underlying gradient-noise state (origin offsets and permutation table).
    #[inline]
    pub const fn state(&self) -> &GradientNoiseState {
        &self.state
    }

    /// Returns the parity configuration summary string for this octave.
    pub fn parity_config_string(&self) -> String {
        self.state.parity_config_string()
    }

    /// Conservative `f32` output interval `[-2.0, 2.0]`.
    #[inline]
    pub const fn range(&self) -> Interval {
        Interval::Bounds {
            min: -2.0_f32,
            max: 2.0_f32,
        }
    }

    /// Samples 3D improved Perlin noise at `(x_in, y_in, z_in)`.
    pub fn sample(&self, x_in: f64, y_in: f64, z_in: f64) -> f32 {
        let x = wrap(x_in) + self.state.xo();
        let y = wrap(y_in) + self.state.yo();
        let z = wrap(z_in) + self.state.zo();

        let xi = floor_to_i32(x);
        let yi = floor_to_i32(y);
        let zi = floor_to_i32(z);

        let xr = (x - f64::from(xi)) as f32;
        let yr = (y - f64::from(yi)) as f32;
        let zr = (z - f64::from(zi)) as f32;

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

        let d000 = grad_dot_f32(g000, xr, yr, zr);
        let d100 = grad_dot_f32(g100, xr - 1.0_f32, yr, zr);
        let d010 = grad_dot_f32(g010, xr, yr - 1.0_f32, zr);
        let d110 = grad_dot_f32(g110, xr - 1.0_f32, yr - 1.0_f32, zr);
        let d001 = grad_dot_f32(g001, xr, yr, zr - 1.0_f32);
        let d101 = grad_dot_f32(g101, xr - 1.0_f32, yr, zr - 1.0_f32);
        let d011 = grad_dot_f32(g011, xr, yr - 1.0_f32, zr - 1.0_f32);
        let d111 = grad_dot_f32(g111, xr - 1.0_f32, yr - 1.0_f32, zr - 1.0_f32);

        let ax = smoothstep(xr);
        let ay = smoothstep(yr);
        let az = smoothstep(zr);

        lerp3(ax, ay, az, d000, d100, d010, d110, d001, d101, d011, d111)
    }

    /// Samples 2D improved Perlin noise on the XZ plane: delegates to
    /// `sample(wrap(x_in), 0.0, wrap(y_in))`.
    #[inline]
    pub fn sample_2d(&self, x_in: f64, y_in: f64) -> f32 {
        self.sample(wrap(x_in), 0.0, wrap(y_in))
    }

    /// Samples 3D improved Perlin noise and **accumulates** (`+=`) the analytical partial
    /// derivatives `(d/dx, d/dy, d/dz)` into `derivatives` in `f32`.
    pub fn sample_with_derivative(
        &self,
        x_in: f64,
        y_in: f64,
        z_in: f64,
        derivatives: &mut [f32; 3],
    ) -> f32 {
        let x = wrap(x_in) + self.state.xo();
        let y = wrap(y_in) + self.state.yo();
        let z = wrap(z_in) + self.state.zo();

        let xi = floor_to_i32(x);
        let yi = floor_to_i32(y);
        let zi = floor_to_i32(z);

        let xr = (x - f64::from(xi)) as f32;
        let yr = (y - f64::from(yi)) as f32;
        let zr = (z - f64::from(zi)) as f32;

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

        let d000 = grad_dot_f32(g000, xr, yr, zr);
        let d100 = grad_dot_f32(g100, xr - 1.0_f32, yr, zr);
        let d010 = grad_dot_f32(g010, xr, yr - 1.0_f32, zr);
        let d110 = grad_dot_f32(g110, xr - 1.0_f32, yr - 1.0_f32, zr);
        let d001 = grad_dot_f32(g001, xr, yr, zr - 1.0_f32);
        let d101 = grad_dot_f32(g101, xr - 1.0_f32, yr, zr - 1.0_f32);
        let d011 = grad_dot_f32(g011, xr, yr - 1.0_f32, zr - 1.0_f32);
        let d111 = grad_dot_f32(g111, xr - 1.0_f32, yr - 1.0_f32, zr - 1.0_f32);

        let ax = smoothstep(xr);
        let ay = smoothstep(yr);
        let az = smoothstep(zr);

        let ux = lerp3(
            ax,
            ay,
            az,
            f32::from(g000[0]),
            f32::from(g100[0]),
            f32::from(g010[0]),
            f32::from(g110[0]),
            f32::from(g001[0]),
            f32::from(g101[0]),
            f32::from(g011[0]),
            f32::from(g111[0]),
        );
        let uy = lerp3(
            ax,
            ay,
            az,
            f32::from(g000[1]),
            f32::from(g100[1]),
            f32::from(g010[1]),
            f32::from(g110[1]),
            f32::from(g001[1]),
            f32::from(g101[1]),
            f32::from(g011[1]),
            f32::from(g111[1]),
        );
        let uz = lerp3(
            ax,
            ay,
            az,
            f32::from(g000[2]),
            f32::from(g100[2]),
            f32::from(g010[2]),
            f32::from(g110[2]),
            f32::from(g001[2]),
            f32::from(g101[2]),
            f32::from(g011[2]),
            f32::from(g111[2]),
        );

        let vx = lerp2(ay, az, d100 - d000, d110 - d010, d101 - d001, d111 - d011);
        let vy = lerp2(az, ax, d010 - d000, d011 - d001, d110 - d100, d111 - d101);
        let vz = lerp2(ax, ay, d001 - d000, d101 - d100, d011 - d010, d111 - d110);

        derivatives[0] += ux + smoothstep_derivative(xr) * vx;
        derivatives[1] += uy + smoothstep_derivative(yr) * vy;
        derivatives[2] += uz + smoothstep_derivative(zr) * vz;

        lerp3(ax, ay, az, d000, d100, d010, d110, d001, d101, d011, d111)
    }

    /// Column-cached volume accumulation over `grid` in `(z, x, y)` order.
    ///
    /// Caches XZ hashes and partial dot products within each `(iz, ix)` column while `yi` remains
    /// unchanged (starting from the `i32::MIN` sentinel).
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
                    let y = wrap(f64::from(grid.block_y(iy)) * scale_y) + self.state.yo();
                    let yi = floor_to_i32(y);
                    let yr = (y - f64::from(yi)) as f32;
                    let yr1 = yr - 1.0_f32;
                    let ay = smoothstep(yr);

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

                    let d000 = dxz000 + gy000 * yr;
                    let d100 = dxz100 + gy100 * yr;
                    let d010 = dxz010 + gy010 * yr1;
                    let d110 = dxz110 + gy110 * yr1;
                    let d001 = dxz001 + gy001 * yr;
                    let d101 = dxz101 + gy101 * yr;
                    let d011 = dxz011 + gy011 * yr1;
                    let d111 = dxz111 + gy111 * yr1;

                    let value = lerp3(ax, ay, az, d000, d100, d010, d110, d001, d101, d011, d111);
                    buffer[idx] += amplitude * value;
                    idx += 1;
                }
            }
        }
    }
}

impl Noise for PerlinNoise {
    #[inline]
    fn sample(&self, x: f64, y: f64, z: f64) -> f32 {
        PerlinNoise::sample(self, x, y, z)
    }

    #[inline]
    fn sample_2d(&self, x: f64, y: f64) -> f32 {
        PerlinNoise::sample_2d(self, x, y)
    }

    #[inline]
    fn range(&self) -> Interval {
        PerlinNoise::range(self)
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
        PerlinNoise::accumulate_volume(self, grid, scale_xz, scale_y, amplitude, buffer);
    }
}
