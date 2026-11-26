// Zenith parity oracle — original source, MIT OR Apache-2.0.
package dev.zenith.oracle;

import java.io.ByteArrayOutputStream;
import java.io.DataOutputStream;
import java.io.IOException;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Properties;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.TimeUnit;

/**
 * Tick-by-tick scenario runner (format and semantics: docs/specs/parity/scenarios.md).
 *
 * <p>Driven from the instrumented tick method: {@link #tickStart} runs on the server thread before the body of every
 * server tick, {@link #tickEnd} right before it returns. The first tick builds the world prerequisites (game rules,
 * forced chunk box); later ticks wait until the box is ticking, pin the clocks and random sources, and then walk the
 * timeline. When the timeline is over the output is written and the server is halted.
 */
final class ScenarioRunner {
    static final int TAG_DUMP = 1;
    static final int TAG_END = 0xFF;

    private enum Phase { INIT, LOADING, RUNNING, DONE }

    /** Longest wait for the chunk box to start ticking, in server ticks. */
    private static final int MAX_LOADING_TICKS = 2000;

    private static ScenarioRunner instance;

    private final Scenario scenario;
    private final Path outFile;
    private final Properties hooks;
    private final ByteArrayOutputStream outBytes = new ByteArrayOutputStream();
    private final DataOutputStream out = new DataOutputStream(outBytes);

    private Phase phase = Phase.INIT;
    private Object level;
    private int loadingTicks;
    private int timelineTick;
    private int nextOp;
    private boolean inTimelineTick;
    private boolean written;
    private boolean lightWarned;
    private List<int[]> lightChunks = new ArrayList<>();

    private ScenarioRunner(Scenario scenario, Path outFile, Properties hooks) {
        this.scenario = scenario;
        this.outFile = outFile;
        this.hooks = hooks;
    }

    static boolean active() {
        return instance != null;
    }

    static void configure(Path scenarioFile, Path outFile, Properties hooks) throws IOException {
        Scenario s = Scenario.read(scenarioFile);
        ScenarioRunner r = new ScenarioRunner(s, outFile, hooks);
        r.out.write("ZSO1".getBytes(StandardCharsets.US_ASCII));
        ScenarioDump.writeStr(r.out, s.dimension);
        r.out.writeInt(s.totalTicks);
        instance = r;
        // Never lose the output, even if the process is stopped from outside.
        Runtime.getRuntime().addShutdownHook(new Thread(() -> r.finish(null, 1, "server stopped early"), "zenith-scenario-out"));
        System.out.println("[zenith-oracle] scenario: " + s.ops.size() + " operations, " + s.totalTicks + " ticks in "
                + s.dimension + ", chunks " + s.chunkMinX + "," + s.chunkMinZ + ".." + s.chunkMaxX + "," + s.chunkMaxZ);
    }

    static void tickStart(Object server) {
        ScenarioRunner r = instance;
        if (r != null) {
            r.onTickStart(server);
        }
    }

    static void tickEnd(Object server) {
        ScenarioRunner r = instance;
        if (r != null) {
            r.onTickEnd(server);
        }
    }

    // ---- state machine --------------------------------------------------------------------------------------

    private void onTickStart(Object server) {
        try {
            switch (phase) {
                case INIT -> {
                    setup(server);
                    phase = Phase.LOADING;
                    loadingTicks = 0;
                }
                case LOADING -> {
                    if (boxReady()) {
                        beginTimeline(server);
                    } else if (++loadingTicks > MAX_LOADING_TICKS) {
                        throw new IllegalStateException("chunk box not ticking after " + MAX_LOADING_TICKS + " ticks");
                    }
                }
                default -> { }
            }
            if (phase == Phase.RUNNING) {
                syncLight();
                inTimelineTick = true;
                runOps(server, 0);
                syncLight();
            }
        } catch (Throwable t) {
            fail(server, t);
        }
    }

    private void onTickEnd(Object server) {
        if (phase != Phase.RUNNING || !inTimelineTick) {
            return;
        }
        try {
            inTimelineTick = false;
            syncLight();
            runOps(server, 1);
            timelineTick++;
            if (timelineTick >= scenario.totalTicks) {
                phase = Phase.DONE;
                finish(server, 0, "");
            }
        } catch (Throwable t) {
            fail(server, t);
        }
    }

    private void fail(Object server, Throwable t) {
        System.err.println("[zenith-oracle] scenario failed: " + t);
        t.printStackTrace();
        phase = Phase.DONE;
        finish(server, 1, String.valueOf(t));
    }

