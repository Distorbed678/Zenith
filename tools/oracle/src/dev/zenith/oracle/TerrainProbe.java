// Zenith parity oracle — original source, MIT OR Apache-2.0.
package dev.zenith.oracle;

import java.io.BufferedInputStream;
import java.io.BufferedOutputStream;
import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.io.EOFException;
import java.io.IOException;
import java.io.StringReader;
import java.lang.reflect.Constructor;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Optional;
import java.util.stream.Collectors;
import java.util.stream.Stream;

/**
 * Reference probe for milestone M7 (terrain generation: density functions, climate, biomes, and base columns).
 *
 * <p>Invocation: {@code TerrainProbe <input-file> <output-file> <mode>}, where {@code <mode>} is one of
 * {@code router}, {@code router-volume}, {@code climate}, {@code biome}, or {@code column}.
 * Exact big-endian wire formats are specified in {@code docs/specs/parity/terrain-probe.md}.
 *
 * <p>Game classes are accessed solely through reflection by their public names so this tool compiles against the
 * JDK alone.
 */
public final class TerrainProbe {
    private static final byte[] MAGIC = {'Z', 'T', 'P', '1'};
    private static final int MODE_ROUTER = 1;
    private static final int MODE_ROUTER_VOLUME = 2;
    private static final int MODE_CLIMATE = 3;
    private static final int MODE_BIOME = 4;
    private static final int MODE_COLUMN = 5;

    private static final int IO_BUFFER_BYTES = 64 * 1024;

    private TerrainProbe() {}

    public static void main(String[] args) throws Exception {
        if (args.length != 3) {
            System.err.println("usage: TerrainProbe <input-file> <output-file> <mode>");
            System.err.println("modes: router | router-volume | climate | biome | column");
            System.exit(2);
        }
        Path inputPath = Path.of(args[0]);
        Path outputPath = Path.of(args[1]);
        String mode = args[2];

        try {
            switch (mode) {
                case "router", "router-volume", "climate", "biome", "column" -> {}
                default -> throw new IllegalArgumentException(
                        "Unknown mode '" + mode + "'; expected one of: router, router-volume, climate, biome, column");
            }

            // Bootstrap static registries and built-in world registries.
            Class.forName("net.minecraft.SharedConstants").getMethod("tryDetectVersion").invoke(null);
            Class.forName("net.minecraft.server.Bootstrap").getMethod("bootStrap").invoke(null);
            if (mode.equals("column")) {
                bindVanillaStaticTags();
            }
            Object worldLookup = Class.forName("net.minecraft.data.registries.VanillaRegistries")
                    .getMethod("createWorldLookup")
                    .invoke(null);

            GameApi api = new GameApi(worldLookup);

            Path parent = outputPath.toAbsolutePath().getParent();
            if (parent != null) {
                Files.createDirectories(parent);
            }

            try (DataInputStream in = new DataInputStream(
                         new BufferedInputStream(Files.newInputStream(inputPath), IO_BUFFER_BYTES));
                 DataOutputStream out = new DataOutputStream(
                         new BufferedOutputStream(Files.newOutputStream(outputPath), IO_BUFFER_BYTES))) {
                switch (mode) {
                    case "router" -> runRouter(api, in, out);
                    case "router-volume" -> runRouterVolume(api, in, out);
                    case "climate" -> runClimate(api, in, out);
                    case "biome" -> runBiome(api, in, out);
                    case "column" -> runColumn(api, in, out);
                    default -> throw new AssertionError(mode);
                }
                ensureEof(in);
            }
        } catch (InvocationTargetException e) {
            Throwable cause = e.getCause() != null ? e.getCause() : e;
            System.err.println("[zenith-oracle] terrain probe failed: " + cause);
            cause.printStackTrace(System.err);
            System.exit(1);
        } catch (IllegalArgumentException e) {
            System.err.println("[zenith-oracle] terrain probe error: " + e.getMessage());
            System.exit(1);
        }
    }

