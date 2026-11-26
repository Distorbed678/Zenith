// Zenith parity oracle — original source, MIT OR Apache-2.0.
package dev.zenith.oracle;

import java.io.BufferedOutputStream;
import java.io.DataOutputStream;
import java.io.IOException;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;

/**
 * Exhaustive block-state transform table: for every block state in global id order, the global id of the state
 * after each (mirror, rotation) pair the game can apply when it places a block from a template.
 *
 * <p>Invocation:
 * <pre>{@code
 * BlockTransforms <output-file> [start-id end-id-exclusive]
 * }</pre>
 *
 * <p>Exact big-endian wire format ({@code ZBT1}) is specified in {@code docs/specs/parity/block-transforms.md};
 * the transform rules are specified in {@code docs/specs/blocks/state-transforms.md}.
 *
 * <p>Game classes are accessed solely through reflection by their public names so this tool compiles against the
 * JDK alone.
 */
public final class BlockTransforms {
    private static final byte[] MAGIC = {'Z', 'B', 'T', '1'};
    private static final int FORMAT_VERSION = 1;
    private static final int IO_BUFFER_BYTES = 64 * 1024;

    /** Rotation constant names in column order (the order of {@code Rotation.values()}). */
    private static final String[] ROTATIONS = {"NONE", "CLOCKWISE_90", "CLOCKWISE_180", "COUNTERCLOCKWISE_90"};
    /** Mirror constant names in row order (the order of {@code Mirror.values()}). */
    private static final String[] MIRRORS = {"NONE", "LEFT_RIGHT", "FRONT_BACK"};

    private BlockTransforms() {}

    public static void main(String[] args) throws Exception {
        if (args.length != 1 && args.length != 3) {
            System.err.println("usage: BlockTransforms <output-file> [start-id end-id-exclusive]");
            System.exit(2);
        }
        Path outputPath = Path.of(args[0]);
        Integer reqStart = null;
        Integer reqEnd = null;
        if (args.length == 3) {
            try {
                reqStart = Integer.parseInt(args[1]);
                reqEnd = Integer.parseInt(args[2]);
            } catch (NumberFormatException e) {
                System.err.println("[zenith-oracle] block transforms: invalid range arguments: " + args[1] + " " + args[2]);
                System.exit(2);
            }
        }

        try {
            long t0 = System.nanoTime();

            Class.forName("net.minecraft.SharedConstants").getMethod("tryDetectVersion").invoke(null);
            Class.forName("net.minecraft.server.Bootstrap").getMethod("bootStrap").invoke(null);

            GameApi api = new GameApi();
            int totalStates = api.stateCount();
            if (totalStates <= 0) {
                throw new IllegalStateException("Block state registry is empty after bootstrap");
            }

            int startId = reqStart != null ? reqStart : 0;
            int endId = reqEnd != null ? reqEnd : totalStates;
            if (startId < 0 || endId < startId || endId > totalStates) {
                throw new IllegalArgumentException(
                        "Invalid state id range [" + startId + ", " + endId + "); total states = " + totalStates);
            }
            int stateCount = endId - startId;

            Path parent = outputPath.toAbsolutePath().getParent();
            if (parent != null) {
                Files.createDirectories(parent);
            }

            int slots = MIRRORS.length * ROTATIONS.length;
            int[] changedPerSlot = new int[slots];
            int identityMismatches = 0;
            try (DataOutputStream out = new DataOutputStream(
                    new BufferedOutputStream(Files.newOutputStream(outputPath), IO_BUFFER_BYTES))) {
                out.write(MAGIC);
                out.writeShort(FORMAT_VERSION);
                out.writeInt(totalStates);
                out.writeInt(startId);
                out.writeInt(stateCount);

                for (int id = startId; id < endId; id++) {
                    Object state = api.stateById(id);
                    if (state == null) {
                        throw new IllegalStateException("Null BlockState at global id " + id);
                    }
                    Object block = api.bsGetBlock.invoke(state);
                    out.writeInt(id);
                    for (int m = 0; m < MIRRORS.length; m++) {
                        // Mirror first, then rotate: the order the game uses when it places a template block.
                        Object mirrored = api.bsMirror.invoke(state, api.mirrors[m]);
                        for (int r = 0; r < ROTATIONS.length; r++) {
                            Object result = api.bsRotate.invoke(mirrored, api.rotations[r]);
                            if (result == null) {
                                throw new IllegalStateException("Transform returned null for state " + id);
                            }
                            if (api.bsGetBlock.invoke(result) != block) {
                                throw new IllegalStateException("Transform changed the block of state " + id);
                            }
                            int resultId = api.idOf(result);
                            if (resultId < 0) {
                                throw new IllegalStateException("Transform of state " + id + " is not registered");
                            }
                            int slot = m * ROTATIONS.length + r;
                            if (slot == 0) {
                                if (resultId != id) {
                                    identityMismatches++;
                                }
                            } else if (resultId != id) {
                                changedPerSlot[slot]++;
                            }
                            out.writeInt(resultId);
                        }
                    }
                }
            }

            long elapsedMs = (System.nanoTime() - t0) / 1_000_000L;
            StringBuilder summary = new StringBuilder();
            for (int m = 0; m < MIRRORS.length; m++) {
                for (int r = 0; r < ROTATIONS.length; r++) {
                    int slot = m * ROTATIONS.length + r;
                    if (slot == 0) {
                        continue;
                    }
                    summary.append(' ').append(MIRRORS[m]).append('+').append(ROTATIONS[r]).append('=')
                            .append(changedPerSlot[slot]);
                }
            }
            System.out.println("[zenith-oracle] block transforms: " + stateCount + "/" + totalStates
                    + " states, identity mismatches " + identityMismatches + ", states changed:" + summary
                    + " in " + elapsedMs + " ms");
        } catch (InvocationTargetException e) {
            Throwable cause = e.getCause() != null ? e.getCause() : e;
            System.err.println("[zenith-oracle] block transforms failed: " + cause);
            cause.printStackTrace(System.err);
            System.exit(1);
        } catch (IllegalArgumentException | IllegalStateException | IOException e) {
            System.err.println("[zenith-oracle] block transforms error: " + e.getMessage());
            System.exit(1);
        }
    }

