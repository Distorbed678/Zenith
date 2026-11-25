// Zenith parity oracle — original source, MIT OR Apache-2.0.
package dev.zenith.oracle;

import java.io.BufferedOutputStream;
import java.io.DataOutputStream;
import java.io.IOException;
import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.stream.Stream;

/**
 * Exhaustive static-facts dumper for every block state in global id order (ADR 0009).
 *
 * <p>Invocation:
 * <pre>{@code
 * BlockFacts <output-file> [start-id end-id-exclusive]
 * }</pre>
 *
 * <p>Exact big-endian wire format ({@code ZBF1}) is specified in {@code docs/specs/parity/block-facts.md}.
 *
 * <p>Game classes are accessed solely through reflection by their public names so this tool compiles against the
 * JDK alone.
 */
public final class BlockFacts {
    private static final byte[] MAGIC = {'Z', 'B', 'F', '1'};
    private static final int FORMAT_VERSION = 1;
    private static final int IO_BUFFER_BYTES = 64 * 1024;

    private static final int[][] PROBE_COORDS = {
        {0, 0, 0},
        {1, 0, 3},
        {-7, 64, 13},
    };
    private static final double[] VIEW_PROBE_AABB = {0.0, 0.0, 0.0, 1.0, 1.0, 1.0};

    private static final int OFFSET_NONE = 0;
    private static final int OFFSET_XZ = 1;
    private static final int OFFSET_XYZ = 2;

    private BlockFacts() {}

    public static void main(String[] args) throws Exception {
        if (args.length != 1 && args.length != 3) {
            System.err.println("usage: BlockFacts <output-file> [start-id end-id-exclusive]");
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
                System.err.println("[zenith-oracle] block facts: invalid range arguments: " + args[1] + " " + args[2]);
                System.exit(2);
            }
        }

