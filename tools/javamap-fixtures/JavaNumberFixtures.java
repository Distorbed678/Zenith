// Zenith fixture generator — original source, MIT OR Apache-2.0. JDK only (no game code).
// Prints Double.toString / Float.toString for special and pseudo-random bit patterns as JSON lines.
// Usage: java tools/javamap-fixtures/JavaNumberFixtures.java > crates/zenith-nbt/tests/fixtures/java_numbers.jsonl
import java.util.SplittableRandom;

public class JavaNumberFixtures {
    static void d(long bits) {
        double v = Double.longBitsToDouble(bits);
        if (!Double.isNaN(v)) System.out.println("{\"t\":\"d\",\"bits\":\"" + Long.toHexString(bits) + "\",\"s\":\"" + Double.toString(v) + "\"}");
    }
    static void f(int bits) {
        float v = Float.intBitsToFloat(bits);
        if (!Float.isNaN(v)) System.out.println("{\"t\":\"f\",\"bits\":\"" + Integer.toHexString(bits) + "\",\"s\":\"" + Float.toString(v) + "\"}");
    }
    public static void main(String[] args) {
        long[] ds = {1L, 2L, 3L, 0x000fffffffffffffL, 0x0010000000000000L, Double.doubleToRawLongBits(0.001),
            Double.doubleToRawLongBits(1e7), Double.doubleToRawLongBits(1e-3) - 1, Double.doubleToRawLongBits(1e7) - 1,
            Double.doubleToRawLongBits(Double.MAX_VALUE), Double.doubleToRawLongBits(0.1), Double.doubleToRawLongBits(2e-323)};
        for (long b : ds) { d(b); d(b | 0x8000000000000000L); }
        int[] fs = {1, 2, 3, 0x007fffff, 0x00800000, Float.floatToRawIntBits(-36.8203125f), Float.floatToRawIntBits(1e7f),
            Float.floatToRawIntBits(1e-3f), Float.floatToRawIntBits(Float.MAX_VALUE), Float.floatToRawIntBits(0.1f)};
        for (int b : fs) { f(b); f(b | 0x80000000); }
        SplittableRandom r = new SplittableRandom(0x5EED);
        for (int i = 0; i < 6000; i++) d(r.nextLong());
        for (int i = 0; i < 6000; i++) f(r.nextInt());
        // Values near powers of two (asymmetric rounding intervals) and small integers / simple fractions.
        for (int e = -1074; e < 1024; e += 7) { long b = Double.doubleToRawLongBits(Math.scalb(1.0, e)); d(b); d(b - 1); d(b + 1); }
        for (int e = -149; e < 128; e += 3) { int b = Float.floatToRawIntBits(Math.scalb(1.0f, e)); f(b); f(b - 1); f(b + 1); }
        for (int i = 0; i < 500; i++) { d(Double.doubleToRawLongBits(i / 8.0)); f(Float.floatToRawIntBits(i / 16.0f)); }
    }
}
