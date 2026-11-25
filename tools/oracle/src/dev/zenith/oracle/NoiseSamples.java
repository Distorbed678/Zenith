// Zenith parity oracle — original source, MIT OR Apache-2.0.
package dev.zenith.oracle;

import java.io.BufferedOutputStream;
import java.io.DataOutputStream;
import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;

/**
 * Reference for the {@code noise-samples} suite: constructs the game's own noise generators from fixed random
 * sources and samples them at fixed points, writing every float result as raw bits (sign-extended to 64 bits).
 * The Zenith side ({@code cargo xtask parity noise-samples}) runs the identical case list.
 *
 * <p>Game classes are reached reflectively by their public names (facts listed with javap).
 */
public final class NoiseSamples {
    private NoiseSamples() {}

    static final long[] SEEDS = {0L, 1L, 42L, -7046029254386353131L, 1234567890123L};
    static final double[] SMEARS = {80.0, 8.0, 1.0};
    static final int[] PARITY_FIRST = {-7, -10, -4, -3};
    static final double[][] PARITY_AMPS = {{1.0, 1.0}, {1.5, 0.0, 1.0, 0.0, 0.0, 0.0}, {1.0}, {1.0, 1.0, 0.0, 1.0, 1.0}};
    static final int[] FBM_FIRST = {-7, -3};
    static final double[][] FBM_AMPS = {{1.0, 1.0}, {1.0, 0.0, 0.5}};
    static final int[] SMEARED_FBM_FIRST = {-15, -7};

    /** Sample points: fixed edge cases, then 200 pseudo-random points. Identical on the Rust side. */
    static double[][] points() {
        double[][] fixed = {
            {0, 0, 0}, {0.5, 0.25, -0.75}, {1, 2, 3}, {-1.5, 64, 1000.25}, {12345.678, -63.9, 98765.4321},
            {16777215.5, 0, -16777215.5}, {16777216.0, 1, -16777216.0}, {3.0e7, 100, -3.0e7}, {1e-9, 1e-9, 1e-9},
            {-0.0, 0.0, -0.0}
        };
        double[][] all = new double[fixed.length + 200][];
        System.arraycopy(fixed, 0, all, 0, fixed.length);
        long s = 0x2545F4914F6CDD1DL;
        for (int i = 0; i < 200; i++) {
            double[] p = new double[3];
            for (int k = 0; k < 3; k++) {
                s = s * 6364136223846793005L + 1442695040888963407L;
                // 53 random bits → [0,1), then into [-1e6, 1e6)
                double u = (s >>> 11) * 0x1.0p-53;
                p[k] = (u * 2.0 - 1.0) * 1.0e6;
            }
            all[fixed.length + i] = p;
        }
        return all;
    }

    private static Object rng(boolean legacy, long seed) throws Exception {
        String cls = legacy ? "net.minecraft.world.level.levelgen.LegacyRandomSource" : "net.minecraft.world.level.levelgen.XoroshiroRandomSource";
        return Class.forName(cls).getConstructor(long.class).newInstance(seed);
    }

    /** Sentinel for every sample of a noise whose construction threw (matches REJECTED on the Rust side). */
    static final long REJECTED = -1L;

    interface Maker {
        Object make() throws Exception;
    }

    /** Construct, or return null if the game rejects the configuration. */
    private static Object attempt(Maker m) throws Exception {
        try {
            return m.make();
        } catch (java.lang.reflect.InvocationTargetException rejected) {
            return null;
        }
    }

    private static long sample3(Object noise, double[] p) throws Exception {
        return noise == null ? REJECTED : Float.floatToRawIntBits(get3(noise, p));
    }

    // Call through the public Noise interface: some instances are of non-public nested classes.
    private static Method noiseGet3;
    private static Method noiseGet2;

    private static float get3(Object noise, double[] p) throws Exception {
        if (noiseGet3 == null) {
            noiseGet3 = Class.forName("net.minecraft.world.level.levelgen.synth.Noise").getMethod("get", double.class, double.class, double.class);
        }
        return (Float) noiseGet3.invoke(noise, p[0], p[1], p[2]);
    }

    private static float get2(Object noise, double[] p) throws Exception {
        if (noiseGet2 == null) {
            noiseGet2 = Class.forName("net.minecraft.world.level.levelgen.synth.Noise").getMethod("get", double.class, double.class);
        }
        return (Float) noiseGet2.invoke(noise, p[0], p[2]);
    }

