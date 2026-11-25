// Zenith parity oracle — original source, MIT OR Apache-2.0.
package dev.zenith.oracle;

import java.io.BufferedInputStream;
import java.io.BufferedOutputStream;
import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.DataInput;
import java.io.DataInputStream;
import java.io.DataOutput;
import java.io.DataOutputStream;
import java.io.EOFException;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;

/**
 * Reference for the {@code nbt-roundtrip} suite: reads each corpus item with the game's own NBT reader and writes it
 * back with the game's own writer, so Zenith can be compared with what vanilla itself produces.
 *
 * <p>Modes: {@code resave} (default) writes the bytes vanilla re-serializes; {@code snbt} writes the UTF-16BE text vanilla
 * prints for the value ({@code toString()}, the single-line SNBT form).
 *
 * <p>Input: repeated {@code [u32 length][named-root NBT bytes]}. Output: repeated
 * {@code [u8 status][u32 length][bytes]}, where status 0 carries the re-serialized bytes and status 1 carries the
 * UTF-8 error text from vanilla's reader.
 *
 * <p>Game classes are reached reflectively by their public names (facts listed with javap), so this compiles
 * without the game on the classpath.
 */
public final class NbtRoundtrip {
    private NbtRoundtrip() {}

    public static void main(String[] args) throws Exception {
        if (args.length < 2 || args.length > 3) {
            throw new IllegalArgumentException("usage: NbtRoundtrip <corpus.bin> <results.bin> [resave|snbt]");
        }
        boolean snbt = args.length == 3 && args[2].equals("snbt");
        Class<?> io = Class.forName("net.minecraft.nbt.NbtIo");
        Class<?> accounter = Class.forName("net.minecraft.nbt.NbtAccounter");
        Class<?> tag = Class.forName("net.minecraft.nbt.Tag");
        Method unlimited = accounter.getMethod("unlimitedHeap");
        Method read = io.getMethod("readUnnamedTag", DataInput.class, accounter);
        Method write = io.getMethod("writeUnnamedTag", tag, DataOutput.class);

        int items = 0;
        int rejected = 0;
        try (DataInputStream corpus = new DataInputStream(new BufferedInputStream(Files.newInputStream(Path.of(args[0]))));
                DataOutputStream results = new DataOutputStream(new BufferedOutputStream(Files.newOutputStream(Path.of(args[1]))))) {
            while (true) {
                int length;
                try {
                    length = corpus.readInt();
                } catch (EOFException end) {
                    break;
                }
                byte[] payload = corpus.readNBytes(length);
                try {
                    Object value = read.invoke(null, new DataInputStream(new ByteArrayInputStream(payload)), unlimited.invoke(null));
                    // Serialize fully before writing the status byte, so a failure can't leave a half-written record.
                    byte[] produced;
                    if (snbt) {
                        // Raw UTF-16 code units, big-endian: lossless, unpaired surrogates included (charset encoders
                        // would replace them).
                        String text = String.valueOf(value);
                        produced = new byte[text.length() * 2];
                        for (int k = 0; k < text.length(); k++) {
                            produced[2 * k] = (byte) (text.charAt(k) >> 8);
                            produced[2 * k + 1] = (byte) text.charAt(k);
                        }
                    } else {
                        ByteArrayOutputStream written = new ByteArrayOutputStream(length);
                        write.invoke(null, value, new DataOutputStream(written));
                        produced = written.toByteArray();
                    }
                    results.writeByte(0);
                    results.writeInt(produced.length);
                    results.write(produced);
                } catch (InvocationTargetException failure) {
                    byte[] message = String.valueOf(failure.getCause()).getBytes(StandardCharsets.UTF_8);
                    results.writeByte(1);
                    results.writeInt(message.length);
                    results.write(message);
                    rejected++;
                }
                items++;
            }
        }
        System.out.println("[zenith-oracle] nbt " + (snbt ? "snbt" : "re-save") + ": " + items + " items, " + rejected + " rejected by vanilla");
    }
}
