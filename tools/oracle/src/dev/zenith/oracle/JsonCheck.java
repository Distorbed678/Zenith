// Zenith parity oracle — original source, MIT OR Apache-2.0.
package dev.zenith.oracle;

import java.io.BufferedInputStream;
import java.io.BufferedOutputStream;
import java.io.ByteArrayInputStream;
import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.io.EOFException;
import java.io.InputStreamReader;
import java.io.Reader;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Map;
import java.util.Optional;

/**
 * Reference for the {@code datapack-load} suite (stage 1: the JSON layer).
 *
 * <p>Modes ({@code args[2]}):
 * <ul>
 *   <li>{@code tree}: each input is parsed with the game's strict data-file parser; {@code metadata}: with the
 *       read path used for {@code pack.mcmeta} and {@code .mcmeta} files. Output per input: status byte (0 parsed, 1 rejected) then, if parsed, a canonical dump of
 *       the tree as raw UTF-16BE code units (u32 unit count first).</li>
 *   <li>{@code numbers}: each input is a number text; it is parsed strictly and then decoded with the game's bundled
 *       primitive codecs (byte, short, int, long, float, double). Output per (input, codec): status byte then 8
 *       bytes (sign-extended integer or raw float/double bits).</li>
 * </ul>
 * Input: repeated {@code [u32 length][UTF-8 bytes]}. Game classes are reached by their public names.
 */
public final class JsonCheck {
    private JsonCheck() {}

    public static void main(String[] args) throws Exception {
        String mode = args[2];
        // "metadata": the read path for pack.mcmeta and .mcmeta sidecars, which requires a JSON object at the root.
        Class<?> parserClass = Class.forName(mode.equals("metadata") ? "net.minecraft.util.GsonHelper" : "net.minecraft.util.StrictJsonParser");
        Method parse = parserClass.getMethod("parse", Reader.class);
        int items = 0;
        try (DataInputStream in = new DataInputStream(new BufferedInputStream(Files.newInputStream(Path.of(args[0]))));
                DataOutputStream out = new DataOutputStream(new BufferedOutputStream(Files.newOutputStream(Path.of(args[1]))))) {
            NumberProbe numbers = mode.equals("numbers") ? new NumberProbe() : null;
            while (true) {
                int len;
                try {
                    len = in.readInt();
                } catch (EOFException end) {
                    break;
                }
                byte[] input = in.readNBytes(len);
                Object tree;
                try {
                    tree = parse.invoke(null, new InputStreamReader(new ByteArrayInputStream(input), StandardCharsets.UTF_8));
                } catch (Throwable rejected) {
                    tree = null;
                }
                if (numbers != null) {
                    numbers.probe(tree, out);
                } else if (tree == null) {
                    out.writeByte(1);
                } else {
                    StringBuilder dump = new StringBuilder();
                    canonical(tree, dump);
                    out.writeByte(0);
                    out.writeInt(dump.length());
                    for (int i = 0; i < dump.length(); i++) {
                        out.writeChar(dump.charAt(i));
                    }
                }
                items++;
            }
        }
        System.out.println("[zenith-oracle] json " + mode + ": " + items + " inputs");
    }

    /** Canonical tree dump; must match the Rust side in xtask/src/parity.rs. Reflection only (no compile-time
     *  dependency on the game's bundled JSON library). */
    static void canonical(Object el, StringBuilder b) throws Exception {
        if ((Boolean) call(el, "isJsonNull")) {
            b.append('n');
        } else if ((Boolean) call(el, "isJsonPrimitive")) {
            Object p = call(el, "getAsJsonPrimitive");
            if ((Boolean) call(p, "isBoolean")) {
                b.append((Boolean) call(p, "getAsBoolean") ? 't' : 'f');
            } else if ((Boolean) call(p, "isNumber")) {
                b.append('#').append(call(p, "getAsNumber").toString()).append(';');
            } else {
                String s = (String) call(p, "getAsString");
                b.append('s').append(s.length()).append(':').append(s);
            }
        } else if ((Boolean) call(el, "isJsonArray")) {
            b.append('[');
            for (Object c : (Iterable<?>) call(el, "getAsJsonArray")) {
                canonical(c, b);
                b.append(',');
            }
            b.append(']');
        } else {
            b.append('{');
            for (Object o : (Iterable<?>) call(call(el, "getAsJsonObject"), "entrySet")) {
                Map.Entry<?, ?> entry = (Map.Entry<?, ?>) o;
                String key = (String) entry.getKey();
                b.append('s').append(key.length()).append(':').append(key).append('=');
                canonical(entry.getValue(), b);
                b.append(';');
            }
            b.append('}');
        }
    }

    private static Object call(Object target, String method) throws Exception {
        Method m = target.getClass().getMethod(method);
        m.setAccessible(true);
        return m.invoke(target);
    }

    /** Decodes a parsed number with the bundled codec library's primitive codecs. */
    static final class NumberProbe {
        static final String[] CODECS = {"BYTE", "SHORT", "INT", "LONG", "FLOAT", "DOUBLE"};
        final Object ops;
        final Object[] codecs = new Object[CODECS.length];
        final Method parse;
        final Method result;

        NumberProbe() throws Exception {
            ops = Class.forName("com.mojang.serialization.JsonOps").getField("INSTANCE").get(null);
            Class<?> codec = Class.forName("com.mojang.serialization.Codec");
            for (int i = 0; i < CODECS.length; i++) {
                codecs[i] = codec.getField(CODECS[i]).get(null);
            }
            parse = Class.forName("com.mojang.serialization.Decoder").getMethod("parse", Class.forName("com.mojang.serialization.DynamicOps"), Object.class);
            result = Class.forName("com.mojang.serialization.DataResult").getMethod("result");
        }

        void probe(Object tree, DataOutputStream out) throws Exception {
            for (int i = 0; i < CODECS.length; i++) {
                Object value = null;
                if (tree != null) {
                    try {
                        Optional<?> r = (Optional<?>) result.invoke(parse.invoke(codecs[i], ops, tree));
                        value = r.orElse(null);
                    } catch (Throwable thrown) {
                        value = null;
                    }
                }
                if (value == null) {
                    out.writeByte(1);
                    out.writeLong(0);
                } else {
                    out.writeByte(0);
                    if (value instanceof Float f) out.writeLong(Float.floatToRawIntBits(f));
                    else if (value instanceof Double d) out.writeLong(Double.doubleToRawLongBits(d));
                    else out.writeLong(((Number) value).longValue());
                }
            }
        }
    }
}