    /**
     * Loads and applies built-in server-data tags onto static registries (in particular {@code BuiltInRegistries.BLOCK})
     * so heightmap predicates such as {@code #minecraft:blocks_motion_in_heightmap} match a running server.
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

    // ── Mode 1: router ───────────────────────────────────────────────────────

    private static void runRouter(GameApi api, DataInputStream in, DataOutputStream out) throws Exception {
        long seed = in.readLong();
        String settingsId = readStr(in);
        int pointCount = readCount(in, "point_count");

        Object settingsHolder = api.getNoiseSettingsHolder(settingsId);
        Object settings = api.holderValue(settingsHolder);
        Object randomState = api.createRandomState(seed, settings);

        Object router = api.ngsNoiseRouter.invoke(settings);
        Object[] routerFns = {
            api.nrTemperature.invoke(router),
            api.nrVegetation.invoke(router),
            api.nrContinents.invoke(router),
            api.nrErosion.invoke(router),
            api.nrDepth.invoke(router),
            api.nrRidges.invoke(router),
            api.nrChunkSurfaceLevel.invoke(router),
            api.nrFinalDensity.invoke(router),
        };

        Optional<?> aquifersOpt = (Optional<?>) api.ngsAquifers.invoke(settings);
        boolean hasAquifers = aquifersOpt.isPresent();
        Object[] aquiferFns = null;
        if (hasAquifers) {
            Object cfg = aquifersOpt.get();
            aquiferFns = new Object[] {
                api.aqBarrier.invoke(cfg),
                api.aqFloodedness.invoke(cfg),
                api.aqSpread.invoke(cfg),
                api.aqLava.invoke(cfg),
                api.aqExclusion.invoke(cfg),
                api.aqSurfaceLevel.invoke(cfg),
            };
        }

        out.write(MAGIC);
        out.writeByte(MODE_ROUTER);
        out.writeByte(hasAquifers ? 1 : 0);
        out.writeInt(pointCount);

        for (int i = 0; i < pointCount; i++) {
            int x = in.readInt();
            int y = in.readInt();
            int z = in.readInt();
            for (Object fn : routerFns) {
                float v = (Float) api.rsSampleUncached.invoke(randomState, fn, x, y, z);
                out.writeInt(Float.floatToRawIntBits(v));
            }
            if (hasAquifers) {
                for (Object fn : aquiferFns) {
                    float v = (Float) api.rsSampleUncached.invoke(randomState, fn, x, y, z);
                    out.writeInt(Float.floatToRawIntBits(v));
                }
            }
        }
        System.out.println("[zenith-oracle] terrain router (" + settingsId + "): " + pointCount + " points");
    }

    // ── Mode 2: router-volume ────────────────────────────────────────────────

    private static void runRouterVolume(GameApi api, DataInputStream in, DataOutputStream out) throws Exception {
        long seed = in.readLong();
        String settingsId = readStr(in);
        int chunkCount = readCount(in, "chunk_count");

        Object settingsHolder = api.getNoiseSettingsHolder(settingsId);
        Object settings = api.holderValue(settingsHolder);
        Object randomState = api.createRandomState(seed, settings);

        Object noiseSettings = api.ngsNoiseSettings.invoke(settings);
        int minY = (Integer) api.nsMinY.invoke(noiseSettings);
        int height = (Integer) api.nsHeight.invoke(noiseSettings);
        if (height <= 0 || (height & 3) != 0) {
            throw new IllegalArgumentException("Unsupported noise_settings height " + height + " for " + settingsId);
        }
        int quartHeight = height >> 2;

        Object router = api.ngsNoiseRouter.invoke(settings);
        Object finalDensityFn = api.nrFinalDensity.invoke(router);
        Object finalSampler = api.rsGetSampler.invoke(randomState, finalDensityFn);

        int finalLen = 16 * height * 16;
        int climateLen = 4 * quartHeight * 4;
        Object finalBuf = api.dbCreateUnpooled.invoke(null, finalLen);
        Object climateBuf = api.dbCreateUnpooled.invoke(null, climateLen);

        out.write(MAGIC);
        out.writeByte(MODE_ROUTER_VOLUME);
        out.writeInt(minY);
        out.writeInt(height);
        out.writeInt(16);
        out.writeInt(height);
        out.writeInt(16);
        out.writeInt(4);
        out.writeInt(quartHeight);
        out.writeInt(4);
        out.writeInt(chunkCount);

        for (int c = 0; c < chunkCount; c++) {
            int chunkX = in.readInt();
            int chunkZ = in.readInt();
            int minBlockX = chunkX << 4;
            int minBlockZ = chunkZ << 4;

            // 1. final_density over 16 x height x 16 (stride 1,1,1) with fresh per-chunk caching context.
            Object finalVol = api.dvUnitCtor.newInstance(16, height, 16, minBlockX, minY, minBlockZ);
            Object finalCtx = api.newCachingSamplerContext();
            api.dsSampleVolume.invoke(finalSampler, finalCtx, finalBuf, finalVol);
            writeDensityBuffer(api, finalBuf, finalLen, out);

            // 2. Climate channels over 4 x (height/4) x 4 (stride 4,4,4) with fresh per-chunk caching context,
            // matching MultiNoiseBiomeSource.createResolverForChunk evaluation order.
            Object climateVol = api.dvStridedCtor.newInstance(4, quartHeight, 4, minBlockX, minY, minBlockZ, 4, 4, 4);
            Object climateCtx = api.newCachingSamplerContext();
            Object climateSampler = api.rsCreateClimateSampler.invoke(randomState, climateCtx);
            Object[] climateChannels = {
                api.csTemperature.invoke(climateSampler),
                api.csHumidity.invoke(climateSampler),
                api.csContinentalness.invoke(climateSampler),
                api.csErosion.invoke(climateSampler),
                api.csDepth.invoke(climateSampler),
                api.csWeirdness.invoke(climateSampler),
            };
            for (Object bound : climateChannels) {
                api.dsBoundSampleVolume.invoke(bound, climateBuf, climateVol);
                writeDensityBuffer(api, climateBuf, climateLen, out);
            }
        }
        System.out.println("[zenith-oracle] terrain router-volume (" + settingsId + "): " + chunkCount + " chunks");
    }

    private static void writeDensityBuffer(GameApi api, Object buf, int len, DataOutputStream out) throws Exception {
        for (int i = 0; i < len; i++) {
            float v = (Float) api.dbGet.invoke(buf, i);
            out.writeInt(Float.floatToRawIntBits(v));
        }
    }

    // ── Mode 3: climate ──────────────────────────────────────────────────────

    private static void runClimate(GameApi api, DataInputStream in, DataOutputStream out) throws Exception {
        long seed = in.readLong();
        String settingsId = readStr(in);
        int quartCount = readCount(in, "quart_count");

        Object settingsHolder = api.getNoiseSettingsHolder(settingsId);
        Object settings = api.holderValue(settingsHolder);
        Object randomState = api.createRandomState(seed, settings);
        Object climateSampler = api.rsCreateClimateSampler.invoke(randomState, api.scEmptyUncached);

        out.write(MAGIC);
        out.writeByte(MODE_CLIMATE);
        out.writeInt(quartCount);

        for (int i = 0; i < quartCount; i++) {
            int qx = in.readInt();
            int qy = in.readInt();
            int qz = in.readInt();
            Object tp = api.csSample.invoke(climateSampler, qx, qy, qz);
            out.writeLong((Long) api.tpTemperature.invoke(tp));
            out.writeLong((Long) api.tpHumidity.invoke(tp));
            out.writeLong((Long) api.tpContinentalness.invoke(tp));
            out.writeLong((Long) api.tpErosion.invoke(tp));
            out.writeLong((Long) api.tpDepth.invoke(tp));
            out.writeLong((Long) api.tpWeirdness.invoke(tp));
        }
        System.out.println("[zenith-oracle] terrain climate (" + settingsId + "): " + quartCount + " quarts");
    }

    // ── Mode 4: biome ────────────────────────────────────────────────────────

    private static void runBiome(GameApi api, DataInputStream in, DataOutputStream out) throws Exception {
        long seed = in.readLong();
        String selector = readStr(in);

        Object biomeSource;
        Object noiseSettings;
        if (selector.equals("json") || selector.startsWith("json:")) {
            byte[] jsonBytes = readBytes(in, "biome_source_json");
            String settingsId = selector.equals("json")
                    ? "minecraft:overworld"
                    : selector.substring("json:".length());
            if (settingsId.isEmpty()) {
                throw new IllegalArgumentException("Empty noise_settings id in selector '" + selector + "'");
            }
            biomeSource = api.decodeBiomeSourceJson(new String(jsonBytes, StandardCharsets.UTF_8));
            noiseSettings = api.holderValue(api.getNoiseSettingsHolder(settingsId));
        } else {
            Object[] pair = api.resolvePresetBiomeSourceAndSettings(selector);
            biomeSource = pair[0];
            noiseSettings = pair[1];
        }

        Object randomState = api.createRandomState(seed, noiseSettings);
        Object resolver = api.bsCreateUncachedResolver.invoke(biomeSource, randomState);
        long obfuscatedSeed = (Long) api.bmObfuscateSeed.invoke(null, seed);
        Object biomeManager = api.bmCtor.newInstance(resolver, obfuscatedSeed);

        int quartCount = readCount(in, "quart_count");
        out.write(MAGIC);
        out.writeByte(MODE_BIOME);
        out.writeLong(obfuscatedSeed);
        out.writeInt(quartCount);

        for (int i = 0; i < quartCount; i++) {
            int qx = in.readInt();
            int qy = in.readInt();
            int qz = in.readInt();
            Object biomeHolder = api.brGetNoiseBiome.invoke(resolver, qx, qy, qz);
            writeStr(out, api.holderRegisteredName(biomeHolder));
        }

        int blockCount = readCount(in, "block_count");
        out.writeInt(blockCount);
        for (int i = 0; i < blockCount; i++) {
            int bx = in.readInt();
            int by = in.readInt();
            int bz = in.readInt();
            Object biomeHolder = api.bmGetBiome.invoke(biomeManager, bx, by, bz);
            writeStr(out, api.holderRegisteredName(biomeHolder));
        }
        System.out.println("[zenith-oracle] terrain biome (" + selector + "): "
                + quartCount + " quarts, " + blockCount + " blocks");
    }

    // ── Mode 5: column ───────────────────────────────────────────────────────

    private static void runColumn(GameApi api, DataInputStream in, DataOutputStream out) throws Exception {
        long seed = in.readLong();
        String settingsId = readStr(in);
        int columnCount = readCount(in, "column_count");

        Object settingsHolder = api.getNoiseSettingsHolder(settingsId);
        Object settings = api.holderValue(settingsHolder);
        Object randomState = api.createRandomState(seed, settings);

        Object noiseSettings = api.ngsNoiseSettings.invoke(settings);
        int minY = (Integer) api.nsMinY.invoke(noiseSettings);
        int height = (Integer) api.nsHeight.invoke(noiseSettings);

        Object biomeSource = api.matchingBiomeSourceForSettings(settingsHolder);
        Object generator = api.nbcgCtor.newInstance(biomeSource, settingsHolder);
        Object heightAccessor = api.lhaCreate.invoke(null, minY, height);

        out.write(MAGIC);
        out.writeByte(MODE_COLUMN);
        out.writeInt(minY);
        out.writeInt(height);
        out.writeByte(api.heightmapTypes.length);
        out.writeInt(columnCount);

        for (int i = 0; i < columnCount; i++) {
            int x = in.readInt();
            int z = in.readInt();
            for (Object hmType : api.heightmapTypes) {
                int h = (Integer) api.nbcgGetBaseHeight.invoke(generator, x, z, hmType, heightAccessor, randomState);
                out.writeInt(h);
            }
            Object noiseColumn = api.nbcgGetBaseColumn.invoke(generator, x, z, heightAccessor, randomState);
            for (int iy = 0; iy < height; iy++) {
                Object blockState = api.ncGetBlock.invoke(noiseColumn, minY + iy);
                int stateId = (Integer) api.blockGetId.invoke(null, blockState);
                out.writeInt(stateId);
            }
        }
        System.out.println("[zenith-oracle] terrain column (" + settingsId + "): " + columnCount + " columns");
    }

    // ── Binary I/O helpers ───────────────────────────────────────────────────

    private static int readCount(DataInputStream in, String label) throws IOException {
        int n = in.readInt();
        if (n < 0) {
            throw new IllegalArgumentException("Negative or out-of-range " + label + ": " + Integer.toUnsignedString(n));
        }
        return n;
    }

    private static byte[] readExact(DataInputStream in, int len) throws IOException {
        byte[] buf = new byte[len];
        in.readFully(buf);
        return buf;
    }

    private static String readStr(DataInputStream in) throws IOException {
        int len = in.readUnsignedShort();
        return new String(readExact(in, len), StandardCharsets.UTF_8);
    }

    private static byte[] readBytes(DataInputStream in, String label) throws IOException {
        int len = readCount(in, label);
        return readExact(in, len);
    }

    private static void writeStr(DataOutputStream out, String s) throws IOException {
        byte[] bytes = s.getBytes(StandardCharsets.UTF_8);
        if (bytes.length > 0xFFFF) {
            throw new IllegalArgumentException("String exceeds u16 byte length: " + bytes.length);
        }
        out.writeShort(bytes.length);
        out.write(bytes);
    }

    private static void ensureEof(DataInputStream in) throws IOException {
        int next;
        try {
            next = in.read();
        } catch (EOFException e) {
            return;
        }
        if (next != -1) {
            throw new IllegalArgumentException("Unexpected trailing bytes in input file");
        }
    }

    // ── Reflective game API facade ───────────────────────────────────────────

    private static final class GameApi {
        final Object worldLookup;
        final Object noiseRegistry;
        final Object noiseSettingsRegistry;
        final Object worldPresetRegistry;

        final Object rkNoiseSettings;
        final Object rkWorldPreset;
        final Object stemOverworld;
        final Object stemNether;
        final Object stemEnd;

        final Method idTryParse;
        final Method rkCreate;
        final Method rkIdentifier;
        final Method hgGet;
        final Method hlListElementIds;
        final Method holderValue;
        final Method holderRegisteredName;
        final Method holderUnwrapKey;

        final Method ngsNoiseSettings;
        final Method ngsNoiseRouter;
        final Method ngsAquifers;
        final Method nsMinY;
        final Method nsHeight;

        final Method nrTemperature;
        final Method nrVegetation;
        final Method nrContinents;
        final Method nrErosion;
        final Method nrDepth;
        final Method nrRidges;
        final Method nrChunkSurfaceLevel;
        final Method nrFinalDensity;

        final Method aqBarrier;
        final Method aqFloodedness;
        final Method aqSpread;
        final Method aqLava;
        final Method aqExclusion;
        final Method aqSurfaceLevel;

        final Method rsCreate;
        final Method rsSampleUncached;
        final Method rsGetSampler;
        final Method rsCreateClimateSampler;

        final Object scEmptyUncached;
        final Method scBuilder;
        final Method scBuilderEnableCaches;
        final Method scBuilderBuild;

        final Constructor<?> dvUnitCtor;
        final Constructor<?> dvStridedCtor;
        final Method dbCreateUnpooled;
        final Method dbGet;
        final Method dsSampleVolume;
        final Method dsBoundSampleVolume;

        final Method csTemperature;
        final Method csHumidity;
        final Method csContinentalness;
        final Method csErosion;
        final Method csDepth;
        final Method csWeirdness;
        final Method csSample;

        final Method tpTemperature;
        final Method tpHumidity;
        final Method tpContinentalness;
        final Method tpErosion;
        final Method tpDepth;
        final Method tpWeirdness;

        final Method wpCreateWorldDimensions;
        final Method wdGet;
        final Method lsGenerator;
        final Method cgGetBiomeSource;
        final Method nbcgGeneratorSettings;

        final Method bsCreateUncachedResolver;
        final Method brGetNoiseBiome;
        final Method bmObfuscateSeed;
        final Constructor<?> bmCtor;
        final Method bmGetBiome;

        final Object biomeSourceCodec;
        final Object jsonOps;
        final Method strictJsonParse;
        final Method decoderParse;
        final Method dataResultResult;

        final Constructor<?> nbcgCtor;
        final Method nbcgGetBaseHeight;
        final Method nbcgGetBaseColumn;
        final Method lhaCreate;
        final Method ncGetBlock;
        final Method blockGetId;
        final Object[] heightmapTypes;

        GameApi(Object worldLookup) throws Exception {
            this.worldLookup = worldLookup;

            Class<?> idClass = Class.forName("net.minecraft.resources.Identifier");
            Class<?> rkClass = Class.forName("net.minecraft.resources.ResourceKey");
            Class<?> registriesClass = Class.forName("net.minecraft.core.registries.Registries");
            Class<?> hlProviderClass = Class.forName("net.minecraft.core.HolderLookup$Provider");
            Class<?> hlClass = Class.forName("net.minecraft.core.HolderLookup");
            Class<?> hgClass = Class.forName("net.minecraft.core.HolderGetter");
            Class<?> holderClass = Class.forName("net.minecraft.core.Holder");

            this.idTryParse = idClass.getMethod("tryParse", String.class);
            this.rkCreate = rkClass.getMethod("create", rkClass, idClass);
            this.rkIdentifier = rkClass.getMethod("identifier");
            this.hgGet = hgClass.getMethod("get", rkClass);
            this.hlListElementIds = hlClass.getMethod("listElementIds");
            this.holderValue = holderClass.getMethod("value");
            this.holderRegisteredName = holderClass.getMethod("getRegisteredName");
            this.holderUnwrapKey = holderClass.getMethod("unwrapKey");

            Object rkNoise = registriesClass.getField("NOISE").get(null);
            this.rkNoiseSettings = registriesClass.getField("NOISE_SETTINGS").get(null);
            this.rkWorldPreset = registriesClass.getField("WORLD_PRESET").get(null);

            Method lookupOrThrow = hlProviderClass.getMethod("lookupOrThrow", rkClass);
            this.noiseRegistry = lookupOrThrow.invoke(worldLookup, rkNoise);
            this.noiseSettingsRegistry = lookupOrThrow.invoke(worldLookup, this.rkNoiseSettings);
            this.worldPresetRegistry = lookupOrThrow.invoke(worldLookup, this.rkWorldPreset);

            Class<?> levelStemClass = Class.forName("net.minecraft.world.level.dimension.LevelStem");
            this.stemOverworld = levelStemClass.getField("OVERWORLD").get(null);
            this.stemNether = levelStemClass.getField("NETHER").get(null);
            this.stemEnd = levelStemClass.getField("END").get(null);
            this.lsGenerator = levelStemClass.getMethod("generator");

            Class<?> ngsClass = Class.forName("net.minecraft.world.level.levelgen.NoiseGeneratorSettings");
            this.ngsNoiseSettings = ngsClass.getMethod("noiseSettings");
            this.ngsNoiseRouter = ngsClass.getMethod("noiseRouter");
            this.ngsAquifers = ngsClass.getMethod("aquifers");

            Class<?> nsClass = Class.forName("net.minecraft.world.level.levelgen.NoiseSettings");
            this.nsMinY = nsClass.getMethod("minY");
            this.nsHeight = nsClass.getMethod("height");

            Class<?> nrClass = Class.forName("net.minecraft.world.level.levelgen.NoiseRouter");
            this.nrTemperature = nrClass.getMethod("temperature");
            this.nrVegetation = nrClass.getMethod("vegetation");
            this.nrContinents = nrClass.getMethod("continents");
            this.nrErosion = nrClass.getMethod("erosion");
            this.nrDepth = nrClass.getMethod("depth");
            this.nrRidges = nrClass.getMethod("ridges");
            this.nrChunkSurfaceLevel = nrClass.getMethod("chunkSurfaceLevel");
            this.nrFinalDensity = nrClass.getMethod("finalDensity");

            Class<?> aqConfigClass = Class.forName("net.minecraft.world.level.levelgen.Aquifer$Config");
            this.aqBarrier = aqConfigClass.getMethod("barrierNoise");
            this.aqFloodedness = aqConfigClass.getMethod("fluidLevelFloodednessNoise");
            this.aqSpread = aqConfigClass.getMethod("fluidLevelSpreadNoise");
            this.aqLava = aqConfigClass.getMethod("lavaNoise");
            this.aqExclusion = aqConfigClass.getMethod("exclusion");
            this.aqSurfaceLevel = aqConfigClass.getMethod("surfaceLevel");

            Class<?> dfClass = Class.forName("net.minecraft.world.level.levelgen.densityfunction.DensityFunction");
            Class<?> scClass = Class.forName("net.minecraft.world.level.levelgen.densityfunction.SamplerContext");
            Class<?> scBuilderClass = Class.forName("net.minecraft.world.level.levelgen.densityfunction.SamplerContext$Builder");
            Class<?> rsClass = Class.forName("net.minecraft.world.level.levelgen.RandomState");

            this.rsCreate = rsClass.getMethod("create", hgClass, long.class, ngsClass);
            this.rsSampleUncached = rsClass.getMethod("sampleBlockValueUncached", dfClass, int.class, int.class, int.class);
            this.rsGetSampler = rsClass.getMethod("getSampler", dfClass);
            this.rsCreateClimateSampler = rsClass.getMethod("createClimateSampler", scClass);

            this.scEmptyUncached = scClass.getField("EMPTY_UNCACHED").get(null);
            this.scBuilder = scClass.getMethod("builder");
            this.scBuilderEnableCaches = scBuilderClass.getMethod("enableCaches");
            this.scBuilderBuild = scBuilderClass.getMethod("build");

            Class<?> dvClass = Class.forName("net.minecraft.world.level.levelgen.densityfunction.DensityVolume");
            Class<?> dbClass = Class.forName("net.minecraft.world.level.levelgen.densityfunction.DensityBuffer");
            Class<?> dsClass = Class.forName("net.minecraft.world.level.levelgen.densityfunction.DensitySampler");
            Class<?> dsBoundClass = Class.forName("net.minecraft.world.level.levelgen.densityfunction.DensitySampler$Bound");

            this.dvUnitCtor = dvClass.getConstructor(int.class, int.class, int.class, int.class, int.class, int.class);
            this.dvStridedCtor = dvClass.getConstructor(
                    int.class, int.class, int.class, int.class, int.class, int.class, int.class, int.class, int.class);
            this.dbCreateUnpooled = dbClass.getMethod("createUnpooled", int.class);
            this.dbGet = dbClass.getMethod("get", int.class);
            this.dsSampleVolume = dsClass.getMethod("sampleVolume", scClass, dbClass, dvClass);
            this.dsBoundSampleVolume = dsBoundClass.getMethod("sampleVolume", dbClass, dvClass);

            Class<?> csClass = Class.forName("net.minecraft.world.level.biome.Climate$Sampler");
            this.csTemperature = csClass.getMethod("temperature");
            this.csHumidity = csClass.getMethod("humidity");
            this.csContinentalness = csClass.getMethod("continentalness");
            this.csErosion = csClass.getMethod("erosion");
            this.csDepth = csClass.getMethod("depth");
            this.csWeirdness = csClass.getMethod("weirdness");
            this.csSample = csClass.getMethod("sample", int.class, int.class, int.class);

            Class<?> tpClass = Class.forName("net.minecraft.world.level.biome.Climate$TargetPoint");
            this.tpTemperature = tpClass.getMethod("temperature");
            this.tpHumidity = tpClass.getMethod("humidity");
            this.tpContinentalness = tpClass.getMethod("continentalness");
            this.tpErosion = tpClass.getMethod("erosion");
            this.tpDepth = tpClass.getMethod("depth");
            this.tpWeirdness = tpClass.getMethod("weirdness");

            Class<?> wpClass = Class.forName("net.minecraft.world.level.levelgen.presets.WorldPreset");
            Class<?> wdClass = Class.forName("net.minecraft.world.level.levelgen.WorldDimensions");
            Class<?> cgClass = Class.forName("net.minecraft.world.level.chunk.ChunkGenerator");
            Class<?> nbcgClass = Class.forName("net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator");
            Class<?> bsClass = Class.forName("net.minecraft.world.level.biome.BiomeSource");
            Class<?> brClass = Class.forName("net.minecraft.world.level.biome.BiomeResolver");
            Class<?> bmClass = Class.forName("net.minecraft.world.level.biome.BiomeManager");

            this.wpCreateWorldDimensions = wpClass.getMethod("createWorldDimensions");
            this.wdGet = wdClass.getMethod("get", rkClass);
            this.cgGetBiomeSource = cgClass.getMethod("getBiomeSource");
            this.nbcgGeneratorSettings = nbcgClass.getMethod("generatorSettings");

            this.bsCreateUncachedResolver = bsClass.getMethod("createUncachedResolver", rsClass);
            this.brGetNoiseBiome = brClass.getMethod("getNoiseBiome", int.class, int.class, int.class);
            this.bmObfuscateSeed = bmClass.getMethod("obfuscateSeed", long.class);
            this.bmCtor = bmClass.getConstructor(brClass, long.class);
            this.bmGetBiome = bmClass.getMethod("getBiome", int.class, int.class, int.class);

            Class<?> dynamicOpsClass = Class.forName("com.mojang.serialization.DynamicOps");
            this.biomeSourceCodec = bsClass.getField("CODEC").get(null);
            Object rawJsonOps = Class.forName("com.mojang.serialization.JsonOps").getField("INSTANCE").get(null);
            this.jsonOps = hlProviderClass.getMethod("createSerializationContext", dynamicOpsClass)
                    .invoke(worldLookup, rawJsonOps);
            this.strictJsonParse = Class.forName("net.minecraft.util.StrictJsonParser")
                    .getMethod("parse", java.io.Reader.class);
            this.decoderParse = Class.forName("com.mojang.serialization.Decoder")
                    .getMethod("parse", dynamicOpsClass, Object.class);
            this.dataResultResult = Class.forName("com.mojang.serialization.DataResult").getMethod("result");

            Class<?> lhaClass = Class.forName("net.minecraft.world.level.LevelHeightAccessor");
            Class<?> hmTypesClass = Class.forName("net.minecraft.world.level.levelgen.Heightmap$Types");
            Class<?> ncClass = Class.forName("net.minecraft.world.level.NoiseColumn");
            Class<?> blockClass = Class.forName("net.minecraft.world.level.block.Block");
            Class<?> blockStateClass = Class.forName("net.minecraft.world.level.block.state.BlockState");

            this.nbcgCtor = nbcgClass.getConstructor(bsClass, holderClass);
            this.nbcgGetBaseHeight = nbcgClass.getMethod(
                    "getBaseHeight", int.class, int.class, hmTypesClass, lhaClass, rsClass);
            this.nbcgGetBaseColumn = nbcgClass.getMethod(
                    "getBaseColumn", int.class, int.class, lhaClass, rsClass);
            this.lhaCreate = lhaClass.getMethod("create", int.class, int.class);
            this.ncGetBlock = ncClass.getMethod("getBlock", int.class);
            this.blockGetId = blockClass.getMethod("getId", blockStateClass);
            this.heightmapTypes = new Object[] {
                hmTypesClass.getField("OCEAN_FLOOR_WG").get(null),
                hmTypesClass.getField("WORLD_SURFACE_WG").get(null),
                hmTypesClass.getField("MOTION_BLOCKING").get(null),
                hmTypesClass.getField("MOTION_BLOCKING_NO_LEAVES").get(null),
            };
        }

        Object parseIdentifier(String raw, String kind) throws Exception {
            Object id = idTryParse.invoke(null, raw);
            if (id == null) {
                throw new IllegalArgumentException("Invalid " + kind + " identifier: '" + raw + "'");
            }
            return id;
        }

        Object getNoiseSettingsHolder(String rawId) throws Exception {
            Object id = parseIdentifier(rawId, "noise_settings");
            Object key = rkCreate.invoke(null, rkNoiseSettings, id);
            Optional<?> found = (Optional<?>) hgGet.invoke(noiseSettingsRegistry, key);
            if (found.isEmpty()) {
                throw new IllegalArgumentException(
                        "Unknown noise_settings id '" + rawId + "'; available: " + listIds(noiseSettingsRegistry));
            }
            return found.get();
        }

        String listIds(Object registryLookup) throws Exception {
            Stream<?> ids = (Stream<?>) hlListElementIds.invoke(registryLookup);
            return ids.map(k -> {
                        try {
                            return rkIdentifier.invoke(k).toString();
                        } catch (Exception e) {
                            return k.toString();
                        }
                    })
                    .sorted()
                    .collect(Collectors.joining(", "));
        }

        Object holderValue(Object holder) throws Exception {
            return holderValue.invoke(holder);
        }

        String holderRegisteredName(Object holder) throws Exception {
            return (String) holderRegisteredName.invoke(holder);
        }

        Object createRandomState(long seed, Object settings) throws Exception {
            return rsCreate.invoke(null, noiseRegistry, seed, settings);
        }

        Object newCachingSamplerContext() throws Exception {
            Object builder = scBuilder.invoke(null);
            scBuilderEnableCaches.invoke(builder);
            return scBuilderBuild.invoke(builder);
        }

        Object getPresetStemGenerator(String presetId, Object stemKey) throws Exception {
            Object id = parseIdentifier(presetId, "world_preset");
            Object key = rkCreate.invoke(null, rkWorldPreset, id);
            Optional<?> found = (Optional<?>) hgGet.invoke(worldPresetRegistry, key);
            if (found.isEmpty()) {
                throw new IllegalArgumentException("Unknown world_preset id '" + presetId + "'");
            }
            Object preset = holderValue(found.get());
            Object dims = wpCreateWorldDimensions.invoke(preset);
            Optional<?> stemOpt = (Optional<?>) wdGet.invoke(dims, stemKey);
            if (stemOpt.isEmpty()) {
                throw new IllegalArgumentException("WorldPreset '" + presetId + "' has no dimension " + stemKey);
            }
            return lsGenerator.invoke(stemOpt.get());
        }

        /** Returns {BiomeSource, NoiseGeneratorSettings} for a named biome selector. */
        Object[] resolvePresetBiomeSourceAndSettings(String selector) throws Exception {
            String presetId;
            Object stemKey;
            switch (selector) {
                case "overworld", "minecraft:overworld", "normal", "minecraft:normal" -> {
                    presetId = "minecraft:normal";
                    stemKey = stemOverworld;
                }
                case "large_biomes", "minecraft:large_biomes" -> {
                    presetId = "minecraft:large_biomes";
                    stemKey = stemOverworld;
                }
                case "amplified", "minecraft:amplified" -> {
                    presetId = "minecraft:amplified";
                    stemKey = stemOverworld;
                }
                case "nether", "minecraft:nether", "the_nether", "minecraft:the_nether" -> {
                    presetId = "minecraft:normal";
                    stemKey = stemNether;
                }
                case "end", "minecraft:end", "the_end", "minecraft:the_end" -> {
                    presetId = "minecraft:normal";
                    stemKey = stemEnd;
                }
                default -> throw new IllegalArgumentException(
                        "Unknown biome selector '" + selector
                                + "'; expected one of: overworld, large_biomes, amplified, nether, end, json, json:<noise_settings_id>");
            }
            Object gen = getPresetStemGenerator(presetId, stemKey);
            Object biomeSource = cgGetBiomeSource.invoke(gen);
            Object settingsHolder = nbcgGeneratorSettings.invoke(gen);
            Object settings = holderValue(settingsHolder);
            return new Object[] {biomeSource, settings};
        }