    public static void main(String[] args) throws Exception {
        // Some noise classes touch registries, which the game only allows after its own bootstrap.
        Class.forName("net.minecraft.SharedConstants").getMethod("tryDetectVersion").invoke(null);
        Class.forName("net.minecraft.server.Bootstrap").getMethod("bootStrap").invoke(null);
        String synth = "net.minecraft.world.level.levelgen.synth.";
        Class<?> rs = Class.forName("net.minecraft.util.RandomSource");
        Class<?> perlin = Class.forName(synth + "PerlinNoise");
        Class<?> simplex = Class.forName(synth + "SimplexNoise");
        Class<?> smeared = Class.forName(synth + "SmearedPerlinNoise");
        Class<?> normal = Class.forName(synth + "NormalNoise");
        Class<?> legacyFbm = Class.forName(synth + "LegacyFbmInitializer");
        Class<?> blended = Class.forName(synth + "BlendedNoise");
        Class<?> doubleList = Class.forName("it.unimi.dsi.fastutil.doubles.DoubleList");
        Method wrap = Class.forName("it.unimi.dsi.fastutil.doubles.DoubleArrayList").getMethod("wrap", double[].class);
        Method createParity = normal.getMethod("createParity", int.class, double[].class);
        Method create = normal.getMethod("create", rs);
        Method createNether = normal.getMethod("createForLegacyNetherBiome", rs);
        Method fbmNether = legacyFbm.getMethod("createForLegacyNetherBiome", rs, int.class, doubleList);
        Method smearedFbm = blended.getMethod("createFbm", rs, int.class, double.class, double.class);
        double[][] pts = points();

        int written = 0;
        try (DataOutputStream out = new DataOutputStream(new BufferedOutputStream(Files.newOutputStream(Path.of(args[0]))))) {
            for (boolean legacy : new boolean[] {false, true}) {
                for (long seed : SEEDS) {
                    // 1. Perlin: 3D, 2D, value + derivatives
                    Object pn = perlin.getConstructor(rs).newInstance(rng(legacy, seed));
                    Method deriv = perlin.getMethod("noiseWithDerivative", double.class, double.class, double.class, float[].class);
                    for (double[] p : pts) {
                        out.writeLong(Float.floatToRawIntBits(get3(pn, p)));
                        out.writeLong(Float.floatToRawIntBits(get2(pn, p)));
                        float[] d = new float[3];
                        out.writeLong(Float.floatToRawIntBits((Float) deriv.invoke(pn, p[0], p[1], p[2], d)));
                        for (float v : d) out.writeLong(Float.floatToRawIntBits(v));
                        written += 6;
                    }
                    // 2. Simplex: 2D and 3D
                    Object sn = simplex.getConstructor(rs).newInstance(rng(legacy, seed));
                    for (double[] p : pts) {
                        out.writeLong(Float.floatToRawIntBits(get2(sn, p)));
                        out.writeLong(Float.floatToRawIntBits(get3(sn, p)));
                        written += 2;
                    }
                    // 3. Smeared Perlin
                    for (double smear : SMEARS) {
                        Object mn = smeared.getConstructor(rs, double.class).newInstance(rng(legacy, seed), smear);
                        for (double[] p : pts) {
                            out.writeLong(Float.floatToRawIntBits(get3(mn, p)));
                            written++;
                        }
                    }
                    // 4. Normal noise (parity parameters), standard and legacy-Nether instantiation
                    for (int c = 0; c < PARITY_FIRST.length; c++) {
                        final int cc = c;
                        final boolean lg = legacy;
                        final long sd = seed;
                        Object params = attempt(() -> createParity.invoke(null, PARITY_FIRST[cc], PARITY_AMPS[cc]));
                        Object std = params == null ? null : attempt(() -> create.invoke(params, rng(lg, sd)));
                        Object nether = params == null ? null : attempt(() -> createNether.invoke(params, rng(lg, sd)));
                        for (double[] p : pts) {
                            out.writeLong(sample3(std, p));
                            out.writeLong(sample3(nether, p));
                            written += 2;
                        }
                    }
                    // 5. Legacy FBM stacks
                    for (int c = 0; c < FBM_FIRST.length; c++) {
                        final int cc = c;
                        final boolean lg = legacy;
                        final long sd = seed;
                        Object stack = attempt(() -> fbmNether.invoke(null, rng(lg, sd), FBM_FIRST[cc], wrap.invoke(null, (Object) FBM_AMPS[cc])));
                        for (double[] p : pts) {
                            out.writeLong(sample3(stack, p));
                            written++;
                        }
                    }
                    // 6. Smeared FBM stacks (blended noise building blocks)
                    for (int first : SMEARED_FBM_FIRST) {
                        final boolean lg = legacy;
                        final long sd = seed;
                        Object stack = attempt(() -> smearedFbm.invoke(null, rng(lg, sd), first, 80.0, 1.0));
                        for (double[] p : pts) {
                            out.writeLong(sample3(stack, p));
                            written++;
                        }
                    }
                }
            }
        }
        System.out.println("[zenith-oracle] noise samples: " + written + " values");
    }
}
