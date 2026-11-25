// Zenith parity oracle — original source, MIT OR Apache-2.0.
package dev.zenith.oracle;

import java.io.BufferedOutputStream;
import java.io.DataInput;
import java.io.DataInputStream;
import java.io.DataOutput;
import java.io.DataOutputStream;
import java.lang.reflect.Constructor;
import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;

/**
 * Reference for the {@code anvil-io} suite. For each line of the pairs file
 * ({@code original \t zenithCopy \t vanillaCopy \t regionX \t regionZ}), every chunk slot is read with the game's own
 * region code from the original and from the copy Zenith wrote, compared with the game's own equality, and written
 * into a fresh region file by the game in the same slot order (so Zenith can compare sector layouts).
 *
 * <p>Output: one byte per slot (1024 per region): bit 0 original present, bit 1 Zenith copy present, bit 2 equal
 * (two absent chunks count as equal). All paths are copies made by the parity runner, never the user's worlds.
 */
public final class RegionCheck {
    private RegionCheck() {}

    public static void main(String[] args) throws Exception {
        Class.forName("net.minecraft.SharedConstants").getMethod("tryDetectVersion").invoke(null);
        Class.forName("net.minecraft.server.Bootstrap").getMethod("bootStrap").invoke(null);

        Object overworld = Class.forName("net.minecraft.world.level.Level").getField("OVERWORLD").get(null);
        Class<?> infoClass = Class.forName("net.minecraft.world.level.chunk.storage.RegionStorageInfo");
        Object info = infoClass.getConstructor(String.class, Class.forName("net.minecraft.resources.ResourceKey"), String.class)
                .newInstance("zenith-parity", overworld, "chunk");
        Class<?> regionClass = Class.forName("net.minecraft.world.level.chunk.storage.RegionFile");
        Constructor<?> openRegion = regionClass.getConstructor(infoClass, Path.class, Path.class, boolean.class);
        Class<?> chunkPos = Class.forName("net.minecraft.world.level.ChunkPos");
        Constructor<?> newPos = chunkPos.getConstructor(int.class, int.class);
        Method in = regionClass.getMethod("getChunkDataInputStream", chunkPos);
        Method out = regionClass.getMethod("getChunkDataOutputStream", chunkPos);
        Class<?> io = Class.forName("net.minecraft.nbt.NbtIo");
        Method read = io.getMethod("read", DataInput.class);
        Method write = io.getMethod("write", Class.forName("net.minecraft.nbt.CompoundTag"), DataOutput.class);

        List<String> lines = Files.readAllLines(Path.of(args[0]));
        int slots = 0;
        try (DataOutputStream flags = new DataOutputStream(new BufferedOutputStream(Files.newOutputStream(Path.of(args[1]))))) {
            for (String line : lines) {
                if (line.isBlank()) {
                    continue;
                }
                String[] f = line.split("\t");
                Path original = Path.of(f[0]);
                Path zenith = Path.of(f[1]);
                Path vanilla = Path.of(f[2]);
                int rx = Integer.parseInt(f[3]);
                int rz = Integer.parseInt(f[4]);
                try (AutoCloseable a = (AutoCloseable) openRegion.newInstance(info, original, original.getParent(), false);
                        AutoCloseable b = (AutoCloseable) openRegion.newInstance(info, zenith, zenith.getParent(), false);
                        AutoCloseable c = (AutoCloseable) openRegion.newInstance(info, vanilla, vanilla.getParent(), false)) {
                    for (int i = 0; i < 1024; i++) {
                        Object pos = newPos.newInstance(rx * 32 + (i & 31), rz * 32 + (i >> 5));
                        Object tagA = readTag(in, read, a, pos);
                        Object tagB = readTag(in, read, b, pos);
                        int bits = (tagA != null ? 1 : 0) | (tagB != null ? 2 : 0);
                        if ((tagA == null && tagB == null) || (tagA != null && tagA.equals(tagB))) {
                            bits |= 4;
                        }
                        if (tagA != null) {
                            try (DataOutputStream o = (DataOutputStream) out.invoke(c, pos)) {
                                write.invoke(null, tagA, o);
                            }
                        }
                        flags.writeByte(bits);
                        slots++;
                    }
                }
            }
        }
        System.out.println("[zenith-oracle] region check: " + slots + " slots");
    }

    private static Object readTag(Method in, Method read, Object region, Object pos) throws Exception {
        try (DataInputStream s = (DataInputStream) in.invoke(region, pos)) {
            return s == null ? null : read.invoke(null, s);
        }
    }
}