    private synchronized void finish(Object server, int status, String message) {
        if (written) {
            return;
        }
        written = true;
        try {
            out.writeByte(TAG_END);
            out.writeByte(status);
            ScenarioDump.writeStr(out, message);
            out.writeInt(timelineTick);
            if (outFile.getParent() != null) {
                Files.createDirectories(outFile.getParent());
            }
            Files.write(outFile, outBytes.toByteArray());
            System.out.println("[zenith-oracle] scenario " + (status == 0 ? "finished" : "ended with an error")
                    + " after " + timelineTick + " ticks, output " + outFile);
        } catch (IOException e) {
            System.err.println("[zenith-oracle] could not write scenario output: " + e);
        }
        if (server != null) {
            try {
                Method halt = server.getClass().getMethod(hooks.getProperty("server.halt"), boolean.class);
                halt.invoke(server, false);
            } catch (ReflectiveOperationException e) {
                System.err.println("[zenith-oracle] could not halt the server: " + e);
            }
        }
    }

    // ---- setup ----------------------------------------------------------------------------------------------

    private void setup(Object server) throws Exception {
        for (Object l : (Iterable<?>) ScenarioGame.call(server, "getAllLevels")) {
            if (ScenarioGame.dimensionId(l).equals(scenario.dimension)) {
                level = l;
            }
        }
        if (level == null) {
            throw new IllegalArgumentException("unknown dimension " + scenario.dimension);
        }
        if ((scenario.flags & Scenario.FLAG_FAST_TICKS) != 0) {
            // Only the wall-clock pacing changes; what a tick does does not depend on the tick rate.
            Object rates = ScenarioGame.call(server, "tickRateManager");
            ScenarioGame.method(rates.getClass(), "setTickRate", float.class).invoke(rates, 5000.0f);
        }
        for (String[] rule : scenario.gamerules) {
            ScenarioGame.setGameRule(server, rule[0], rule[1]);
        }
        // Forced chunks, added in a fixed order (x ascending, then z ascending) so the level's chunk iteration order
        // is reproducible. Each call also loads the chunk to FULL.
        Method forced = ScenarioGame.method(level.getClass(), "setChunkForced", int.class, int.class, boolean.class);
        for (int cx = scenario.chunkMinX; cx <= scenario.chunkMaxX; cx++) {
            for (int cz = scenario.chunkMinZ; cz <= scenario.chunkMaxZ; cz++) {
                forced.invoke(level, cx, cz, true);
            }
        }
        for (int cx = scenario.chunkMinX - 1; cx <= scenario.chunkMaxX + 1; cx++) {
            for (int cz = scenario.chunkMinZ - 1; cz <= scenario.chunkMaxZ + 1; cz++) {
                lightChunks.add(new int[] {cx, cz});
            }
        }
    }

    /** Ready when every box chunk is entity ticking and every chunk of the one-chunk ring around it block ticks. */
    private boolean boxReady() throws Exception {
        List<int[]> ticking = ScenarioDump.tickingChunks(level);
        Method posTicking = ScenarioGame.method(level.getClass(), "isPositionTickingWithEntitiesLoaded", long.class);
        Method pack = ScenarioGame.cls("net.minecraft.world.level.ChunkPos").getMethod("pack", int.class, int.class);
        for (int cx = scenario.chunkMinX - 1; cx <= scenario.chunkMaxX + 1; cx++) {
            for (int cz = scenario.chunkMinZ - 1; cz <= scenario.chunkMaxZ + 1; cz++) {
                boolean inBox = cx >= scenario.chunkMinX && cx <= scenario.chunkMaxX && cz >= scenario.chunkMinZ
                        && cz <= scenario.chunkMaxZ;
                if (inBox) {
                    boolean found = false;
                    for (int[] c : ticking) {
                        if (c[0] == cx && c[1] == cz) {
                            found = true;
                        }
                    }
                    if (!found) {
                        return false;
                    }
                }
                if (!(Boolean) posTicking.invoke(level, pack.invoke(null, cx, cz))) {
                    return false;
                }
            }
        }
        return true;
    }

    /** Pins game time, sub-tick counter and the random sources, then arms the timeline. */
    private void beginTimeline(Object server) throws Exception {
        syncLight();
        ScenarioGame.setOverworldGameTime(server, 0L);
        ScenarioGame.setSubTickCount(level, 0L);
        pinRng(scenario.rngMode, scenario.rngValue, scenario.hasRandValue, scenario.randValue);
        int leftover = (Integer) ScenarioGame.call(ScenarioGame.call(level, "getBlockTicks"), "count")
                + (Integer) ScenarioGame.call(ScenarioGame.call(level, "getFluidTicks"), "count");
        if (leftover != 0) {
            System.err.println("[zenith-oracle] warning: " + leftover + " scheduled ticks existed before the timeline");
        }
        phase = Phase.RUNNING;
        timelineTick = 0;
        nextOp = 0;
        System.out.println("[zenith-oracle] timeline armed after " + loadingTicks + " loading ticks");
    }

    private void pinRng(int mode, long value, boolean hasRand, int rand) throws Exception {
        Object random = ScenarioGame.levelRandom(level);
        if (mode == 1) {
            ScenarioGame.call(random, "setSeed", new Class<?>[] {long.class}, value);
        } else if (mode == 2) {
            ScenarioGame.setRandomState(random, value);
        }
        if (hasRand) {
            ScenarioGame.setRandValue(level, rand);
        }
    }

    // ---- operations -----------------------------------------------------------------------------------------

