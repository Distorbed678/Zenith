// Zenith parity oracle — original source, MIT OR Apache-2.0.
package dev.zenith.oracle;

import java.io.BufferedOutputStream;
import java.io.DataOutputStream;
import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;

/**
 * Reference for the {@code rng-streams} suite: runs a fixed script of random-source operations on the game's own
 * random sources (legacy and xoroshiro) and writes every result as a big-endian 64-bit value. The Zenith side
 * ({@code cargo xtask parity rng-streams}) runs the identical script and compares value by value.
 *
 * <p>Floats/doubles are written as raw bits; ints and booleans are sign-extended / 0-1. Game classes are reached
 * reflectively by their public names, so this compiles without the game on the classpath.
 */
public final class RngStreams {
    static final int[] BOUNDS = {1, 2, 3, 7, 10, 16, 100, 255, 256, 1000, 1 << 30, Integer.MAX_VALUE, 1_000_000_007};
    static final int[][] COORDS = {
        {0, 0, 0}, {1, 2, 3}, {-1, -64, -1}, {1000, 64, -1000}, {30_000_000, 319, -30_000_000},
        {Integer.MAX_VALUE, 0, Integer.MIN_VALUE}, {-123456, 7, 654321}, {686, -10, 2147483}
    };
    static final String[] NAMES = {"", "minecraft:terrain", "octave_-7", "é✓🎲", "minecraft:aquifer"};

    private RngStreams() {}

    static long[] seeds() {
        long[] fixed = {0L, 1L, -1L, 42L, 12345L, Long.MIN_VALUE, Long.MAX_VALUE, 0x5DEECE66DL, -7046029254386353131L};
        long[] all = new long[fixed.length + 100];
        System.arraycopy(fixed, 0, all, 0, fixed.length);
        long s = 1L;
        for (int i = 0; i < 100; i++) {
            s = s * 6364136223846793005L + 1442695040888963407L;
            all[fixed.length + i] = s;
        }
        return all;
    }

    private static Method m(Class<?> c, String name, Class<?>... args) throws NoSuchMethodException {
        return c.getMethod(name, args);
    }

    public static void main(String[] args) throws Exception {
        Class<?> rs = Class.forName("net.minecraft.util.RandomSource");
        Class<?> prf = Class.forName("net.minecraft.world.level.levelgen.PositionalRandomFactory");
        Class<?> mth = Class.forName("net.minecraft.util.Mth");
        Class<?>[] kinds = {
            Class.forName("net.minecraft.world.level.levelgen.LegacyRandomSource"),
            Class.forName("net.minecraft.world.level.levelgen.XoroshiroRandomSource")
        };
        Ops ops = new Ops(rs, prf);
        Method getSeed = m(mth, "getSeed", int.class, int.class, int.class);

        int written = 0;
        try (DataOutputStream out = new DataOutputStream(new BufferedOutputStream(Files.newOutputStream(Path.of(args[0]))))) {
            for (Class<?> kind : kinds) {
                for (long seed : seeds()) {
                    Object r = kind.getConstructor(long.class).newInstance(seed);
                    written += script(ops, r, seed, out);
                }
            }
            for (int[] c : COORDS) {
                out.writeLong((Long) getSeed.invoke(null, c[0], c[1], c[2]));
                written++;
            }
        }
        System.out.println("[zenith-oracle] rng streams: " + written + " values");
    }

    static final class Ops {
        final Method nextInt, nextIntB, nextLong, nextBool, nextFloat, nextDouble, nextGauss, setSeed, fork, forkPos, consume;
        final Method at, fromHash, fromSeed;

        Ops(Class<?> rs, Class<?> prf) throws NoSuchMethodException {
            nextInt = m(rs, "nextInt");
            nextIntB = m(rs, "nextInt", int.class);
            nextLong = m(rs, "nextLong");
            nextBool = m(rs, "nextBoolean");
            nextFloat = m(rs, "nextFloat");
            nextDouble = m(rs, "nextDouble");
            nextGauss = m(rs, "nextGaussian");
            setSeed = m(rs, "setSeed", long.class);
            fork = m(rs, "fork");
            forkPos = m(rs, "forkPositional");
            consume = m(rs, "consumeCount", int.class);
            at = m(prf, "at", int.class, int.class, int.class);
            fromHash = m(prf, "fromHashOf", String.class);
            fromSeed = m(prf, "fromSeed", long.class);
        }
    }

    /** The shared script. Must stay identical to the Rust side in xtask/src/parity.rs. */
    static int script(Ops o, Object r, long seed, DataOutputStream out) throws Exception {
        int n = 0;
        for (int i = 0; i < 16; i++, n++) out.writeLong((Integer) o.nextInt.invoke(r));
        for (int round = 0; round < 2; round++) {
            for (int b : BOUNDS) {
                out.writeLong((Integer) o.nextIntB.invoke(r, b));
                n++;
            }
        }
        for (int i = 0; i < 8; i++, n++) out.writeLong((Long) o.nextLong.invoke(r));
        for (int i = 0; i < 8; i++, n++) out.writeLong((Boolean) o.nextBool.invoke(r) ? 1 : 0);
        for (int i = 0; i < 16; i++, n++) out.writeLong(Float.floatToRawIntBits((Float) o.nextFloat.invoke(r)));
        for (int i = 0; i < 16; i++, n++) out.writeLong(Double.doubleToRawLongBits((Double) o.nextDouble.invoke(r)));
        for (int i = 0; i < 16; i++, n++) out.writeLong(Double.doubleToRawLongBits((Double) o.nextGauss.invoke(r)));

        o.setSeed.invoke(r, seed * 31 + 7);
        for (int i = 0; i < 4; i++, n++) out.writeLong((Integer) o.nextInt.invoke(r));
        for (int i = 0; i < 2; i++, n++) out.writeLong(Double.doubleToRawLongBits((Double) o.nextGauss.invoke(r)));

        Object f = o.fork.invoke(r);
        for (int i = 0; i < 8; i++, n++) out.writeLong((Long) o.nextLong.invoke(f));
        for (int i = 0; i < 2; i++, n++) out.writeLong((Long) o.nextLong.invoke(r));

        Object p = o.forkPos.invoke(r);
        for (int[] c : COORDS) {
            Object q = o.at.invoke(p, c[0], c[1], c[2]);
            for (int i = 0; i < 3; i++, n++) out.writeLong((Long) o.nextLong.invoke(q));
            out.writeLong(Double.doubleToRawLongBits((Double) o.nextDouble.invoke(q)));
            n++;
        }
        for (String name : NAMES) {
            Object q = o.fromHash.invoke(p, name);
            for (int i = 0; i < 3; i++, n++) out.writeLong((Long) o.nextLong.invoke(q));
        }
        for (long s : new long[] {seed, 0L}) {
            Object q = o.fromSeed.invoke(p, s);
            for (int i = 0; i < 3; i++, n++) out.writeLong((Long) o.nextLong.invoke(q));
        }

        o.consume.invoke(r, 5);
        for (int i = 0; i < 2; i++, n++) out.writeLong((Long) o.nextLong.invoke(r));
        return n;
    }
}
