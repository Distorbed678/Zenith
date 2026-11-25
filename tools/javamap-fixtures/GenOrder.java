import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;

/**
 * Generates deterministic Java 21 HashMap<String, Integer> iteration-order fixtures
 * for testing zenith-nbt's Compound map ordering.
 *
 * Clean-room utility using only the Java SE standard library.
 * Run: java tools/javamap-fixtures/GenOrder.java
 */
public final class GenOrder {
    private static final class Op {
        final String kind; // "put", "remove", "copy"
        final String key;

        Op(String kind, String key) {
            this.kind = kind;
            this.key = key;
        }
    }

    private static final class TestCase {
        final String name;
        final List<Op> ops;
        final List<String> expectedOrder;

        TestCase(String name, List<Op> ops, List<String> expectedOrder) {
            this.name = name;
            this.ops = ops;
            this.expectedOrder = expectedOrder;
        }
    }

    private static final class Utf16Case {
        final String name;
        final List<int[]> insertedKeys;
        final List<int[]> expectedOrder;
        final List<Integer> expectedHashes;

        Utf16Case(String name, List<int[]> insertedKeys, List<int[]> expectedOrder, List<Integer> expectedHashes) {
            this.name = name;
            this.insertedKeys = insertedKeys;
            this.expectedOrder = expectedOrder;
            this.expectedHashes = expectedHashes;
        }
    }

    // Simple deterministic xorshift64* PRNG so key generation is completely self-contained.
    private static final class Rng {
        private long state;

        Rng(long seed) {
            this.state = seed;
        }

        long nextLong() {
            long x = state;
            x ^= x >>> 12;
            x ^= x << 25;
            x ^= x >>> 27;
            state = x;
            return x * 0x2545F4914F6CDD1DL;
        }

        int nextInt(int bound) {
            return (int) (Long.remainderUnsigned(nextLong(), bound));
        }

        String nextKey(int minLen, int maxLen) {
            String alphabet = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_.-:";
            int len = minLen + nextInt(maxLen - minLen + 1);
            StringBuilder sb = new StringBuilder(len);
            for (int i = 0; i < len; i++) {
                sb.append(alphabet.charAt(nextInt(alphabet.length())));
            }
            return sb.toString();
        }
    }

    private static TestCase runCase(String name, List<Op> ops) {
        HashMap<String, Integer> map = new HashMap<>();
        int seq = 0;
        for (Op op : ops) {
            switch (op.kind) {
                case "put" -> map.put(op.key, seq++);
                case "remove" -> map.remove(op.key);
                case "copy" -> map = new HashMap<>(map);
                default -> throw new IllegalArgumentException("Unknown op: " + op.kind);
            }
        }
        return new TestCase(name, ops, new ArrayList<>(map.keySet()));
    }

    private static List<Op> putAll(List<String> keys) {
        List<Op> ops = new ArrayList<>(keys.size());
        for (String k : keys) {
            ops.add(new Op("put", k));
        }
        return ops;
    }

