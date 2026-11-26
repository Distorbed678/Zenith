// Zenith parity oracle — original source, MIT OR Apache-2.0.
package dev.zenith.oracle;

import java.io.ByteArrayOutputStream;
import java.io.DataOutput;
import java.io.DataOutputStream;
import java.io.IOException;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;
import java.util.function.Consumer;
import java.util.stream.Stream;

/** Writes one {@code ZSO1} dump record (format: docs/specs/parity/scenarios.md). */
final class ScenarioDump {
    private ScenarioDump() {}

    /** One scheduled tick as read from a chunk container. */
    private record TickRow(long trigger, int priority, long sub, int x, int y, int z, int typeId, String typeName) {}

    static void write(DataOutputStream o, Object server, Object level, int timelineTick, int phase, int[] box,
            int sections) throws Exception {
        int minX = Math.min(box[0], box[3]);
        int minY = Math.min(box[1], box[4]);
        int minZ = Math.min(box[2], box[5]);
        int maxX = Math.max(box[0], box[3]);
        int maxY = Math.max(box[1], box[4]);
        int maxZ = Math.max(box[2], box[5]);

        o.writeByte(ScenarioRunner.TAG_DUMP);
        o.writeInt(timelineTick);
        o.writeByte(phase);
        o.writeLong(ScenarioGame.gameTime(level));

        // Drift detectors: the level random, its cached Gaussian, the random-tick LCG, the sub-tick counter and the
        // number of scheduled ticks the whole level holds.
        Object random = ScenarioGame.levelRandom(level);
        o.writeLong(ScenarioGame.randomState(random));
        Object[] gauss = ScenarioGame.gaussianCache(random);
        o.writeByte((Boolean) gauss[0] ? 1 : 0);
        o.writeDouble((Double) gauss[1]);
        o.writeInt(ScenarioGame.randValue(level));
        o.writeLong(ScenarioGame.subTickCount(level));
        o.writeInt((Integer) ScenarioGame.call(ScenarioGame.call(level, "getBlockTicks"), "count"));
        o.writeInt((Integer) ScenarioGame.call(ScenarioGame.call(level, "getFluidTicks"), "count"));

        // Chunks in the order the level visits them for random ticks (the entity-ticking chunks).
        List<int[]> order = tickingChunks(level);
        o.writeInt(order.size());
        for (int[] c : order) {
            o.writeInt(c[0]);
            o.writeInt(c[1]);
        }

        for (int v : new int[] {minX, minY, minZ, maxX, maxY, maxZ}) {
            o.writeInt(v);
        }
        o.writeByte(sections);

        if ((sections & Scenario.SECTION_BLOCKS) != 0) {
            long volume = (long) (maxX - minX + 1) * (maxY - minY + 1) * (maxZ - minZ + 1);
            if (volume > 8_000_000L) {
                throw new IllegalArgumentException("dump box too large: " + volume + " blocks");
            }
            Method getState = ScenarioGame.method(level.getClass(), "getBlockState", ScenarioGame.BLOCK_POS);
            o.writeInt((int) volume);
            for (int y = minY; y <= maxY; y++) {
                for (int z = minZ; z <= maxZ; z++) {
                    for (int x = minX; x <= maxX; x++) {
                        o.writeInt(ScenarioGame.stateId(getState.invoke(level, ScenarioGame.pos(x, y, z))));
                    }
                }
            }
        }
        if ((sections & Scenario.SECTION_TICKS) != 0) {
            writeTicks(o, level, "getBlockTicks", ScenarioGame.blockRegistry(), minX, minY, minZ, maxX, maxY, maxZ);
            writeTicks(o, level, "getFluidTicks", ScenarioGame.fluidRegistry(), minX, minY, minZ, maxX, maxY, maxZ);
        }
        if ((sections & Scenario.SECTION_EVENTS) != 0) {
            writeEvents(o, level, minX, minY, minZ, maxX, maxY, maxZ);
        }
        if ((sections & Scenario.SECTION_BLOCK_ENTITIES) != 0) {
            writeBlockEntities(o, level, minX, minY, minZ, maxX, maxY, maxZ);
        }
    }

