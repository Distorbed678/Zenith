import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.util.Random;
import java.util.random.RandomGenerator;
import java.util.random.RandomGeneratorFactory;

/**
 * Generates deterministic JDK 21 reference vectors for zenith-math RNG tests.
 *
 * Clean-room utility using only the Java SE standard library.
 * Run: java tools/javamap-fixtures/RngFixtures.java > crates/zenith-math/tests/fixtures/rng_jdk.json
 */
public final class RngFixtures {
    private static final float SPEC_F = 5.9604645E-8F;
    // The spec constant: the float literal widened to double (exactly 2^-53; see random.md correction).
    private static final double SPEC_D = (double) 1.110223E-16F;
    private static final double SPEC_D_FROM_FLOAT = (double) 1.110223E-16F;

    /** Subclass of java.util.Random solely to expose protected next(int) for spec-formula checks. */
    private static final class ExposedRandom extends Random {
        ExposedRandom(long seed) {
            super(seed);
        }

        int nextBits(int bits) {
            return next(bits);
        }

        double specNextDouble() {
            long hi = nextBits(26);
            long lo = nextBits(27);
            return ((hi << 27) + lo) * SPEC_D;
        }
    }

    private static final long[] LEGACY_SEEDS = {
        0L,
        1L,
        -1L,
        42L,
        0x5DEECE66DL,
        Long.MIN_VALUE,
        Long.MAX_VALUE,
        0x123456789ABCDEF0L
    };

    private static final int[] LEGACY_BOUNDS = {
        1,
        2,
        3,
        4,
        7,
        8,
        15,
        16,
        17,
        100,
        1000,
        65536,
        1_000_000_007,
        0x40000000,
        0x40000001,
        0x60000000,
        Integer.MAX_VALUE
    };

    private static final long[][] XOROSHIRO_STATES = {
        {1L, 2L},
        {0L, 1L},
        {1L, 0L},
        {0x9E3779B97F4A7C15L, 0x6A09E667F3BCC909L},
        {-1L, -1L},
        {0x123456789ABCDEF0L, 0xFEDCBA9876543210L},
        {Long.MIN_VALUE, Long.MAX_VALUE}
    };

    private static final String[] SAMPLE_STRINGS = {
        "",
        "a",
        "hello",
        "polygenelubricants",
        "overworld",
        "caf\u00e9",
        "\u65e5\u672c\u8a9e",
        "\u03a9\u2248\u00e7\u221a\u222b",
        "\uD83D\uDE00",
        "a\uD83C\uDF89b\uD83D\uDE80c",
        "\uD834\uDD1E"
    };

    private static String escapeJson(String s) {
        StringBuilder sb = new StringBuilder(s.length() + 2);
        sb.append('"');
        for (int i = 0; i < s.length(); i++) {
            char c = s.charAt(i);
            switch (c) {
                case '"' -> sb.append("\\\"");
                case '\\' -> sb.append("\\\\");
                case '\b' -> sb.append("\\b");
                case '\f' -> sb.append("\\f");
                case '\n' -> sb.append("\\n");
                case '\r' -> sb.append("\\r");
                case '\t' -> sb.append("\\t");
                default -> {
                    if (c < 0x20 || c > 0x7E) {
                        sb.append(String.format("\\u%04x", (int) c));
                    } else {
                        sb.append(c);
                    }
                }
            }
        }
        sb.append('"');
        return sb.toString();
    }

    private static void appendIntArray(StringBuilder sb, int[] values) {
        sb.append('[');
        for (int i = 0; i < values.length; i++) {
            if (i > 0) sb.append(", ");
            sb.append(values[i]);
        }
        sb.append(']');
    }

    private static void appendLongArray(StringBuilder sb, long[] values) {
        sb.append('[');
        for (int i = 0; i < values.length; i++) {
            if (i > 0) sb.append(", ");
            sb.append(values[i]);
        }
        sb.append(']');
    }

    private static void appendBoolArray(StringBuilder sb, boolean[] values) {
        sb.append('[');
        for (int i = 0; i < values.length; i++) {
            if (i > 0) sb.append(", ");
            sb.append(values[i]);
        }
        sb.append(']');
    }

    private static void appendUnsignedIntArray(StringBuilder sb, int[] values) {
        sb.append('[');
        for (int i = 0; i < values.length; i++) {
            if (i > 0) sb.append(", ");
            sb.append(Integer.toUnsignedString(values[i]));
        }
        sb.append(']');
    }