        Object decodeBiomeSourceJson(String json) throws Exception {
            Object tree;
            try {
                tree = strictJsonParse.invoke(null, new StringReader(json));
            } catch (InvocationTargetException e) {
                Throwable cause = e.getCause() != null ? e.getCause() : e;
                throw new IllegalArgumentException("Invalid biome source JSON: " + cause.getMessage());
            }
            Object dr = decoderParse.invoke(biomeSourceCodec, jsonOps, tree);
            Optional<?> decoded = (Optional<?>) dataResultResult.invoke(dr);
            if (decoded.isEmpty()) {
                throw new IllegalArgumentException("Failed to decode BiomeSource JSON: " + dr);
            }
            return decoded.get();
        }

        Object matchingBiomeSourceForSettings(Object settingsHolder) throws Exception {
            Optional<?> keyOpt = (Optional<?>) holderUnwrapKey.invoke(settingsHolder);
            String id = keyOpt.isPresent() ? rkIdentifier.invoke(keyOpt.get()).toString() : "minecraft:overworld";
            Object stemKey = switch (id) {
                case "minecraft:nether" -> stemNether;
                case "minecraft:end" -> stemEnd;
                default -> stemOverworld;
            };
            Object gen = getPresetStemGenerator("minecraft:normal", stemKey);
            return cgGetBiomeSource.invoke(gen);
        }
    }
}