    private static List<String> genUniqueKeys(Rng rng, int count) {
        List<String> keys = new ArrayList<>(count);
        HashMap<String, Boolean> seen = new HashMap<>();
        while (keys.size() < count) {
            String k = rng.nextKey(1, 14);
            if (!seen.containsKey(k)) {
                seen.put(k, Boolean.TRUE);
                keys.add(k);
            }
        }
        return keys;
    }

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
                    if (c < 0x20) {
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

    private static int[] toCodeUnits(String s) {
        int[] units = new int[s.length()];
        for (int i = 0; i < s.length(); i++) {
            units[i] = s.charAt(i);
        }
        return units;
    }

    private static String fromCodeUnits(int[] units) {
        char[] chars = new char[units.length];
        for (int i = 0; i < units.length; i++) {
            chars[i] = (char) units[i];
        }
        return new String(chars);
    }

    public static void main(String[] args) throws IOException {
        List<TestCase> cases = new ArrayList<>();
        Rng rng = new Rng(0x5EED_C0DE_1234_5678L);

        // 1. Empty and single-entry
        cases.add(runCase("empty", List.of()));
        cases.add(runCase("single", List.of(new Op("put", "id"))));

        // 2. Typical NBT-like keys below initial threshold (12)
        cases.add(runCase("nbt_typical_10", putAll(List.of(
                "DataVersion", "Pos", "Motion", "Rotation", "FallDistance",
                "Fire", "Air", "OnGround", "Invulnerable", "UUID"
        ))));

        // 3. Exact resize boundary tests: 12, 13, 24, 25, 48, 49, 96, 97
        for (int n : new int[]{12, 13, 24, 25, 48, 49, 96, 97}) {
            cases.add(runCase("prng_grow_" + n, putAll(genUniqueKeys(rng, n))));
        }

        // 4. Overwriting existing keys must not change order or trigger resize
        {
            List<String> keys = genUniqueKeys(rng, 12);
            List<Op> ops = new ArrayList<>(putAll(keys));
            ops.add(new Op("put", keys.get(0)));
            ops.add(new Op("put", keys.get(5)));
            ops.add(new Op("put", keys.get(11)));
            ops.add(new Op("put", keys.get(3)));
            cases.add(runCase("overwrite_at_threshold_12", ops));
        }

        // 5. Removals and re-insertions across resizes
        {
            List<String> keys = genUniqueKeys(rng, 30);
            List<Op> ops = new ArrayList<>(putAll(keys));
            for (int i = 0; i < 30; i += 2) {
                ops.add(new Op("remove", keys.get(i)));
            }
            // Re-insert some removed keys and brand-new keys
            ops.add(new Op("put", keys.get(0)));
            ops.add(new Op("put", keys.get(6)));
            ops.add(new Op("put", keys.get(14)));
            for (String extra : genUniqueKeys(rng, 10)) {
                ops.add(new Op("put", extra));
            }
            cases.add(runCase("removals_and_reinsert", ops));
        }

        // 6. Copy constructor on fresh maps of various sizes (tests exact capacity formula)
        for (int n : new int[]{0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 12, 13, 15, 16, 17, 24, 25, 48, 49}) {
            List<Op> ops = new ArrayList<>(putAll(genUniqueKeys(rng, n)));
            ops.add(new Op("copy", ""));
            cases.add(runCase("copy_fresh_" + n, ops));
        }

        // 7. Copy constructor after growing large and removing down to various sizes
        for (int keep : new int[]{0, 1, 2, 3, 5, 6, 11, 12, 13, 24, 25}) {
            List<String> keys = genUniqueKeys(rng, 55);
            List<Op> ops = new ArrayList<>(putAll(keys));
            for (int i = keep; i < keys.size(); i++) {
                ops.add(new Op("remove", keys.get(i)));
            }
            ops.add(new Op("copy", ""));
            // Also insert 2 more keys after copy to verify threshold after copy!
            ops.add(new Op("put", "post_copy_a"));
            ops.add(new Op("put", "post_copy_b"));
            cases.add(runCase("copy_after_shrink_to_" + keep, ops));
        }

        // 8. Exact hashCode collisions (Aa / BB pairs; up to 7 in one bin, below treeify threshold 8)
        {
            List<String> colliding = List.of(
                    "AaAaAa", "AaAaBB", "AaBBAa", "AaBBBB",
                    "BBAaAa", "BBAaBB", "BBBBAa"
            );
            List<Op> ops = new ArrayList<>(putAll(colliding));
            cases.add(runCase("same_hash_7_entries", ops));

            // Remove middle entry from collision chain, then re-insert and grow past 12
            List<Op> ops2 = new ArrayList<>(putAll(colliding));
            ops2.add(new Op("remove", "AaBBBB"));
            ops2.add(new Op("put", "AaBBBB"));
            for (String extra : genUniqueKeys(rng, 10)) {
                ops2.add(new Op("put", extra));
            }
            cases.add(runCase("same_hash_7_remove_reinsert_grow", ops2));
        }

        // 9. 8 entries colliding at capacity 16 (hash & 15 == same) that split on resize to 32
        // Triggers treeifyBin's resize-instead-of-treeify path when cap < 64!
        {
            List<String> mod16Same = new ArrayList<>();
            int targetBucket = 3;
            int candidate = 0;
            while (mod16Same.size() < 9) {
                String s = "k" + candidate++;
                int h = s.hashCode();
                int spread = h ^ (h >>> 16);
                if ((spread & 15) == targetBucket) {
                    mod16Same.add(s);
                }
            }
            cases.add(runCase("bucket_collision_triggers_early_resize", putAll(mod16Same)));
        }

        // 10. Unicode, NUL, CJK, and astral (surrogate-pair) keys
        {
            List<String> uniKeys = List.of(
                    "",
                    "\u0000",
                    "\u0000\u0000",
                    "caf\u00e9",
                    "\u4e16\u754c",
                    "\uD83D\uDE00",          // U+1F600 grinning face
                    "\uD83C\uDF0D_world",    // U+1F30D earth globe
                    "a\u0000b\uD83D\uDE80",  // NUL + rocket
                    "\u007f",
                    "\u0080",
                    "\u07ff",
                    "\u0800",
                    "\uffff",
                    "prefix_\uD83E\uDD80_crab"
            );
            cases.add(runCase("unicode_and_astral_keys", putAll(uniKeys)));
        }

        // 11. UTF-16 code unit cases including lone surrogates
        List<Utf16Case> utf16Cases = new ArrayList<>();
        {
            List<int[]> rawKeys = List.of(
                    new int[]{0xD800},
                    new int[]{0xDC00},
                    new int[]{0xD800, 0x0041},
                    new int[]{0x0041, 0xDFFF},
                    new int[]{0xDC00, 0xD800}, // reversed surrogate pair
                    new int[]{0xD83D, 0xDE00}, // valid surrogate pair (U+1F600)
                    new int[]{0x0000, 0xD800, 0x0000},
                    new int[]{0x0061, 0x0062},
                    new int[]{0xD801, 0xD802, 0xDC03},
                    new int[]{0xFFFF, 0xD800},
                    new int[]{0x0030},
                    new int[]{0x0031},
                    new int[]{0x0032},
                    new int[]{0x0033}
            );
            HashMap<String, Integer> map = new HashMap<>();
            List<Integer> hashes = new ArrayList<>();
            for (int i = 0; i < rawKeys.size(); i++) {
                String s = fromCodeUnits(rawKeys.get(i));
                hashes.add(s.hashCode());
                map.put(s, i);
            }
            List<int[]> expectedOrder = new ArrayList<>();
            for (String k : map.keySet()) {
                expectedOrder.add(toCodeUnits(k));
            }
            utf16Cases.add(new Utf16Case("lone_surrogates_and_mixed", rawKeys, expectedOrder, hashes));
        }

        // Write JSON manually (zero external dependencies)
        StringBuilder json = new StringBuilder();
        json.append("{\n");
        json.append("  \"java_version\": ").append(escapeJson(System.getProperty("java.version"))).append(",\n");
        json.append("  \"cases\": [\n");
        for (int i = 0; i < cases.size(); i++) {
            TestCase tc = cases.get(i);
            json.append("    {\n");
            json.append("      \"name\": ").append(escapeJson(tc.name)).append(",\n");
            json.append("      \"ops\": [");
            for (int j = 0; j < tc.ops.size(); j++) {
                Op op = tc.ops.get(j);
                if (j > 0) json.append(", ");
                json.append("{\"op\": ").append(escapeJson(op.kind));
                if (!op.kind.equals("copy")) {
                    json.append(", \"key\": ").append(escapeJson(op.key));
                }
                json.append("}");
            }
            json.append("],\n");
            json.append("      \"expected_order\": [");
            for (int j = 0; j < tc.expectedOrder.size(); j++) {
                if (j > 0) json.append(", ");
                json.append(escapeJson(tc.expectedOrder.get(j)));
            }
            json.append("]\n");
            json.append("    }").append(i + 1 < cases.size() ? "," : "").append("\n");
        }
        json.append("  ],\n");
        json.append("  \"utf16_cases\": [\n");
        for (int i = 0; i < utf16Cases.size(); i++) {
            Utf16Case uc = utf16Cases.get(i);
            json.append("    {\n");
            json.append("      \"name\": ").append(escapeJson(uc.name)).append(",\n");
            json.append("      \"inserted_keys\": [");
            for (int j = 0; j < uc.insertedKeys.size(); j++) {
                if (j > 0) json.append(", ");
                appendIntArray(json, uc.insertedKeys.get(j));
            }
            json.append("],\n");
            json.append("      \"expected_hashes\": [");
            for (int j = 0; j < uc.expectedHashes.size(); j++) {
                if (j > 0) json.append(", ");
                json.append(uc.expectedHashes.get(j));
            }
            json.append("],\n");
            json.append("      \"expected_order\": [");
            for (int j = 0; j < uc.expectedOrder.size(); j++) {
                if (j > 0) json.append(", ");
                appendIntArray(json, uc.expectedOrder.get(j));
            }
            json.append("]\n");
            json.append("    }").append(i + 1 < utf16Cases.size() ? "," : "").append("\n");
        }
        json.append("  ]\n");
        json.append("}\n");

        Path outPath = Path.of("crates/zenith-nbt/tests/fixtures/javamap_order.json");
        Files.createDirectories(outPath.getParent());
        Files.writeString(outPath, json.toString(), StandardCharsets.UTF_8);
        System.out.println("Wrote " + cases.size() + " string cases and " + utf16Cases.size()
                + " utf16 cases to " + outPath);
    }

    private static void appendIntArray(StringBuilder sb, int[] arr) {
        sb.append('[');
        for (int i = 0; i < arr.length; i++) {
            if (i > 0) sb.append(", ");
            sb.append(arr[i]);
        }
        sb.append(']');
    }
}