    private void runOps(Object server, int wantPhase) throws Exception {
        while (nextOp < scenario.ops.size()) {
            Scenario.Op op = scenario.ops.get(nextOp);
            if (op.tick < timelineTick || (op.tick == timelineTick && op.phase < wantPhase)) {
                throw new IllegalStateException("operation " + nextOp + " is behind the timeline (tick " + op.tick
                        + ", phase " + op.phase + ")");
            }
            if (op.tick != timelineTick || op.phase != wantPhase) {
                return;
            }
            apply(server, op);
            nextOp++;
        }
    }

    private void apply(Object server, Scenario.Op op) throws Exception {
        int[] a = op.ints;
        switch (op.code) {
            case Scenario.OP_SET_BLOCK -> setBlock(a[0], a[1], a[2], a[3], a[4]);
            case Scenario.OP_FILL -> {
                for (int y = Math.min(a[1], a[4]); y <= Math.max(a[1], a[4]); y++) {
                    for (int z = Math.min(a[2], a[5]); z <= Math.max(a[2], a[5]); z++) {
                        for (int x = Math.min(a[0], a[3]); x <= Math.max(a[0], a[3]); x++) {
                            setBlock(x, y, z, a[6], a[7]);
                        }
                    }
                }
            }
            case Scenario.OP_SCHEDULE_BLOCK -> schedule(a, ScenarioGame.blockRegistry(), ScenarioGame.BLOCK);
            case Scenario.OP_SCHEDULE_FLUID -> schedule(a, ScenarioGame.fluidRegistry(), ScenarioGame.FLUID);
            case Scenario.OP_BLOCK_EVENT -> ScenarioGame.call(level, "blockEvent",
                    new Class<?>[] {ScenarioGame.BLOCK_POS, ScenarioGame.BLOCK, int.class, int.class},
                    ScenarioGame.pos(a[0], a[1], a[2]), ScenarioGame.byId(ScenarioGame.blockRegistry(), a[3]), a[4], a[5]);
            case Scenario.OP_PIN_RNG -> pinRng(a[0], op.value, a[1] != 0, a[2]);
            case Scenario.OP_GAMERULE -> ScenarioGame.setGameRule(server, op.s1, op.s2);
            case Scenario.OP_DUMP -> {
                syncLight();
                ScenarioDump.write(out, server, level, timelineTick, op.phase, new int[] {a[0], a[1], a[2], a[3], a[4], a[5]}, a[6]);
            }
            default -> throw new IllegalStateException("unknown opcode " + op.code);
        }
    }

    private void setBlock(int x, int y, int z, int stateId, int flags) throws Exception {
        ScenarioGame.call(level, "setBlock", new Class<?>[] {ScenarioGame.BLOCK_POS, ScenarioGame.BLOCK_STATE, int.class},
                ScenarioGame.pos(x, y, z), ScenarioGame.stateById(stateId), flags);
    }

    private void schedule(int[] a, Object registry, Class<?> typeClass) throws Exception {
        ScenarioGame.call(level, "scheduleTick",
                new Class<?>[] {ScenarioGame.BLOCK_POS, typeClass, int.class, ScenarioGame.TICK_PRIORITY},
                ScenarioGame.pos(a[0], a[1], a[2]), ScenarioGame.byId(registry, a[3]), a[4], ScenarioGame.priority(a[5]));
    }

    // ---- light synchronisation ------------------------------------------------------------------------------

    /**
     * Vanilla applies light changes on its own thread some time after a block change. To make light-dependent
     * behaviour reproducible the runner waits until the light engine has processed everything queued for the box and
     * its ring, at every tick boundary and before every dump. (Changes made inside a tick body can still be seen by
     * later phases of that same tick at an arbitrary stage; scenarios must not depend on that.)
     */
    private void syncLight() {
        if ((scenario.flags & Scenario.FLAG_LIGHT_SYNC) == 0) {
            return;
        }
        try {
            Object engine = ScenarioGame.call(level, "getLightEngine");
            Method wait = ScenarioGame.method(engine.getClass(), "waitForPendingTasks", int.class, int.class);
            Method schedule = ScenarioGame.method(engine.getClass(), "tryScheduleUpdate");
            List<CompletableFuture<?>> futures = new ArrayList<>();
            for (int[] c : lightChunks) {
                futures.add((CompletableFuture<?>) wait.invoke(engine, c[0], c[1]));
            }
            long deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(10);
            for (CompletableFuture<?> f : futures) {
                while (!f.isDone()) {
                    schedule.invoke(engine);
                    try {
                        f.get(1, TimeUnit.MILLISECONDS);
                    } catch (java.util.concurrent.TimeoutException ignored) {
                        if (System.nanoTime() > deadline) {
                            throw new IllegalStateException("light engine did not settle within 10s");
                        }
                    }
                }
            }
        } catch (Exception e) {
            if (!lightWarned) {
                lightWarned = true;
                System.err.println("[zenith-oracle] light sync unavailable: " + e);
            }
        }
    }
}