        try {
            long t0 = System.nanoTime();

            // Bootstrap static registries and bind built-in server-data tags (heightmap motion tags,
            // suffocation tags, fluid tags, shulker tags, etc.).
            Class.forName("net.minecraft.SharedConstants").getMethod("tryDetectVersion").invoke(null);
            Class.forName("net.minecraft.server.Bootstrap").getMethod("bootStrap").invoke(null);
            bindVanillaStaticTags();

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

            int positionDependentCount = 0;
            try (DataOutputStream out = new DataOutputStream(
                    new BufferedOutputStream(Files.newOutputStream(outputPath), IO_BUFFER_BYTES))) {
                // Header
                out.write(MAGIC);
                out.writeShort(FORMAT_VERSION);
                out.writeInt(totalStates);
                out.writeInt(startId);
                out.writeInt(stateCount);
                for (int[] coord : PROBE_COORDS) {
                    out.writeInt(coord[0]);
                    out.writeInt(coord[1]);
                    out.writeInt(coord[2]);
                }
                writeBox(out, VIEW_PROBE_AABB);

                // Stream per-state records
                for (int id = startId; id < endId; id++) {
                    Object state = api.stateById(id);
                    if (state == null) {
                        throw new IllegalStateException("Null BlockState at global id " + id);
                    }
                    if (writeStateRecord(api, out, id, state)) {
                        positionDependentCount++;
                    }
                }
            }

            long elapsedMs = (System.nanoTime() - t0) / 1_000_000L;
            System.out.println("[zenith-oracle] block facts: " + stateCount + "/" + totalStates
                    + " states (" + positionDependentCount + " position-dependent) in " + elapsedMs + " ms");
        } catch (InvocationTargetException e) {
            Throwable cause = e.getCause() != null ? e.getCause() : e;
            System.err.println("[zenith-oracle] block facts failed: " + cause);
            cause.printStackTrace(System.err);
            System.exit(1);
        } catch (IllegalArgumentException e) {
            System.err.println("[zenith-oracle] block facts error: " + e.getMessage());
            System.exit(1);
        }
    }

    /**
     * Loads and applies built-in server-data tags onto static registries ({@code BuiltInRegistries.BLOCK},
     * {@code BuiltInRegistries.FLUID}, etc.) so tag-backed predicates match a running server.
     */
    private static void bindVanillaStaticTags() throws Exception {
        Object vanillaPack = Class.forName("net.minecraft.server.packs.repository.ServerPacksSource")
                .getMethod("createVanillaPackSource")
                .invoke(null);
        Object packResources = Class.forName("net.minecraft.server.packs.VanillaPackResources")
                .getMethod("fullResources")
                .invoke(vanillaPack);
        Class<?> packTypeClass = Class.forName("net.minecraft.server.packs.PackType");
        Object serverData = packTypeClass.getField("SERVER_DATA").get(null);
        Constructor<?> managerCtor = Class.forName("net.minecraft.server.packs.resources.MultiPackResourceManager")
                .getConstructor(packTypeClass, List.class);
        AutoCloseable resourceManager = (AutoCloseable) managerCtor.newInstance(serverData, List.of(packResources));
        try {
            Object rootRegistry = Class.forName("net.minecraft.core.registries.BuiltInRegistries")
                    .getField("REGISTRY")
                    .get(null);
            Class<?> registryClass = Class.forName("net.minecraft.core.Registry");
            Object staticAccess = Class.forName("net.minecraft.core.RegistryAccess")
                    .getMethod("fromRegistryOfRegistries", registryClass)
                    .invoke(null, rootRegistry);
            Method loadTags = Class.forName("net.minecraft.tags.TagLoader")
                    .getMethod(
                            "loadTagsForExistingRegistries",
                            Class.forName("net.minecraft.server.packs.resources.ResourceManager"),
                            Class.forName("net.minecraft.core.RegistryAccess"));
            List<?> pending = (List<?>) loadTags.invoke(null, resourceManager, staticAccess);
            Method apply = Class.forName("net.minecraft.core.Registry$PendingTags").getMethod("apply");
            for (Object p : pending) {
                apply.invoke(p);
            }
        } finally {
            resourceManager.close();
        }
    }

    /** Writes one block-state record and returns {@code true} if any shape differed across probe positions. */
    private static boolean writeStateRecord(GameApi api, DataOutputStream out, int globalId, Object state)
            throws Exception {
        Object block = api.bsGetBlock.invoke(state);
        String blockId = api.registryKey(api.blockRegistry, block);

        // 1. Identity
        out.writeInt(globalId);
        writeStr(out, blockId);
        List<?> propValues;
        try (Stream<?> stream = (Stream<?>) api.shGetValues.invoke(state)) {
            propValues = stream.toList();
        }
        if (propValues.size() > 255) {
            throw new IllegalStateException("State " + globalId + " (" + blockId + ") has >255 properties");
        }
        out.writeByte(propValues.size());
        for (Object pv : propValues) {
            Object prop = api.pvProperty.invoke(pv);
            String propName = (String) api.propGetName.invoke(prop);
            String valName = (String) api.pvValueName.invoke(pv);
            writeStr(out, propName);
            writeStr(out, valName);
        }

        // 2. Flags (u32 bitfield)
        Object pos0 = api.probePos[0];
        Object emptyGetter = api.emptyBlockGetter;
        Object defaultState = api.blockDefaultState.invoke(block);

        int flags = 0;
        if (state == defaultState) flags |= 1 << 0;
        if (bool(api.bsIsAir.invoke(state))) flags |= 1 << 1;
        if (bool(api.bsCanOcclude.invoke(state))) flags |= 1 << 2;
        if (bool(api.bsIsSolidRender.invoke(state))) flags |= 1 << 3;
        if (bool(api.bsPropagatesSkylightDown.invoke(state))) flags |= 1 << 4;
        if (bool(api.bsUseShapeForLightOcclusion.invoke(state))) flags |= 1 << 5;
        if (bool(api.bsIsCollisionShapeFullBlock.invoke(state, emptyGetter, pos0))) flags |= 1 << 6;
        if (bool(api.bsHasLargeCollisionShape.invoke(state))) flags |= 1 << 7;
        if (bool(api.blockHasDynamicShape.invoke(block))) flags |= 1 << 8;
        if (bool(api.bsIsRandomlyTicking.invoke(state))) flags |= 1 << 9;
        if (bool(api.bsCanBeReplaced.invoke(state))) flags |= 1 << 10;
        if (bool(api.bsLiquid.invoke(state))) flags |= 1 << 11;
        if (bool(api.bsIsSolid.invoke(state))) flags |= 1 << 12;
        if (bool(api.bsIsTag.invoke(state, api.tagBlocksMotionInHeightmap))) flags |= 1 << 13;
        if (bool(api.bsIsTag.invoke(state, api.tagBlocksMotionInHeightmapNoLeaves))) flags |= 1 << 14;
        if (bool(api.bsIgnitedByLava.invoke(state))) flags |= 1 << 15;
        if (bool(api.bsRequiresCorrectToolForDrops.invoke(state))) flags |= 1 << 16;
        if (bool(api.bsIsRedstoneConductor.invoke(state, emptyGetter, pos0))) flags |= 1 << 17;
        if (bool(api.bsIsSuffocating.invoke(state, emptyGetter, pos0))) flags |= 1 << 18;
        if (bool(api.bsIsViewBlocking.invoke(state, emptyGetter, pos0, api.viewProbeAabb))) flags |= 1 << 19;
        if (bool(api.bsEmissiveRendering.invoke(state))) flags |= 1 << 20;
        if (bool(api.bsIsSignalSource.invoke(state))) flags |= 1 << 21;
        if (bool(api.bsHasAnalogOutputSignal.invoke(state))) flags |= 1 << 22;
        if (bool(api.bsHasBlockEntity.invoke(state))) flags |= 1 << 23;
        if (bool(api.bsShouldSpawnTerrainParticles.invoke(state))) flags |= 1 << 24;
        if (bool(api.bsIsLightPermeable.invoke(state))) flags |= 1 << 25;
        if (bool(api.bsIsPathfindable.invoke(state, api.pathLand))) flags |= 1 << 26;
        if (bool(api.bsIsPathfindable.invoke(state, api.pathWater))) flags |= 1 << 27;
        if (bool(api.bsIsPathfindable.invoke(state, api.pathAir))) flags |= 1 << 28;
        if (bool(api.blockIsPossibleToRespawnInThis.invoke(block, state))) flags |= 1 << 29;
        if (bool(api.blockIsExceptionForConnection.invoke(null, state))) flags |= 1 << 30;
        if (bool(api.bsCanBeReplacedFluid.invoke(state, api.fluidWater))) flags |= 1 << 31;
        out.writeInt(flags);

        // 3. Scalars, enums, offsets, post-process
        int lightBlock = (Integer) api.bsGetLightDampening.invoke(state);
        int lightEmission = (Integer) api.bsGetLightEmission.invoke(state);
        Object mapColor = api.bsGetMapColor.invoke(state, emptyGetter, pos0);
        int mapColorId = api.mcId.getInt(mapColor);
        int mapColorRgb = api.mcCol.getInt(mapColor);
        String pushReaction = ((Enum<?>) api.bsGetPistonPushReaction.invoke(state)).name();
        String instrument = (String) api.nbiGetSerializedName.invoke(api.bsInstrument.invoke(state));
        int renderShape = ((Enum<?>) api.bsGetRenderShape.invoke(state)).ordinal();

        out.writeByte(lightBlock);
        out.writeByte(lightEmission);
        out.writeByte(mapColorId);
        out.writeInt(mapColorRgb);
        writeStr(out, pushReaction);
        writeStr(out, instrument);
        out.writeByte(renderShape);

        boolean hasOffset = bool(api.bsHasOffsetFunction.invoke(state));
        if (!hasOffset) {
            out.writeByte(OFFSET_NONE);
        } else {
            double[][] offsets = new double[3][3];
            boolean anyVertical = false;
            for (int p = 0; p < 3; p++) {
                Object vec = api.bsGetOffset.invoke(state, api.probePos[p]);
                offsets[p][0] = api.vecX.getDouble(vec);
                offsets[p][1] = api.vecY.getDouble(vec);
                offsets[p][2] = api.vecZ.getDouble(vec);
                if (offsets[p][1] != 0.0) {
                    anyVertical = true;
                }
            }
            if (!anyVertical) {
                for (Object extraPos : api.offsetClassifyPos) {
                    Object vec = api.bsGetOffset.invoke(state, extraPos);
                    if (api.vecY.getDouble(vec) != 0.0) {
                        anyVertical = true;
                        break;
                    }
                }
            }
            out.writeByte(anyVertical ? OFFSET_XYZ : OFFSET_XZ);
            for (int p = 0; p < 3; p++) {
                writeF64(out, offsets[p][0]);
                writeF64(out, offsets[p][1]);
                writeF64(out, offsets[p][2]);
            }
        }

        Object postProcessPos = api.bsGetPostProcessPos.invoke(state, emptyGetter, pos0);
        if (postProcessPos == null) {
            out.writeByte(0);
        } else {
            out.writeByte(1);
            out.writeInt((Integer) api.bpGetX.invoke(postProcessPos) - PROBE_COORDS[0][0]);
            out.writeInt((Integer) api.bpGetY.invoke(postProcessPos) - PROBE_COORDS[0][1]);
            out.writeInt((Integer) api.bpGetZ.invoke(postProcessPos) - PROBE_COORDS[0][2]);
        }

        writeF32(out, (Float) api.bsGetDestroySpeed.invoke(state, emptyGetter, pos0));
        writeF32(out, (Float) api.blockGetExplosionResistance.invoke(block));
        writeF32(out, (Float) api.blockGetFriction.invoke(block));
        writeF32(out, (Float) api.blockGetSpeedFactor.invoke(block));
        writeF32(out, (Float) api.blockGetJumpFactor.invoke(block));
        writeF32(out, (Float) api.blockGetBounceRestitution.invoke(block));
        writeF32(out, (Float) api.blockGetFallDistanceReduction.invoke(block));
        writeF32(out, (Float) api.bsGetShadeBrightness.invoke(state, emptyGetter, pos0));

        // 4. Sound type
        Object soundType = api.bsGetSoundType.invoke(state);
        writeF32(out, (Float) api.stGetVolume.invoke(soundType));
        writeF32(out, (Float) api.stGetPitch.invoke(soundType));
        writeStr(out, api.soundEventId(api.stGetBreakSound.invoke(soundType)));
        writeStr(out, api.soundEventId(api.stGetStepSound.invoke(soundType)));
        writeStr(out, api.soundEventId(api.stGetPlaceSound.invoke(soundType)));
        writeStr(out, api.soundEventId(api.stGetHitSound.invoke(soundType)));
        writeStr(out, api.soundEventId(api.stGetFallSound.invoke(soundType)));

        // 5. Fluid state
        Object fluidState = api.bsGetFluidState.invoke(state);
        Object fluid = api.fsGetType.invoke(fluidState);
        String fluidId = api.registryKey(api.fluidRegistry, fluid);
        int fluidAmount = (Integer) api.fsGetAmount.invoke(fluidState);
        boolean fluidFalling = bool(api.shHasProperty.invoke(fluidState, api.fallingProp))
                && bool(api.shGetValue.invoke(fluidState, api.fallingProp));
        boolean fluidIsSource = bool(api.fsIsSource.invoke(fluidState));
        boolean fluidIsEmpty = bool(api.fsIsEmpty.invoke(fluidState));
        float fluidOwnHeight = (Float) api.fsGetOwnHeight.invoke(fluidState);

        int fluidFlags = 0;
        if (fluidFalling) fluidFlags |= 1 << 0;
        if (fluidIsSource) fluidFlags |= 1 << 1;
        if (fluidIsEmpty) fluidFlags |= 1 << 2;

        writeStr(out, fluidId);
        out.writeByte(fluidAmount);
        out.writeByte(fluidFlags);
        writeF32(out, fluidOwnHeight);

        // 6. Shapes
        Object occlusionShape = api.bsGetOcclusionShape.invoke(state);
        writeShape(out, api.extractShape(occlusionShape));

        Object[] shapesP0 = evalPosShapes(api, state, api.probePos[0]);
        Object[] shapesP1 = evalPosShapes(api, state, api.probePos[1]);
        Object[] shapesP2 = evalPosShapes(api, state, api.probePos[2]);

        ShapeData[] dataP0 = new ShapeData[5];
        ShapeData[] dataP1 = null;
        ShapeData[] dataP2 = null;
        int diffMask = 0;

        for (int i = 0; i < 5; i++) {
            dataP0[i] = api.extractShape(shapesP0[i]);
            writeShape(out, dataP0[i]);
            if (shapesP1[i] != shapesP0[i] || shapesP2[i] != shapesP0[i]) {
                ShapeData d1 = shapesP1[i] == shapesP0[i] ? dataP0[i] : api.extractShape(shapesP1[i]);
                ShapeData d2 = shapesP2[i] == shapesP0[i] ? dataP0[i] : api.extractShape(shapesP2[i]);
                if (!dataP0[i].exactEquals(d1) || !dataP0[i].exactEquals(d2)) {
                    diffMask |= 1 << i;
                    if (dataP1 == null) {
                        dataP1 = new ShapeData[5];
                        dataP2 = new ShapeData[5];
                    }
                    dataP1[i] = d1;
                    dataP2[i] = d2;
                }
            }
        }

        out.writeByte(diffMask);
        if (diffMask != 0) {
            for (int i = 0; i < 5; i++) {
                ShapeData d1 = dataP1[i] != null ? dataP1[i]
                        : (shapesP1[i] == shapesP0[i] ? dataP0[i] : api.extractShape(shapesP1[i]));
                writeShape(out, d1);
            }
            for (int i = 0; i < 5; i++) {
                ShapeData d2 = dataP2[i] != null ? dataP2[i]
                        : (shapesP2[i] == shapesP0[i] ? dataP0[i] : api.extractShape(shapesP2[i]));
                writeShape(out, d2);
            }
        }

        // 7. Face facts per direction (DOWN=0, UP=1, NORTH=2, SOUTH=3, WEST=4, EAST=5)
        int sturdyFullMask = 0;
        int sturdyCenterMask = 0;
        int sturdyRigidMask = 0;
        int collisionFullMask = 0;
        Object collisionShape0 = shapesP0[0];

        for (int d = 0; d < 6; d++) {
            Object dir = api.directions[d];
            if (bool(api.bsIsFaceSturdy.invoke(state, emptyGetter, pos0, dir, api.supportFull))) {
                sturdyFullMask |= 1 << d;
            }
            if (bool(api.bsIsFaceSturdy.invoke(state, emptyGetter, pos0, dir, api.supportCenter))) {
                sturdyCenterMask |= 1 << d;
            }
            if (bool(api.bsIsFaceSturdy.invoke(state, emptyGetter, pos0, dir, api.supportRigid))) {
                sturdyRigidMask |= 1 << d;
            }
            if (bool(api.blockIsFaceFull.invoke(null, collisionShape0, dir))) {
                collisionFullMask |= 1 << d;
            }
        }
        out.writeByte(sturdyFullMask);
        out.writeByte(sturdyCenterMask);
        out.writeByte(sturdyRigidMask);
        out.writeByte(collisionFullMask);

        for (int d = 0; d < 6; d++) {
            Object faceOcc = api.bsGetFaceOcclusionShape.invoke(state, api.directions[d]);
            writeShape(out, api.extractShape(faceOcc));
        }

        return diffMask != 0;
    }

    private static Object[] evalPosShapes(GameApi api, Object state, Object pos) throws Exception {
        Object getter = api.emptyBlockGetter;
        Object ctx = api.emptyCollisionContext;
        return new Object[] {
            api.bsGetCollisionShape.invoke(state, getter, pos, ctx),
            api.bsGetShape.invoke(state, getter, pos, ctx),
            api.bsGetVisualShape.invoke(state, getter, pos, ctx),
            api.bsGetInteractionShape.invoke(state, getter, pos),
            api.bsGetBlockSupportShape.invoke(state, getter, pos),
        };
    }

    // ── Shape representation and binary I/O helpers ──────────────────────────

    private static final class ShapeData {
        static final ShapeData EMPTY = new ShapeData(null, new double[0]);

        final double[] bounds; // null when empty, length 6 otherwise
        final double[] boxes;  // length 6 * boxCount

        ShapeData(double[] bounds, double[] boxes) {
            this.bounds = bounds;
            this.boxes = boxes;
        }

        int boxCount() {
            return boxes.length / 6;
        }

        boolean exactEquals(ShapeData other) {
            if (this == other) {
                return true;
            }
            if (this.boxes.length != other.boxes.length) {
                return false;
            }
            if (this.bounds == null && other.bounds == null) {
                return true;
            }
            if (this.bounds == null || other.bounds == null) {
                return false;
            }
            for (int i = 0; i < 6; i++) {
                if (Double.doubleToRawLongBits(this.bounds[i]) != Double.doubleToRawLongBits(other.bounds[i])) {
                    return false;
                }
            }
            for (int i = 0; i < this.boxes.length; i++) {
                if (Double.doubleToRawLongBits(this.boxes[i]) != Double.doubleToRawLongBits(other.boxes[i])) {
                    return false;
                }
            }
            return true;
        }
    }

    private static boolean bool(Object o) {
        return (Boolean) o;
    }

    private static void writeStr(DataOutputStream out, String s) throws IOException {
        byte[] bytes = s.getBytes(StandardCharsets.UTF_8);
        if (bytes.length > 0xFFFF) {
            throw new IllegalArgumentException("String exceeds u16 byte length: " + bytes.length);
        }
        out.writeShort(bytes.length);
        out.write(bytes);
    }

    private static void writeF32(DataOutputStream out, float v) throws IOException {
        out.writeInt(Float.floatToRawIntBits(v));
    }

    private static void writeF64(DataOutputStream out, double v) throws IOException {
        out.writeLong(Double.doubleToRawLongBits(v));
    }

    private static void writeBox(DataOutputStream out, double[] b) throws IOException {
        for (int i = 0; i < 6; i++) {
            writeF64(out, b[i]);
        }
    }

    private static void writeShape(DataOutputStream out, ShapeData s) throws IOException {
        int count = s.boxCount();
        if (count > 0xFFFF) {
            throw new IllegalStateException("Shape box count exceeds u16: " + count);
        }
        out.writeShort(count);
        if (count > 0) {
            writeBox(out, s.bounds);
            for (int i = 0; i < s.boxes.length; i++) {
                writeF64(out, s.boxes[i]);
            }
        }
    }

    // ── Reflected game API ───────────────────────────────────────────────────

    private static final class GameApi {
        final Object blockStateRegistry;
        final Method idMapperSize;
        final Method idMapperById;

        final Object blockRegistry;
        final Object fluidRegistry;
        final Method registryGetKey;

        final Object emptyBlockGetter;
        final Object emptyCollisionContext;
        final Object[] probePos;
        final Object[] offsetClassifyPos;
        final Object viewProbeAabb;

        final Object[] directions;
        final Object supportFull;
        final Object supportCenter;
        final Object supportRigid;

        final Object pathLand;
        final Object pathWater;
        final Object pathAir;

        final Object tagBlocksMotionInHeightmap;
        final Object tagBlocksMotionInHeightmapNoLeaves;
        final Object fluidWater;
        final Object fallingProp;

        // StateHolder / Property
        final Method shGetValues;
        final Method shHasProperty;
        final Method shGetValue;
        final Method pvProperty;
        final Method pvValueName;
        final Method propGetName;

        // BlockState / BlockStateBase
        final Method bsGetBlock;
        final Method bsIsAir;
        final Method bsCanOcclude;
        final Method bsIsSolidRender;
        final Method bsPropagatesSkylightDown;
        final Method bsUseShapeForLightOcclusion;
        final Method bsIsCollisionShapeFullBlock;
        final Method bsHasLargeCollisionShape;
        final Method bsIsRandomlyTicking;
        final Method bsCanBeReplaced;
        final Method bsCanBeReplacedFluid;
        final Method bsLiquid;
        final Method bsIsSolid;
        final Method bsIsTag;
        final Method bsIgnitedByLava;
        final Method bsRequiresCorrectToolForDrops;
        final Method bsIsRedstoneConductor;
        final Method bsIsSuffocating;
        final Method bsIsViewBlocking;
        final Method bsEmissiveRendering;
        final Method bsIsSignalSource;
        final Method bsHasAnalogOutputSignal;
        final Method bsHasBlockEntity;
        final Method bsShouldSpawnTerrainParticles;
        final Method bsIsLightPermeable;
        final Method bsIsPathfindable;
        final Method bsGetLightDampening;
        final Method bsGetLightEmission;
        final Method bsGetMapColor;
        final Method bsGetPistonPushReaction;
        final Method bsInstrument;
        final Method bsGetRenderShape;
        final Method bsHasOffsetFunction;
        final Method bsGetOffset;
        final Method bsGetPostProcessPos;
        final Method bsGetDestroySpeed;
        final Method bsGetShadeBrightness;
        final Method bsGetSoundType;
        final Method bsGetFluidState;
        final Method bsGetOcclusionShape;
        final Method bsGetCollisionShape;
        final Method bsGetShape;
        final Method bsGetVisualShape;
        final Method bsGetInteractionShape;
        final Method bsGetBlockSupportShape;
        final Method bsIsFaceSturdy;
        final Method bsGetFaceOcclusionShape;

        // Block
        final Method blockDefaultState;
        final Method blockHasDynamicShape;
        final Method blockGetExplosionResistance;
        final Method blockGetFriction;
        final Method blockGetSpeedFactor;
        final Method blockGetJumpFactor;
        final Method blockGetBounceRestitution;
        final Method blockGetFallDistanceReduction;
        final Method blockIsPossibleToRespawnInThis;
        final Method blockIsExceptionForConnection;
        final Method blockIsFaceFull;

        // MapColor, NoteBlockInstrument, Vec3, BlockPos, AABB, VoxelShape, SoundType, SoundEvent, FluidState
        final Field mcId;
        final Field mcCol;
        final Method nbiGetSerializedName;
        final Field vecX;
        final Field vecY;
        final Field vecZ;
        final Method bpGetX;
        final Method bpGetY;
        final Method bpGetZ;
        final Field aabbMinX;
        final Field aabbMinY;
        final Field aabbMinZ;
        final Field aabbMaxX;
        final Field aabbMaxY;
        final Field aabbMaxZ;
        final Method vsIsEmpty;
        final Method vsBounds;
        final Method vsToAabbs;
        final Method stGetVolume;
        final Method stGetPitch;
        final Method stGetBreakSound;
        final Method stGetStepSound;
        final Method stGetPlaceSound;
        final Method stGetHitSound;
        final Method stGetFallSound;
        final Method seLocation;
        final Method fsGetType;
        final Method fsGetAmount;
        final Method fsIsSource;
        final Method fsIsEmpty;
        final Method fsGetOwnHeight;

        GameApi() throws Exception {
            Class<?> blockClass = Class.forName("net.minecraft.world.level.block.Block");
            Class<?> blockStateClass = Class.forName("net.minecraft.world.level.block.state.BlockState");
            Class<?> stateHolderClass = Class.forName("net.minecraft.world.level.block.state.StateHolder");
            Class<?> propClass = Class.forName("net.minecraft.world.level.block.state.properties.Property");
            Class<?> propValueClass = Class.forName("net.minecraft.world.level.block.state.properties.Property$Value");
            Class<?> idMapperClass = Class.forName("net.minecraft.core.IdMapper");
            Class<?> registryClass = Class.forName("net.minecraft.core.Registry");
            Class<?> builtInRegistriesClass = Class.forName("net.minecraft.core.registries.BuiltInRegistries");
            Class<?> blockGetterClass = Class.forName("net.minecraft.world.level.BlockGetter");
            Class<?> emptyBlockGetterClass = Class.forName("net.minecraft.world.level.EmptyBlockGetter");
            Class<?> collisionContextClass = Class.forName("net.minecraft.world.phys.shapes.CollisionContext");
            Class<?> voxelShapeClass = Class.forName("net.minecraft.world.phys.shapes.VoxelShape");
            Class<?> blockPosClass = Class.forName("net.minecraft.core.BlockPos");
            Class<?> directionClass = Class.forName("net.minecraft.core.Direction");
            Class<?> supportTypeClass = Class.forName("net.minecraft.world.level.block.SupportType");
            Class<?> pathCompClass = Class.forName("net.minecraft.world.level.pathfinder.PathComputationType");
            Class<?> tagKeyClass = Class.forName("net.minecraft.tags.TagKey");
            Class<?> blockTagsClass = Class.forName("net.minecraft.tags.BlockTags");
            Class<?> aabbClass = Class.forName("net.minecraft.world.phys.AABB");
            Class<?> vec3Class = Class.forName("net.minecraft.world.phys.Vec3");
            Class<?> mapColorClass = Class.forName("net.minecraft.world.level.material.MapColor");
            Class<?> instrumentClass = Class.forName("net.minecraft.world.level.block.state.properties.NoteBlockInstrument");
            Class<?> soundTypeClass = Class.forName("net.minecraft.world.level.block.SoundType");
            Class<?> soundEventClass = Class.forName("net.minecraft.sounds.SoundEvent");
            Class<?> fluidClass = Class.forName("net.minecraft.world.level.material.Fluid");
            Class<?> fluidsClass = Class.forName("net.minecraft.world.level.material.Fluids");
            Class<?> flowingFluidClass = Class.forName("net.minecraft.world.level.material.FlowingFluid");
            Class<?> fluidStateClass = Class.forName("net.minecraft.world.level.material.FluidState");

            this.blockStateRegistry = blockClass.getField("BLOCK_STATE_REGISTRY").get(null);
            this.idMapperSize = idMapperClass.getMethod("size");
            this.idMapperById = idMapperClass.getMethod("byId", int.class);

            this.blockRegistry = builtInRegistriesClass.getField("BLOCK").get(null);
            this.fluidRegistry = builtInRegistriesClass.getField("FLUID").get(null);
            this.registryGetKey = registryClass.getMethod("getKey", Object.class);

            this.emptyBlockGetter = emptyBlockGetterClass.getField("INSTANCE").get(null);
            this.emptyCollisionContext = collisionContextClass.getMethod("empty").invoke(null);

            Constructor<?> bpCtor = blockPosClass.getConstructor(int.class, int.class, int.class);
            this.probePos = new Object[PROBE_COORDS.length];
            for (int i = 0; i < PROBE_COORDS.length; i++) {
                this.probePos[i] = bpCtor.newInstance(PROBE_COORDS[i][0], PROBE_COORDS[i][1], PROBE_COORDS[i][2]);
            }
            this.offsetClassifyPos = new Object[16];
            for (int x = 0; x < 16; x++) {
                this.offsetClassifyPos[x] = bpCtor.newInstance(x, 0, 0);
            }

            Constructor<?> aabbCtor = aabbClass.getConstructor(
                    double.class, double.class, double.class, double.class, double.class, double.class);
            this.viewProbeAabb = aabbCtor.newInstance(
                    VIEW_PROBE_AABB[0], VIEW_PROBE_AABB[1], VIEW_PROBE_AABB[2],
                    VIEW_PROBE_AABB[3], VIEW_PROBE_AABB[4], VIEW_PROBE_AABB[5]);

            this.directions = new Object[] {
                directionClass.getField("DOWN").get(null),
                directionClass.getField("UP").get(null),
                directionClass.getField("NORTH").get(null),
                directionClass.getField("SOUTH").get(null),
                directionClass.getField("WEST").get(null),
                directionClass.getField("EAST").get(null),
            };
            this.supportFull = supportTypeClass.getField("FULL").get(null);
            this.supportCenter = supportTypeClass.getField("CENTER").get(null);
            this.supportRigid = supportTypeClass.getField("RIGID").get(null);

            this.pathLand = pathCompClass.getField("LAND").get(null);
            this.pathWater = pathCompClass.getField("WATER").get(null);
            this.pathAir = pathCompClass.getField("AIR").get(null);

            this.tagBlocksMotionInHeightmap = blockTagsClass.getField("BLOCKS_MOTION_IN_HEIGHTMAP").get(null);
            this.tagBlocksMotionInHeightmapNoLeaves = blockTagsClass.getField("BLOCKS_MOTION_IN_HEIGHTMAP_NO_LEAVES").get(null);
            this.fluidWater = fluidsClass.getField("WATER").get(null);
            this.fallingProp = flowingFluidClass.getField("FALLING").get(null);

            // StateHolder / Property
            this.shGetValues = stateHolderClass.getMethod("getValues");
            this.shHasProperty = stateHolderClass.getMethod("hasProperty", propClass);
            this.shGetValue = stateHolderClass.getMethod("getValue", propClass);
            this.pvProperty = propValueClass.getMethod("property");
            this.pvValueName = propValueClass.getMethod("valueName");
            this.propGetName = propClass.getMethod("getName");

            // BlockState methods
            this.bsGetBlock = blockStateClass.getMethod("getBlock");
            this.bsIsAir = blockStateClass.getMethod("isAir");
            this.bsCanOcclude = blockStateClass.getMethod("canOcclude");
            this.bsIsSolidRender = blockStateClass.getMethod("isSolidRender");
            this.bsPropagatesSkylightDown = blockStateClass.getMethod("propagatesSkylightDown");
            this.bsUseShapeForLightOcclusion = blockStateClass.getMethod("useShapeForLightOcclusion");
            this.bsIsCollisionShapeFullBlock = blockStateClass.getMethod("isCollisionShapeFullBlock", blockGetterClass, blockPosClass);
            this.bsHasLargeCollisionShape = blockStateClass.getMethod("hasLargeCollisionShape");
            this.bsIsRandomlyTicking = blockStateClass.getMethod("isRandomlyTicking");
            this.bsCanBeReplaced = blockStateClass.getMethod("canBeReplaced");
            this.bsCanBeReplacedFluid = blockStateClass.getMethod("canBeReplaced", fluidClass);
            this.bsLiquid = blockStateClass.getMethod("liquid");
            this.bsIsSolid = blockStateClass.getMethod("isSolid");
            this.bsIsTag = blockStateClass.getMethod("is", tagKeyClass);
            this.bsIgnitedByLava = blockStateClass.getMethod("ignitedByLava");
            this.bsRequiresCorrectToolForDrops = blockStateClass.getMethod("requiresCorrectToolForDrops");
            this.bsIsRedstoneConductor = blockStateClass.getMethod("isRedstoneConductor", blockGetterClass, blockPosClass);
            this.bsIsSuffocating = blockStateClass.getMethod("isSuffocating", blockGetterClass, blockPosClass);
            this.bsIsViewBlocking = blockStateClass.getMethod("isViewBlocking", blockGetterClass, blockPosClass, aabbClass);
            this.bsEmissiveRendering = blockStateClass.getMethod("emissiveRendering");
            this.bsIsSignalSource = blockStateClass.getMethod("isSignalSource");
            this.bsHasAnalogOutputSignal = blockStateClass.getMethod("hasAnalogOutputSignal");
            this.bsHasBlockEntity = blockStateClass.getMethod("hasBlockEntity");
            this.bsShouldSpawnTerrainParticles = blockStateClass.getMethod("shouldSpawnTerrainParticles");
            this.bsIsLightPermeable = blockStateClass.getMethod("isLightPermeable");
            this.bsIsPathfindable = blockStateClass.getMethod("isPathfindable", pathCompClass);
            this.bsGetLightDampening = blockStateClass.getMethod("getLightDampening");
            this.bsGetLightEmission = blockStateClass.getMethod("getLightEmission");
            this.bsGetMapColor = blockStateClass.getMethod("getMapColor", blockGetterClass, blockPosClass);
            this.bsGetPistonPushReaction = blockStateClass.getMethod("getPistonPushReaction");
            this.bsInstrument = blockStateClass.getMethod("instrument");
            this.bsGetRenderShape = blockStateClass.getMethod("getRenderShape");
            this.bsHasOffsetFunction = blockStateClass.getMethod("hasOffsetFunction");
            this.bsGetOffset = blockStateClass.getMethod("getOffset", blockPosClass);
            this.bsGetPostProcessPos = blockStateClass.getMethod("getPostProcessPos", blockGetterClass, blockPosClass);
            this.bsGetDestroySpeed = blockStateClass.getMethod("getDestroySpeed", blockGetterClass, blockPosClass);
            this.bsGetShadeBrightness = blockStateClass.getMethod("getShadeBrightness", blockGetterClass, blockPosClass);
            this.bsGetSoundType = blockStateClass.getMethod("getSoundType");
            this.bsGetFluidState = blockStateClass.getMethod("getFluidState");
            this.bsGetOcclusionShape = blockStateClass.getMethod("getOcclusionShape");
            this.bsGetCollisionShape = blockStateClass.getMethod("getCollisionShape", blockGetterClass, blockPosClass, collisionContextClass);
            this.bsGetShape = blockStateClass.getMethod("getShape", blockGetterClass, blockPosClass, collisionContextClass);
            this.bsGetVisualShape = blockStateClass.getMethod("getVisualShape", blockGetterClass, blockPosClass, collisionContextClass);
            this.bsGetInteractionShape = blockStateClass.getMethod("getInteractionShape", blockGetterClass, blockPosClass);
            this.bsGetBlockSupportShape = blockStateClass.getMethod("getBlockSupportShape", blockGetterClass, blockPosClass);
            this.bsIsFaceSturdy = blockStateClass.getMethod("isFaceSturdy", blockGetterClass, blockPosClass, directionClass, supportTypeClass);
            this.bsGetFaceOcclusionShape = blockStateClass.getMethod("getFaceOcclusionShape", directionClass);

            // Block methods
            this.blockDefaultState = blockClass.getMethod("defaultBlockState");
            this.blockHasDynamicShape = blockClass.getMethod("hasDynamicShape");
            this.blockGetExplosionResistance = blockClass.getMethod("getExplosionResistance");
            this.blockGetFriction = blockClass.getMethod("getFriction");
            this.blockGetSpeedFactor = blockClass.getMethod("getSpeedFactor");
            this.blockGetJumpFactor = blockClass.getMethod("getJumpFactor");
            this.blockGetBounceRestitution = blockClass.getMethod("getBounceRestitution");
            this.blockGetFallDistanceReduction = blockClass.getMethod("getFallDistanceReduction");
            this.blockIsPossibleToRespawnInThis = blockClass.getMethod("isPossibleToRespawnInThis", blockStateClass);
            this.blockIsExceptionForConnection = blockClass.getMethod("isExceptionForConnection", blockStateClass);
            this.blockIsFaceFull = blockClass.getMethod("isFaceFull", voxelShapeClass, directionClass);

            // Supporting types
            this.mcId = mapColorClass.getField("id");
            this.mcCol = mapColorClass.getField("col");
            this.nbiGetSerializedName = instrumentClass.getMethod("getSerializedName");
            this.vecX = vec3Class.getField("x");
            this.vecY = vec3Class.getField("y");
            this.vecZ = vec3Class.getField("z");
            this.bpGetX = blockPosClass.getMethod("getX");
            this.bpGetY = blockPosClass.getMethod("getY");
            this.bpGetZ = blockPosClass.getMethod("getZ");
            this.aabbMinX = aabbClass.getField("minX");
            this.aabbMinY = aabbClass.getField("minY");
            this.aabbMinZ = aabbClass.getField("minZ");
            this.aabbMaxX = aabbClass.getField("maxX");
            this.aabbMaxY = aabbClass.getField("maxY");
            this.aabbMaxZ = aabbClass.getField("maxZ");
            this.vsIsEmpty = voxelShapeClass.getMethod("isEmpty");
            this.vsBounds = voxelShapeClass.getMethod("bounds");
            this.vsToAabbs = voxelShapeClass.getMethod("toAabbs");
            this.stGetVolume = soundTypeClass.getMethod("getVolume");
            this.stGetPitch = soundTypeClass.getMethod("getPitch");
            this.stGetBreakSound = soundTypeClass.getMethod("getBreakSound");
            this.stGetStepSound = soundTypeClass.getMethod("getStepSound");
            this.stGetPlaceSound = soundTypeClass.getMethod("getPlaceSound");
            this.stGetHitSound = soundTypeClass.getMethod("getHitSound");
            this.stGetFallSound = soundTypeClass.getMethod("getFallSound");
            this.seLocation = soundEventClass.getMethod("location");
            this.fsGetType = fluidStateClass.getMethod("getType");
            this.fsGetAmount = fluidStateClass.getMethod("getAmount");
            this.fsIsSource = fluidStateClass.getMethod("isSource");
            this.fsIsEmpty = fluidStateClass.getMethod("isEmpty");
            this.fsGetOwnHeight = fluidStateClass.getMethod("getOwnHeight");
        }

        int stateCount() throws Exception {
            return (Integer) idMapperSize.invoke(blockStateRegistry);
        }

        Object stateById(int id) throws Exception {
            return idMapperById.invoke(blockStateRegistry, id);
        }

        String registryKey(Object registry, Object entry) throws Exception {
            Object key = registryGetKey.invoke(registry, entry);
            if (key == null) {
                throw new IllegalStateException("Unregistered entry: " + entry);
            }
            return key.toString();
        }

        String soundEventId(Object soundEvent) throws Exception {
            return seLocation.invoke(soundEvent).toString();
        }

        ShapeData extractShape(Object shape) throws Exception {
            if (bool(vsIsEmpty.invoke(shape))) {
                return ShapeData.EMPTY;
            }
            Object boundsAabb = vsBounds.invoke(shape);
            double[] bounds = readAabb(boundsAabb);
            List<?> aabbs = (List<?>) vsToAabbs.invoke(shape);
            if (aabbs.isEmpty()) {
                throw new IllegalStateException("Non-empty VoxelShape produced zero AABBs");
            }
            double[] boxes = new double[aabbs.size() * 6];
            for (int i = 0; i < aabbs.size(); i++) {
                Object a = aabbs.get(i);
                int off = i * 6;
                boxes[off] = aabbMinX.getDouble(a);
                boxes[off + 1] = aabbMinY.getDouble(a);
                boxes[off + 2] = aabbMinZ.getDouble(a);
                boxes[off + 3] = aabbMaxX.getDouble(a);
                boxes[off + 4] = aabbMaxY.getDouble(a);
                boxes[off + 5] = aabbMaxZ.getDouble(a);
            }
            return new ShapeData(bounds, boxes);
        }

        private double[] readAabb(Object a) throws Exception {
            return new double[] {
                aabbMinX.getDouble(a),
                aabbMinY.getDouble(a),
                aabbMinZ.getDouble(a),
                aabbMaxX.getDouble(a),
                aabbMaxY.getDouble(a),
                aabbMaxZ.getDouble(a),
            };
        }
    }
}
