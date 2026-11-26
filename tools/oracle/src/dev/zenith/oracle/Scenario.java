// Zenith parity oracle — original source, MIT OR Apache-2.0.
package dev.zenith.oracle;

import java.io.ByteArrayInputStream;
import java.io.DataInputStream;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;

/**
 * A parsed scenario file ({@code ZSC1}, format: docs/specs/parity/scenarios.md). Everything is big-endian.
 */
final class Scenario {
    static final int OP_SET_BLOCK = 1;
    static final int OP_FILL = 2;
    static final int OP_SCHEDULE_BLOCK = 3;
    static final int OP_SCHEDULE_FLUID = 4;
    static final int OP_BLOCK_EVENT = 5;
    static final int OP_PIN_RNG = 6;
    static final int OP_DUMP = 7;
    static final int OP_GAMERULE = 8;

    static final int FLAG_LIGHT_SYNC = 1;
    static final int FLAG_FAST_TICKS = 2;

    static final int SECTION_BLOCKS = 1;
    static final int SECTION_TICKS = 2;
    static final int SECTION_EVENTS = 4;
    static final int SECTION_BLOCK_ENTITIES = 8;

    /** One timeline operation. {@code ints} / {@code value} / {@code s1} / {@code s2} depend on {@code code}. */
    static final class Op {
        int tick;
        int phase;
        int code;
        int[] ints = new int[0];
        long value;
        String s1 = "";
        String s2 = "";
    }

    String dimension;
    int chunkMinX;
    int chunkMinZ;
    int chunkMaxX;
    int chunkMaxZ;
    int flags;
    final List<String[]> gamerules = new ArrayList<>();
    /** 0 = leave the level random alone, 1 = {@code setSeed(value)}, 2 = raw 48-bit internal state = value. */
    int rngMode;
    long rngValue;
    boolean hasRandValue;
    int randValue;
    int totalTicks;
    final List<Op> ops = new ArrayList<>();

    static Scenario read(Path file) throws IOException {
        DataInputStream in = new DataInputStream(new ByteArrayInputStream(Files.readAllBytes(file)));
        byte[] magic = in.readNBytes(4);
        if (!new String(magic, StandardCharsets.US_ASCII).equals("ZSC1")) {
            throw new IOException("not a ZSC1 scenario file");
        }
        Scenario s = new Scenario();
        s.dimension = str(in);
        s.chunkMinX = in.readInt();
        s.chunkMinZ = in.readInt();
        s.chunkMaxX = in.readInt();
        s.chunkMaxZ = in.readInt();
        s.flags = in.readUnsignedByte();
        int rules = in.readUnsignedShort();
        for (int i = 0; i < rules; i++) {
            s.gamerules.add(new String[] {str(in), str(in)});
        }
        s.rngMode = in.readUnsignedByte();
        s.rngValue = in.readLong();
        s.hasRandValue = in.readUnsignedByte() != 0;
        s.randValue = in.readInt();
        s.totalTicks = in.readInt();
        int n = in.readInt();
        for (int i = 0; i < n; i++) {
            Op op = new Op();
            op.tick = in.readInt();
            op.phase = in.readUnsignedByte();
            op.code = in.readUnsignedByte();
            switch (op.code) {
                case OP_SET_BLOCK -> op.ints = ints(in, 5);
                case OP_FILL -> op.ints = ints(in, 8);
                case OP_SCHEDULE_BLOCK, OP_SCHEDULE_FLUID, OP_BLOCK_EVENT -> op.ints = ints(in, 6);
                case OP_PIN_RNG -> {
                    op.ints = new int[] {in.readUnsignedByte(), 0, 0};
                    op.value = in.readLong();
                    op.ints[1] = in.readUnsignedByte();
                    op.ints[2] = in.readInt();
                }
                case OP_DUMP -> {
                    op.ints = ints(in, 6);
                    int sections = in.readUnsignedByte();
                    op.ints = new int[] {op.ints[0], op.ints[1], op.ints[2], op.ints[3], op.ints[4], op.ints[5], sections};
                }
                case OP_GAMERULE -> {
                    op.s1 = str(in);
                    op.s2 = str(in);
                }
                default -> throw new IOException("unknown scenario opcode " + op.code);
            }
            s.ops.add(op);
        }
        if (in.available() != 0) {
            throw new IOException("trailing bytes in scenario file");
        }
        // Stable: operations with the same (tick, phase) keep their file order.
        s.ops.sort(Comparator.<Op>comparingInt(o -> o.tick).thenComparingInt(o -> o.phase));
        return s;
    }

    private static int[] ints(DataInputStream in, int n) throws IOException {
        int[] a = new int[n];
        for (int i = 0; i < n; i++) {
            a[i] = in.readInt();
        }
        return a;
    }

    static String str(DataInputStream in) throws IOException {
        int len = in.readUnsignedShort();
        return new String(in.readNBytes(len), StandardCharsets.UTF_8);
    }
}