    /** Chunk coordinates in the order {@code forEachBlockTickingChunk} reports them (the random-tick order). */
    static List<int[]> tickingChunks(Object level) throws Exception {
        Object source = ScenarioGame.call(level, "getChunkSource");
        Object chunkMap = source.getClass().getField("chunkMap").get(source);
        List<int[]> out = new ArrayList<>();
        Consumer<Object> sink = chunk -> {
            try {
                Object pos = ScenarioGame.call(chunk, "getPos");
                out.add(new int[] {(Integer) ScenarioGame.call(pos, "x"), (Integer) ScenarioGame.call(pos, "z")});
            } catch (Exception e) {
                throw new IllegalStateException(e);
            }
        };
        ScenarioGame.method(chunkMap.getClass(), "forEachBlockTickingChunk", Consumer.class).invoke(chunkMap, sink);
        return out;
    }

    private static Object loadedChunk(Object level, int cx, int cz) throws Exception {
        Object source = ScenarioGame.call(level, "getChunkSource");
        Object chunk = ScenarioGame.call(source, "getChunkNow", new Class<?>[] {int.class, int.class}, cx, cz);
        if (chunk == null) {
            throw new IllegalStateException("dump box touches chunk " + cx + "," + cz + " which is not loaded; "
                    + "keep the box inside the forced chunk box");
        }
        return chunk;
    }

    private static void writeTicks(DataOutputStream o, Object level, String getter, Object registry, int minX, int minY,
            int minZ, int maxX, int maxY, int maxZ) throws Exception {
        List<TickRow> rows = new ArrayList<>();
        for (int cx = minX >> 4; cx <= maxX >> 4; cx++) {
            for (int cz = minZ >> 4; cz <= maxZ >> 4; cz++) {
                Object container = ScenarioGame.call(loadedChunk(level, cx, cz), getter);
                try (Stream<?> all = (Stream<?>) ScenarioGame.call(container, "getAll")) {
                    for (Object tick : all.toList()) {
                        Object pos = ScenarioGame.call(tick, "pos");
                        int x = (Integer) ScenarioGame.call(pos, "getX");
                        int y = (Integer) ScenarioGame.call(pos, "getY");
                        int z = (Integer) ScenarioGame.call(pos, "getZ");
                        if (x < minX || x > maxX || y < minY || y > maxY || z < minZ || z > maxZ) {
                            continue;
                        }
                        Object type = ScenarioGame.call(tick, "type");
                        rows.add(new TickRow((Long) ScenarioGame.call(tick, "triggerTick"),
                                (Integer) ScenarioGame.call(ScenarioGame.call(tick, "priority"), "getValue"),
                                (Long) ScenarioGame.call(tick, "subTickOrder"), x, y, z,
                                ScenarioGame.idOf(registry, type), ScenarioGame.nameOf(registry, type)));
                    }
                }
            }
        }
        // Total order: trigger time, priority, then the level-unique sub-tick number.
        rows.sort(Comparator.comparingLong(TickRow::trigger).thenComparingInt(TickRow::priority)
                .thenComparingLong(TickRow::sub));
        o.writeInt(rows.size());
        for (TickRow r : rows) {
            o.writeInt(r.x());
            o.writeInt(r.y());
            o.writeInt(r.z());
            o.writeInt(r.typeId());
            writeStr(o, r.typeName());
            o.writeLong(r.trigger());
            o.writeInt(r.priority());
            o.writeLong(r.sub());
        }
    }

