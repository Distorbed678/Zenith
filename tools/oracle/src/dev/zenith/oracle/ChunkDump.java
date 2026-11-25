// Zenith parity oracle — original source, MIT OR Apache-2.0.
package dev.zenith.oracle;

import java.io.BufferedInputStream;
import java.io.BufferedOutputStream;
import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collection;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Full-chunk dump for the {@code golden-terrain} suite (format: docs/specs/parity/terrain-chunks.md, {@code ZTC1}).
 *
 * <p>Runs on the server thread from {@link Hooks#tickEnd}: each requested chunk is generated through the server's own
 * chunk pipeline up to the requested status, then its blocks, biomes, heightmaps, post-processing marks and tick
 * counts are written. Game classes are reached by their public names.
 */
final class ChunkDump {
    private ChunkDump() {}

    static void run(Object server, Path requests, Path out) throws Exception {
        Class<?> statusClass = Class.forName("net.minecraft.world.level.chunk.status.ChunkStatus");
        Method byName = statusClass.getMethod("byName", String.class);
        Class<?> stateClass = Class.forName("net.minecraft.world.level.block.state.BlockState");
        Method blockId = Class.forName("net.minecraft.world.level.block.Block").getMethod("getId", stateClass);

        Map<String, Object> levels = new LinkedHashMap<>();
        for (Object level : (Iterable<?>) call(server, "getAllLevels")) {
            levels.put(String.valueOf(call(call(level, "dimension"), "identifier")), level);
        }

        int count = 0;
        try (DataInputStream in = new DataInputStream(new BufferedInputStream(Files.newInputStream(requests)));
                DataOutputStream o = new DataOutputStream(new BufferedOutputStream(Files.newOutputStream(out)))) {
            int n = in.readInt();
            o.write("ZTC1".getBytes(StandardCharsets.US_ASCII));
            o.writeInt(n);
            for (int r = 0; r < n; r++) {
                String dimension = readStr(in);
                int cx = in.readInt();
                int cz = in.readInt();
                String statusName = readStr(in);
                Object level = levels.get(dimension);
                if (level == null) {
                    throw new IllegalArgumentException("unknown dimension " + dimension + " (have " + levels.keySet() + ")");
                }
                Object status = byName.invoke(null, statusName);
                Object source = call(level, "getChunkSource");
                Method getChunk = findMethod(source, "getChunk", int.class, int.class, statusClass, boolean.class);
                Object chunk = getChunk.invoke(source, cx, cz, status, true);
                if (chunk == null) {
                    throw new IllegalStateException("no chunk at " + dimension + " " + cx + "," + cz);
                }
                writeChunk(o, chunk, dimension, cx, cz, statusName, blockId);
                count++;
            }
        }
        System.out.println("[zenith-oracle] dumped " + count + " chunks to " + out);
    }

    private static void writeChunk(DataOutputStream o, Object chunk, String dimension, int cx, int cz, String requested,
            Method blockId) throws Exception {
        int minY = (Integer) call(chunk, "getMinY");
        int height = (Integer) call(chunk, "getHeight");
        Object reached = call(call(chunk, "getPersistedStatus"), "getName");
        Object[] sections = (Object[]) call(chunk, "getSections");

        writeStr(o, dimension);
        o.writeInt(cx);
        o.writeInt(cz);
        writeStr(o, requested);
        writeStr(o, String.valueOf(reached));
        o.writeInt(minY);
        o.writeInt(height);
        o.writeInt(sections.length);

        // Blocks: sections bottom-up; inside a section y, then z, then x (index = (y * 16 + z) * 16 + x).
        for (Object section : sections) {
            Method getState = findMethod(section, "getBlockState", int.class, int.class, int.class);
            for (int y = 0; y < 16; y++) {
                for (int z = 0; z < 16; z++) {
                    for (int x = 0; x < 16; x++) {
                        o.writeInt((Integer) blockId.invoke(null, getState.invoke(section, x, y, z)));
                    }
                }
            }
        }

        // Biomes: a per-chunk string table, then per section 64 indices in (qy, qz, qx) order.
        List<String> table = new ArrayList<>();
        Map<String, Integer> index = new LinkedHashMap<>();
        int[] biomeIdx = new int[sections.length * 64];
        int at = 0;
        for (Object section : sections) {
            Method getBiome = findMethod(section, "getNoiseBiome", int.class, int.class, int.class);
            for (int qy = 0; qy < 4; qy++) {
                for (int qz = 0; qz < 4; qz++) {
                    for (int qx = 0; qx < 4; qx++) {
                        String id = String.valueOf(call(getBiome.invoke(section, qx, qy, qz), "getRegisteredName"));
                        Integer i = index.get(id);
                        if (i == null) {
                            i = table.size();
                            index.put(id, i);
                            table.add(id);
                        }
                        biomeIdx[at++] = i;
                    }
                }
            }
        }
        o.writeShort(table.size());
        for (String id : table) {
            writeStr(o, id);
        }
        for (int i : biomeIdx) {
            o.writeShort(i);
        }

        // Heightmaps the chunk holds at this status: serialization name, then 256 first-available heights (z, x).
        Collection<?> heightmaps = (Collection<?>) call(chunk, "getHeightmaps");
        List<Map.Entry<?, ?>> entries = new ArrayList<>();
        for (Object e : heightmaps) {
            entries.add((Map.Entry<?, ?>) e);
        }
        entries.sort((a, b) -> String.valueOf(serialName(a.getKey())).compareTo(String.valueOf(serialName(b.getKey()))));
        o.writeByte(entries.size());
        for (Map.Entry<?, ?> e : entries) {
            writeStr(o, serialName(e.getKey()));
            Method first = findMethod(e.getValue(), "getFirstAvailable", int.class, int.class);
            for (int z = 0; z < 16; z++) {
                for (int x = 0; x < 16; x++) {
                    o.writeInt((Integer) first.invoke(e.getValue(), x, z));
                }
            }
        }

        // Post-processing marks: per section index, the packed local offsets in list order.
        Object[] post = (Object[]) call(chunk, "getPostProcessing");
        List<int[]> marks = new ArrayList<>();
        for (int s = 0; post != null && s < post.length; s++) {
            if (post[s] == null) {
                continue;
            }
            for (Object packed : (Iterable<?>) post[s]) {
                marks.add(new int[] {s, ((Short) packed) & 0xFFFF});
            }
        }
        o.writeInt(marks.size());
        for (int[] m : marks) {
            o.writeInt(m[0]);
            o.writeShort(m[1]);
        }

        // Scheduled tick counts held by the chunk (block, fluid); -1 if the container cannot report a count.
        o.writeInt(tickCount(chunk, "getBlockTicks"));
        o.writeInt(tickCount(chunk, "getFluidTicks"));
    }

    private static int tickCount(Object chunk, String getter) {
        try {
            return (Integer) call(call(chunk, getter), "count");
        } catch (Throwable notCountable) {
            return -1;
        }
    }

    private static String serialName(Object heightmapType) {
        try {
            return String.valueOf(call(heightmapType, "getSerializationKey"));
        } catch (Exception e) {
            return String.valueOf(heightmapType);
        }
    }

    private static Object call(Object target, String name) throws Exception {
        return findMethod(target, name).invoke(target);
    }

    private static Method findMethod(Object target, String name, Class<?>... params) throws NoSuchMethodException {
        Method m = target.getClass().getMethod(name, params);
        m.setAccessible(true);
        return m;
    }

    private static String readStr(DataInputStream in) throws Exception {
        int len = in.readUnsignedShort();
        return new String(in.readNBytes(len), StandardCharsets.UTF_8);
    }

    private static void writeStr(DataOutputStream o, String s) throws Exception {
        byte[] b = s.getBytes(StandardCharsets.UTF_8);
        o.writeShort(b.length);
        o.write(b);
    }
}
