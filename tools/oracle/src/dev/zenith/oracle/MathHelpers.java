// Zenith parity oracle — original source, MIT OR Apache-2.0.
package dev.zenith.oracle;

import java.io.BufferedOutputStream;
import java.io.DataOutputStream;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;

/**
 * Reference for the {@code math-helpers} suite: calls the game's own public math helpers (by their public names) on a
 * fixed input set and writes every result as a big-endian 64-bit value (float/double raw bits; ints sign-extended).
 * A helper that throws writes {@link #REJECTED}. The Zenith side ({@code cargo xtask parity math-helpers}) runs the
 * identical probe list, which must stay in the same order as here.
 */
public final class MathHelpers {
    private MathHelpers() {}

    static final long REJECTED = 0x7EEE_DEAD_BEEF_0001L;

    static double[] doubles() {
        double[] fixed = {0.0, -0.0, 1.0, -1.0, 0.5, -0.5, 1e-9, -1e-9, 1e-300, 3.0e9, -3.0e9, 1e19, -1e19, 1e300,
            180.0, -180.0, 359.9, 720.25, 3.14159, Double.NaN, Double.POSITIVE_INFINITY, Double.NEGATIVE_INFINITY,
            2147483647.5, -2147483648.5, 9.3e18, -9.3e18};
        double[] scales = {1.0, 100.0, 1.0e4, 1.0e7, 1.0e10};
        double[] all = new double[fixed.length + 120];
        System.arraycopy(fixed, 0, all, 0, fixed.length);
        long s = 0x51AB_C0DE_1234_5678L;
        for (int i = 0; i < 120; i++) {
            s = s * 6364136223846793005L + 1442695040888963407L;
            double u = (s >>> 11) * 0x1.0p-53;
            all[fixed.length + i] = (u * 2.0 - 1.0) * scales[i % scales.length];
        }
        return all;
    }

    static float[] floats() {
        double[] d = doubles();
        float[] f = new float[d.length];
        for (int i = 0; i < d.length; i++) f[i] = (float) d[i];
        return f;
    }

    static int[] ints() {
        int[] fixed = {0, 1, -1, 2, 3, 7, 8, 1023, 1024, 1025, 1 << 30, (1 << 30) + 1, Integer.MAX_VALUE, Integer.MIN_VALUE, -2, -1024};
        int[] all = new int[fixed.length + 60];
        System.arraycopy(fixed, 0, all, 0, fixed.length);
        long s = 0x0DDB_A11C_AFE5_EED5L;
        for (int i = 0; i < 60; i++) {
            s = s * 6364136223846793005L + 1442695040888963407L;
            all[fixed.length + i] = (int) (s >>> 32);
        }
        return all;
    }

    static final double[] ALPHAS = {0.0, 1.0, 0.5, -0.5, 1.5, 0.25, Double.NaN};
    static final double[][] ENDS = {{0, 1}, {1, 0}, {-5.5, 3.25}, {1e9, -1e9}, {0.1, 0.2}, {3.0e7, 3.0e7 + 1}};

    private static DataOutputStream out;
    private static int written;

    private static void put(long v) throws Exception {
        out.writeLong(v);
        written++;
    }

    private static Object call(Method m, Object... args) throws Exception {
        try {
            return m.invoke(null, args);
        } catch (InvocationTargetException e) {
            return null;
        }
    }

    private static void putValue(Object v) throws Exception {
        if (v == null) put(REJECTED);
        else if (v instanceof Float f) put(Float.floatToRawIntBits(f));
        else if (v instanceof Double d) put(Double.doubleToRawLongBits(d));
        else if (v instanceof Integer i) put(i);
        else if (v instanceof Long l) put(l);
        else throw new IllegalStateException("unexpected result type " + v.getClass());
    }