    public static void main(String[] args) throws Exception {
        MessageDigest md5 = MessageDigest.getInstance("MD5");
        RandomGeneratorFactory<RandomGenerator> xoroFactory =
            RandomGeneratorFactory.of("Xoroshiro128PlusPlus");

        StringBuilder out = new StringBuilder(65536);
        out.append("{\n");
        out.append("  \"jdkVersion\": ").append(escapeJson(System.getProperty("java.version"))).append(",\n");

        // Constants
        out.append("  \"constants\": {\n");
        out.append("    \"fBits\": ").append(Integer.toUnsignedString(Float.floatToRawIntBits(SPEC_F))).append(",\n");
        out.append("    \"dFloatWidenedBits\": ").append(Long.toUnsignedString(Double.doubleToRawLongBits(SPEC_D_FROM_FLOAT))).append(",\n");
        out.append("    \"powNeg53Bits\": ")
            .append(Long.toUnsignedString(Double.doubleToRawLongBits(Math.scalb(1.0, -53))))
            .append("\n");
        out.append("  },\n");

        // String.hashCode and MD5 name seeds
        out.append("  \"strings\": [\n");
        for (int i = 0; i < SAMPLE_STRINGS.length; i++) {
            String s = SAMPLE_STRINGS[i];
            byte[] digest = md5.digest(s.getBytes(StandardCharsets.UTF_8));
            ByteBuffer bb = ByteBuffer.wrap(digest);
            long lo = bb.getLong();
            long hi = bb.getLong();
            out.append("    { \"text\": ").append(escapeJson(s))
                .append(", \"hashCode\": ").append(s.hashCode())
                .append(", \"md5Lo\": ").append(lo)
                .append(", \"md5Hi\": ").append(hi)
                .append(" }").append(i + 1 < SAMPLE_STRINGS.length ? ",\n" : "\n");
        }
        out.append("  ],\n");

        // java.util.Random (Legacy) vectors
        out.append("  \"legacy\": [\n");
        for (int si = 0; si < LEGACY_SEEDS.length; si++) {
            long seed = LEGACY_SEEDS[si];
            out.append("    {\n");
            out.append("      \"seed\": ").append(seed).append(",\n");

            Random rInts = new Random(seed);
            int[] ints = new int[16];
            for (int i = 0; i < ints.length; i++) ints[i] = rInts.nextInt();
            out.append("      \"nextInt\": ");
            appendIntArray(out, ints);
            out.append(",\n");

            Random rLongs = new Random(seed);
            long[] longs = new long[16];
            for (int i = 0; i < longs.length; i++) longs[i] = rLongs.nextLong();
            out.append("      \"nextLong\": ");
            appendLongArray(out, longs);
            out.append(",\n");

            Random rBools = new Random(seed);
            boolean[] bools = new boolean[32];
            for (int i = 0; i < bools.length; i++) bools[i] = rBools.nextBoolean();
            out.append("      \"nextBoolean\": ");
            appendBoolArray(out, bools);
            out.append(",\n");

            Random rFloats = new Random(seed);
            int[] floatBits = new int[16];
            for (int i = 0; i < floatBits.length; i++) {
                floatBits[i] = Float.floatToRawIntBits(rFloats.nextFloat());
            }
            out.append("      \"nextFloatBits\": ");
            appendUnsignedIntArray(out, floatBits);
            out.append(",\n");

            ExposedRandom rSpecDoubles = new ExposedRandom(seed);
            long[] specDoubleBits = new long[16];
            for (int i = 0; i < specDoubleBits.length; i++) {
                specDoubleBits[i] = Double.doubleToRawLongBits(rSpecDoubles.specNextDouble());
            }
            out.append("      \"specNextDoubleBits\": ");
            appendLongArray(out, specDoubleBits);
            out.append(",\n");

            Random rJdkDoubles = new Random(seed);
            long[] jdkDoubleBits = new long[16];
            for (int i = 0; i < jdkDoubleBits.length; i++) {
                jdkDoubleBits[i] = Double.doubleToRawLongBits(rJdkDoubles.nextDouble());
            }
            out.append("      \"jdkNextDoubleBits\": ");
            appendLongArray(out, jdkDoubleBits);
            out.append(",\n");

            out.append("      \"bounded\": [\n");
            for (int bi = 0; bi < LEGACY_BOUNDS.length; bi++) {
                int bound = LEGACY_BOUNDS[bi];
                Random rBound = new Random(seed);
                int[] vals = new int[16];
                for (int i = 0; i < vals.length; i++) vals[i] = rBound.nextInt(bound);
                out.append("        { \"bound\": ").append(bound).append(", \"values\": ");
                appendIntArray(out, vals);
                out.append(" }").append(bi + 1 < LEGACY_BOUNDS.length ? ",\n" : "\n");
            }
            out.append("      ]\n");

            out.append("    }").append(si + 1 < LEGACY_SEEDS.length ? ",\n" : "\n");
        }
        out.append("  ],\n");

        // jdk.random.Xoroshiro128PlusPlus raw nextLong vectors
        out.append("  \"xoroshiro128PlusPlus\": [\n");
        for (int xi = 0; xi < XOROSHIRO_STATES.length; xi++) {
            long lo = XOROSHIRO_STATES[xi][0];
            long hi = XOROSHIRO_STATES[xi][1];
            byte[] seedBytes = ByteBuffer.allocate(16).putLong(lo).putLong(hi).array();
            RandomGenerator gen = xoroFactory.create(seedBytes);
            long[] vals = new long[32];
            for (int i = 0; i < vals.length; i++) vals[i] = gen.nextLong();
            out.append("    {\n");
            out.append("      \"lo\": ").append(lo).append(",\n");
            out.append("      \"hi\": ").append(hi).append(",\n");
            out.append("      \"nextLong\": ");
            appendLongArray(out, vals);
            out.append("\n    }").append(xi + 1 < XOROSHIRO_STATES.length ? ",\n" : "\n");
        }
        out.append("  ]\n");
        out.append("}\n");

        System.out.print(out);
    }
}
