// Zenith parity oracle — original source, MIT OR Apache-2.0.
package dev.zenith.oracle;

import java.io.BufferedInputStream;
import java.io.BufferedOutputStream;
import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.io.EOFException;
import java.io.InputStreamReader;
import java.io.Reader;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Optional;

/**
 * Reference for the {@code text-component} suite.
 *
 * <p>Input: repeated {@code [u32 length][UTF-8 JSON text]}. Each text is parsed with the game's strict data-file
 * parser and decoded with its text-component codec, with full registry access. Output per input: a status byte (0 decoded, 1 codec rejected,
 * 2 JSON rejected), then for decoded inputs, in order:
 * <ol>
 *   <li>the component re-encoded to JSON, as {@link JsonCheck#canonical} UTF-16 units;</li>
 *   <li>the component encoded to NBT, as unnamed network bytes (type byte + payload);</li>
 *   <li>that NBT decoded again and re-encoded to JSON (canonical units);</li>
 *   <li>the plain string, then the plain string truncated to {@link #LIMIT} units.</li>
 * </ol>
 * Each of 1–3 starts with a flag byte (0 ok, 1 failed); units are written as {@code [u32 count][u16...]}, bytes as
 * {@code [u32 length][bytes]}. Game classes are reached by their public names.
 */
public final class TextCheck {
    static final int LIMIT = 8;

    private TextCheck() {}

    public static void main(String[] args) throws Exception {
        Class.forName("net.minecraft.SharedConstants").getMethod("tryDetectVersion").invoke(null);
        Class.forName("net.minecraft.server.Bootstrap").getMethod("bootStrap").invoke(null);

        Method parseJson = Class.forName("net.minecraft.util.StrictJsonParser").getMethod("parse", Reader.class);
        Object codec = Class.forName("net.minecraft.network.chat.ComponentSerialization").getField("CODEC").get(null);
        Class<?> dynamicOps = Class.forName("com.mojang.serialization.DynamicOps");
        // Full registry access, like every real decode path in the game: the built-in static registries plus the
        // version's built-in world registries (dialogs, enchantments, ...).
        Object lookup = Class.forName("net.minecraft.data.registries.VanillaRegistries").getMethod("createWorldLookup").invoke(null);
        Method withRegistries = Class.forName("net.minecraft.core.HolderLookup$Provider").getMethod("createSerializationContext", dynamicOps);
        Object jsonOps = withRegistries.invoke(lookup, Class.forName("com.mojang.serialization.JsonOps").getField("INSTANCE").get(null));
        Object nbtOps = withRegistries.invoke(lookup, Class.forName("net.minecraft.nbt.NbtOps").getField("INSTANCE").get(null));
        Method decode = Class.forName("com.mojang.serialization.Decoder").getMethod("parse", dynamicOps, Object.class);
        Method encode = Class.forName("com.mojang.serialization.Encoder").getMethod("encodeStart", dynamicOps, Object.class);
        Method result = Class.forName("com.mojang.serialization.DataResult").getMethod("result");
        Class<?> tagClass = Class.forName("net.minecraft.nbt.Tag");
        Method writeAnyTag = Class.forName("net.minecraft.nbt.NbtIo").getMethod("writeAnyTag", tagClass, java.io.DataOutput.class);
        Class<?> componentClass = Class.forName("net.minecraft.network.chat.Component");
        Method getString = componentClass.getMethod("getString");
        Method getStringLimited = componentClass.getMethod("getString", int.class);

        int items = 0;
        try (DataInputStream in = new DataInputStream(new BufferedInputStream(Files.newInputStream(Path.of(args[0]))));
                DataOutputStream out = new DataOutputStream(new BufferedOutputStream(Files.newOutputStream(Path.of(args[1]))))) {
            while (true) {
                int len;
                try {
                    len = in.readInt();
                } catch (EOFException end) {
                    break;
                }
                byte[] input = in.readNBytes(len);
                items++;
                Object tree;
                try {
                    tree = parseJson.invoke(null, new InputStreamReader(new ByteArrayInputStream(input), StandardCharsets.UTF_8));
                } catch (Throwable rejected) {
                    out.writeByte(2);
                    continue;
                }
                Object component = unwrap(result, decode, codec, jsonOps, tree);
                if (component == null) {
                    out.writeByte(1);
                    continue;
                }
                out.writeByte(0);

                writeJson(out, unwrap(result, encode, codec, jsonOps, component));

                Object tag = unwrap(result, encode, codec, nbtOps, component);
                if (tag == null) {
                    out.writeByte(1);
                } else {
                    ByteArrayOutputStream bytes = new ByteArrayOutputStream();
                    writeAnyTag.invoke(null, tag, new DataOutputStream(bytes));
                    out.writeByte(0);
                    out.writeInt(bytes.size());
                    bytes.writeTo(out);
                }

                Object fromNbt = tag == null ? null : unwrap(result, decode, codec, nbtOps, tag);
                writeJson(out, fromNbt == null ? null : unwrap(result, encode, codec, jsonOps, fromNbt));

                writeUnits(out, (String) getString.invoke(component));
                writeUnits(out, (String) getStringLimited.invoke(component, LIMIT));
            }
        }
        System.out.println("[zenith-oracle] text components: " + items + " inputs");
    }

    /** Runs a decode or encode and returns its value, or null if it failed or threw. */
    private static Object unwrap(Method result, Method op, Object codec, Object ops, Object value) {
        if (value == null) {
            return null;
        }
        try {
            return ((Optional<?>) result.invoke(op.invoke(codec, ops, value))).orElse(null);
        } catch (Throwable failed) {
            return null;
        }
    }

    private static void writeJson(DataOutputStream out, Object json) throws Exception {
        if (json == null) {
            out.writeByte(1);
            return;
        }
        StringBuilder dump = new StringBuilder();
        JsonCheck.canonical(json, dump);
        out.writeByte(0);
        writeUnits(out, dump);
    }

    private static void writeUnits(DataOutputStream out, CharSequence s) throws Exception {
        out.writeInt(s.length());
        for (int i = 0; i < s.length(); i++) {
            out.writeChar(s.charAt(i));
        }
    }
}