    public static void main(String[] args) throws Exception {
        Class<?> mth = Class.forName("net.minecraft.util.Mth");
        Class<?> blockPos = Class.forName("net.minecraft.core.BlockPos");
        double[] d = doubles();
        float[] f = floats();
        int[] in = ints();
        Class<?> D = double.class, F = float.class, I = int.class, L = long.class;
        try (DataOutputStream o = new DataOutputStream(new BufferedOutputStream(Files.newOutputStream(Path.of(args[0]))))) {
            out = o;
            for (String n : new String[] {"sin", "cos"}) for (double x : d) putValue(call(mth.getMethod(n, D), x));
            for (int i = 0; i < d.length; i++) putValue(call(mth.getMethod("atan2", D, D), d[i], d[(i * 7 + 3) % d.length]));
            for (double x : d) putValue(call(mth.getMethod("fastInvSqrt", D), x));
            for (float x : f) putValue(call(mth.getMethod("fastInvCubeRoot", F), x));
            for (float x : f) putValue(call(mth.getMethod("invSqrt", F), x));
            for (double x : d) putValue(call(mth.getMethod("invSqrt", D), x));
            for (float x : f) putValue(call(mth.getMethod("floor", F), x));
            for (double x : d) putValue(call(mth.getMethod("floor", D), x));
            for (double x : d) putValue(call(mth.getMethod("lfloor", D), x));
            for (float x : f) putValue(call(mth.getMethod("ceil", F), x));
            for (double x : d) putValue(call(mth.getMethod("ceil", D), x));
            for (float x : f) putValue(call(mth.getMethod("frac", F), x));
            for (double x : d) putValue(call(mth.getMethod("frac", D), x));
            for (int x : in) putValue(call(mth.getMethod("wrapDegrees", I), x));
            for (float x : f) putValue(call(mth.getMethod("wrapDegrees", F), x));
            for (double x : d) putValue(call(mth.getMethod("wrapDegrees", D), x));
            for (double a : ALPHAS) for (double[] e : ENDS) putValue(call(mth.getMethod("lerp", F, F, F), (float) a, (float) e[0], (float) e[1]));
            for (double a : ALPHAS) for (double[] e : ENDS) putValue(call(mth.getMethod("lerp", D, D, D), a, e[0], e[1]));
            for (double a : ALPHAS) for (double[] e : ENDS) putValue(call(mth.getMethod("clampedLerp", D, D, D), a, e[0], e[1]));
            for (double a : ALPHAS) for (double[] e : ENDS) putValue(call(mth.getMethod("inverseLerp", D, D, D), a, e[0], e[1]));
            for (float x : f) putValue(call(mth.getMethod("smoothstep", F), x));
            for (int i = 0; i < in.length; i++) {
                int m = in[(i * 5 + 1) % in.length];
                putValue(m == 0 ? null : call(mth.getMethod("positiveModulo", I, I), in[i], m));
            }
            for (String n : new String[] {"ceillog2", "log2", "smallestEncompassingPowerOfTwo"}) for (int x : in) putValue(call(mth.getMethod(n, I), x));
            for (int i = 0; i < 60; i++) {
                float h = (float) ((i * 0.618034) % 1.0);
                float s = (float) ((i * 0.414214) % 1.0);
                float v = (float) ((i * 0.732051) % 1.0);
                putValue(call(mth.getMethod("hsvToRgb", F, F, F), h, s, v));
            }
            for (double x : d) putValue(call(mth.getMethod("sign", D), x));
            for (int i = 0; i < in.length; i++) putValue(call(blockPos.getMethod("asLong", I, I, I), in[i], in[(i * 3 + 1) % in.length], in[(i * 11 + 2) % in.length]));
            long s = 0x0B10_C0B0_5EED_0001L;
            for (int i = 0; i < 80; i++) {
                s = s * 6364136223846793005L + 1442695040888963407L;
                for (String n : new String[] {"getX", "getY", "getZ"}) putValue(call(blockPos.getMethod(n, L), s));
            }
        }
        System.out.println("[zenith-oracle] math helpers: " + written + " values");
    }
}