    /** Reflection handles to the game classes this tool touches. */
    private static final class GameApi {
        final Object blockStateRegistry;
        final Method idMapperSize;
        final Method idMapperById;
        final Method idMapperGetId;
        final Method bsGetBlock;
        final Method bsRotate;
        final Method bsMirror;
        final Object[] rotations;
        final Object[] mirrors;

        GameApi() throws Exception {
            Class<?> blockClass = Class.forName("net.minecraft.world.level.block.Block");
            Class<?> blockStateClass = Class.forName("net.minecraft.world.level.block.state.BlockState");
            Class<?> idMapperClass = Class.forName("net.minecraft.core.IdMapper");
            Class<?> rotationClass = Class.forName("net.minecraft.world.level.block.Rotation");
            Class<?> mirrorClass = Class.forName("net.minecraft.world.level.block.Mirror");

            this.blockStateRegistry = blockClass.getField("BLOCK_STATE_REGISTRY").get(null);
            this.idMapperSize = idMapperClass.getMethod("size");
            this.idMapperById = idMapperClass.getMethod("byId", int.class);
            this.idMapperGetId = idMapperClass.getMethod("getId", Object.class);
            this.bsGetBlock = blockStateClass.getMethod("getBlock");
            this.bsRotate = blockStateClass.getMethod("rotate", rotationClass);
            this.bsMirror = blockStateClass.getMethod("mirror", mirrorClass);
            this.rotations = constants(rotationClass, ROTATIONS);
            this.mirrors = constants(mirrorClass, MIRRORS);
        }

        private static Object[] constants(Class<?> enumClass, String[] names) {
            Object[] all = enumClass.getEnumConstants();
            Object[] out = new Object[names.length];
            for (int i = 0; i < names.length; i++) {
                for (Object c : all) {
                    if (((Enum<?>) c).name().equals(names[i])) {
                        out[i] = c;
                    }
                }
                if (out[i] == null) {
                    throw new IllegalStateException("Missing enum constant " + names[i] + " in " + enumClass.getName());
                }
            }
            return out;
        }

        int stateCount() throws Exception {
            return (Integer) idMapperSize.invoke(blockStateRegistry);
        }

        Object stateById(int id) throws Exception {
            return idMapperById.invoke(blockStateRegistry, id);
        }

        int idOf(Object state) throws Exception {
            return (Integer) idMapperGetId.invoke(blockStateRegistry, state);
        }
    }
}
