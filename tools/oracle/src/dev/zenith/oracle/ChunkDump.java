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

    /** Optional per-chunk sections (`chunks_extra` bitmask); with any bit set the file starts with {@code ZTC2}. */
    static final int EXTRA_LIGHT = 1;
    static final int EXTRA_BLOCK_ENTITIES = 2;
    static final int EXTRA_ENTITIES = 4;

    static void run(Object server, Path requests, Path out, int extras) throws Exception {
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
            o.write((extras == 0 ? "ZTC1" : "ZTC2").getBytes(StandardCharsets.US_ASCII));
            o.writeInt(n);
            if (extras != 0) {
                o.writeByte(extras);
            }
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
                writeChunk(o, chunk, dimension, cx, cz, statusName, blockId, structureIds(level));
                writeExtras(o, extras, level, chunk, cx, cz);
                count++;
            }
        }
        System.out.println("[zenith-oracle] dumped " + count + " chunks to " + out);
    }

    /** Registry-id lookups for structures and structure piece types. */
    private record Ids(Object structures, Object pieceTypes, Method structureKey, Method pieceKey) {}

    private static Ids structureIds(Object level) throws Exception {
        Object access = call(level, "registryAccess");
        Class<?> keyClass = Class.forName("net.minecraft.resources.ResourceKey");
        Object structureKey = Class.forName("net.minecraft.core.registries.Registries").getField("STRUCTURE").get(null);
        Object structures = findMethod(access, "lookupOrThrow", keyClass).invoke(access, structureKey);
        Object pieces = Class.forName("net.minecraft.core.registries.BuiltInRegistries").getField("STRUCTURE_PIECE").get(null);
        return new Ids(structures, pieces, findMethod(structures, "getKey", Object.class), findMethod(pieces, "getKey", Object.class));
    }

    private static void writeChunk(DataOutputStream o, Object chunk, String dimension, int cx, int cz, String requested,
            Method blockId, Ids ids) throws Exception {
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

        // Structure starts held by this chunk, sorted by structure id: id, start chunk, references, validity, bounding
        // box, then every piece (type id, box, orientation name or "", generation depth) in list order.
        Map<String, Object> starts = new java.util.TreeMap<>();
        for (Map.Entry<?, ?> e : ((Map<?, ?>) call(chunk, "getAllStarts")).entrySet()) {
            starts.put(String.valueOf(ids.structureKey().invoke(ids.structures(), e.getKey())), e.getValue());
        }
        o.writeInt(starts.size());
        for (Map.Entry<String, Object> e : starts.entrySet()) {
            Object start = e.getValue();
            writeStr(o, e.getKey());
            Object pos = call(start, "getChunkPos");
            o.writeInt((Integer) call(pos, "x"));
            o.writeInt((Integer) call(pos, "z"));
            o.writeInt((Integer) call(start, "getReferences"));
            o.writeByte((Boolean) call(start, "isValid") ? 1 : 0);
            writeBox(o, call(start, "getBoundingBox"));
            List<?> pieceList = (List<?>) call(start, "getPieces");
            o.writeInt(pieceList.size());
            for (Object piece : pieceList) {
                writeStr(o, String.valueOf(ids.pieceKey().invoke(ids.pieceTypes(), call(piece, "getType"))));
                writeBox(o, call(piece, "getBoundingBox"));
                Object facing = call(piece, "getOrientation");
                writeStr(o, facing == null ? "" : String.valueOf(call(facing, "getSerializedName")));
                o.writeInt((Integer) call(piece, "getGenDepth"));
            }
        }

        // Structure references, sorted by structure id, each with its packed start-chunk positions sorted.
        Map<String, long[]> refs = new java.util.TreeMap<>();
        for (Map.Entry<?, ?> e : ((Map<?, ?>) call(chunk, "getAllReferences")).entrySet()) {
            Object set = e.getValue();
            long[] packed = (long[]) findMethod(set, "toLongArray").invoke(set);
            java.util.Arrays.sort(packed);
            refs.put(String.valueOf(ids.structureKey().invoke(ids.structures(), e.getKey())), packed);
        }
        o.writeInt(refs.size());
        for (Map.Entry<String, long[]> e : refs.entrySet()) {
            writeStr(o, e.getKey());
            o.writeInt(e.getValue().length);
            for (long l : e.getValue()) {
                o.writeLong(l);
            }
        }
    }

    private static void writeExtras(DataOutputStream o, int extras, Object level, Object chunk, int cx, int cz)
            throws Exception {
        if ((extras & EXTRA_LIGHT) != 0) {
            // Light layers per light section (one more than the chunk's sections above and below): block then sky,
            // each a presence byte and, if present, the 2048 raw nibble bytes.
            Object engine = call(level, "getLightEngine");
            int min = (Integer) call(engine, "getMinLightSection");
            int max = (Integer) call(engine, "getMaxLightSection");
            Class<?> layerClass = Class.forName("net.minecraft.world.level.LightLayer");
            Object blockLayer = Enum.valueOf(layerClass.asSubclass(Enum.class), "BLOCK");
            Object skyLayer = Enum.valueOf(layerClass.asSubclass(Enum.class), "SKY");
            Method listener = findMethod(engine, "getLayerListener", layerClass);
            Object blockListener = listener.invoke(engine, blockLayer);
            Object skyListener = listener.invoke(engine, skyLayer);
            Class<?> sectionPos = Class.forName("net.minecraft.core.SectionPos");
            Method of = sectionPos.getMethod("of", int.class, int.class, int.class);
            o.writeInt(max - min);
            for (int sy = min; sy < max; sy++) {
                Object pos = of.invoke(null, cx, sy, cz);
                o.writeInt(sy);
                for (Object l : new Object[] {blockListener, skyListener}) {
                    Object layer = findMethod(l, "getDataLayerData", sectionPos).invoke(l, pos);
                    if (layer == null) {
                        o.writeByte(0);
                    } else {
                        o.writeByte(1);
                        o.write((byte[]) call(layer, "getData"));
                    }
                }
            }
        }
        Class<?> tagClass = Class.forName("net.minecraft.nbt.Tag");
        Method writeAnyTag = Class.forName("net.minecraft.nbt.NbtIo").getMethod("writeAnyTag", tagClass, java.io.DataOutput.class);
        if ((extras & EXTRA_BLOCK_ENTITIES) != 0) {
            // Block entities sorted by (y, z, x): position then the NBT the chunk would save (network NBT bytes).
            Object registries = call(level, "registryAccess");
            List<Object> positions = new ArrayList<>((java.util.Set<?>) call(chunk, "getBlockEntitiesPos"));
            positions.sort((p, q) -> {
                try {
                    int c = Integer.compare((Integer) call(p, "getY"), (Integer) call(q, "getY"));
                    if (c == 0) c = Integer.compare((Integer) call(p, "getZ"), (Integer) call(q, "getZ"));
                    if (c == 0) c = Integer.compare((Integer) call(p, "getX"), (Integer) call(q, "getX"));
                    return c;
                } catch (Exception e) {
                    throw new IllegalStateException(e);
                }
            });
            Method save = null;
            for (Method m : chunk.getClass().getMethods()) {
                if (m.getName().equals("getBlockEntityNbtForSaving") && m.getParameterCount() == 2) {
                    save = m;
                }
            }
            o.writeInt(positions.size());
            for (Object p : positions) {
                o.writeInt((Integer) call(p, "getX"));
                o.writeInt((Integer) call(p, "getY"));
                o.writeInt((Integer) call(p, "getZ"));
                Object nbt = save == null ? null : save.invoke(chunk, p, registries);
                writeNbt(o, writeAnyTag, nbt);
            }
        }
        if ((extras & EXTRA_ENTITIES) != 0) {
            // Entities a proto chunk holds (saved NBT in list order); -1 when the chunk keeps none (full chunks).
            Method entities = null;
            for (Method m : chunk.getClass().getMethods()) {
                if (m.getName().equals("getEntities") && m.getParameterCount() == 0) {
                    entities = m;
                }
            }
            if (entities == null) {
                o.writeInt(-1);
            } else {
                List<?> list = (List<?>) entities.invoke(chunk);
                o.writeInt(list.size());
                for (Object nbt : list) {
                    writeNbt(o, writeAnyTag, nbt);
                }
            }
        }
    }

    private static void writeNbt(DataOutputStream o, Method writeAnyTag, Object nbt) throws Exception {
        if (nbt == null) {
            o.writeInt(-1);
            return;
        }
        java.io.ByteArrayOutputStream bytes = new java.io.ByteArrayOutputStream();
        writeAnyTag.invoke(null, nbt, new DataOutputStream(bytes));
        o.writeInt(bytes.size());
        bytes.writeTo(o);
    }

    private static void writeBox(DataOutputStream o, Object box) throws Exception {
        for (String m : new String[] {"minX", "minY", "minZ", "maxX", "maxY", "maxZ"}) {
            o.writeInt((Integer) call(box, m));
        }
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