    private static void writeEvents(DataOutputStream o, Object level, int minX, int minY, int minZ, int maxX, int maxY,
            int maxZ) throws Exception {
        Iterable<?> queue = (Iterable<?>) ScenarioGame.field(level.getClass(), "blockEvents").get(level);
        ByteArrayOutputStream buf = new ByteArrayOutputStream();
        DataOutputStream b = new DataOutputStream(buf);
        int n = 0;
        for (Object ev : queue) {
            Object pos = ScenarioGame.call(ev, "pos");
            int x = (Integer) ScenarioGame.call(pos, "getX");
            int y = (Integer) ScenarioGame.call(pos, "getY");
            int z = (Integer) ScenarioGame.call(pos, "getZ");
            if (x < minX || x > maxX || y < minY || y > maxY || z < minZ || z > maxZ) {
                continue;
            }
            Object block = ScenarioGame.call(ev, "block");
            b.writeInt(x);
            b.writeInt(y);
            b.writeInt(z);
            b.writeInt(ScenarioGame.idOf(ScenarioGame.blockRegistry(), block));
            writeStr(b, ScenarioGame.nameOf(ScenarioGame.blockRegistry(), block));
            b.writeInt((Integer) ScenarioGame.call(ev, "paramA"));
            b.writeInt((Integer) ScenarioGame.call(ev, "paramB"));
            n++;
        }
        o.writeInt(n);
        buf.writeTo(o);
    }

    private static void writeBlockEntities(DataOutputStream o, Object level, int minX, int minY, int minZ, int maxX,
            int maxY, int maxZ) throws Exception {
        Class<?> tagClass = ScenarioGame.cls("net.minecraft.nbt.Tag");
        Method writeAnyTag = ScenarioGame.cls("net.minecraft.nbt.NbtIo").getMethod("writeAnyTag", tagClass, DataOutput.class);
        Object registries = ScenarioGame.call(level, "registryAccess");
        List<int[]> positions = new ArrayList<>();
        List<Object> chunks = new ArrayList<>();
        for (int cx = minX >> 4; cx <= maxX >> 4; cx++) {
            for (int cz = minZ >> 4; cz <= maxZ >> 4; cz++) {
                Object chunk = loadedChunk(level, cx, cz);
                chunks.add(chunk);
                for (Object p : (Iterable<?>) ScenarioGame.call(chunk, "getBlockEntitiesPos")) {
                    int x = (Integer) ScenarioGame.call(p, "getX");
                    int y = (Integer) ScenarioGame.call(p, "getY");
                    int z = (Integer) ScenarioGame.call(p, "getZ");
                    if (x >= minX && x <= maxX && y >= minY && y <= maxY && z >= minZ && z <= maxZ) {
                        positions.add(new int[] {x, y, z, chunks.size() - 1});
                    }
                }
            }
        }
        positions.sort(Comparator.<int[]>comparingInt(p -> p[1]).thenComparingInt(p -> p[2]).thenComparingInt(p -> p[0]));
        o.writeInt(positions.size());
        for (int[] p : positions) {
            Object chunk = chunks.get(p[3]);
            o.writeInt(p[0]);
            o.writeInt(p[1]);
            o.writeInt(p[2]);
            Method save = null;
            for (Method m : chunk.getClass().getMethods()) {
                if (m.getName().equals("getBlockEntityNbtForSaving") && m.getParameterCount() == 2) {
                    save = m;
                }
            }
            Object nbt = save == null ? null : save.invoke(chunk, ScenarioGame.pos(p[0], p[1], p[2]), registries);
            if (nbt == null) {
                o.writeInt(-1);
            } else {
                ByteArrayOutputStream bytes = new ByteArrayOutputStream();
                writeAnyTag.invoke(null, nbt, new DataOutputStream(bytes));
                o.writeInt(bytes.size());
                bytes.writeTo(o);
            }
        }
    }

    static void writeStr(DataOutputStream o, String s) throws IOException {
        byte[] b = s.getBytes(StandardCharsets.UTF_8);
        o.writeShort(b.length);
        o.write(b);
    }
}
