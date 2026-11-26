// Zenith parity oracle — original source, MIT OR Apache-2.0.
package dev.zenith.oracle;

import java.io.IOException;
import java.lang.reflect.Method;
import java.nio.file.Path;
import java.util.Properties;

/**
 * Called from the instrumented tick method. Reads public game state reflectively and writes ZPD1 records.
 * All methods swallow and report their own errors so the oracle can never change how the game runs.
 */
public final class Hooks {
    private static final short TICK_END = 1;

    private static DumpWriter writer;
    private static int tickLimit;
    private static Properties names;
    private static boolean halted;

    private static Path chunkRequests;
    private static Path chunkOut;
    private static int chunkExtras;

    private Hooks() {}

    static synchronized void configureChunks(Path requests, Path out, int extras) {
        chunkRequests = requests;
        chunkOut = out;
        chunkExtras = extras;
    }

    static synchronized void configure(Path out, int ticks, long seed, String version, String scenario, Properties hooks)
            throws IOException {
        writer = new DumpWriter(out, "vanilla", version, scenario, seed);
        tickLimit = ticks;
        names = hooks;
        Runtime.getRuntime().addShutdownHook(new Thread(Hooks::close, "zenith-oracle-close"));
    }

    public static void tickStart(Object server) {
        if (ScenarioRunner.active()) {
            ScenarioRunner.tickStart(server);
            return;
        }
        // Reserved for tick_start snapshots (scenario-driven); the baseline scenario records tick ends only.
    }

    public static synchronized void tickEnd(Object server) {
        if (ScenarioRunner.active()) {
            ScenarioRunner.tickEnd(server);
            return;
        }
        if (writer == null || halted) {
            return;
        }
        try {
            if (chunkRequests != null) {
                ChunkDump.run(server, chunkRequests, chunkOut, chunkExtras);
                chunkRequests = null;
                tickLimit = 0; // stop at the end of this tick
            }
            int tickCount = (Integer) call(server, names.getProperty("server.tickCount"));
            writer.record(tickCount, TICK_END, "server")
                    .i64("tickCount", tickCount)
                    .end();
            for (Object level : (Iterable<?>) call(server, names.getProperty("server.levels"))) {
                Object key = call(level, names.getProperty("level.dimension"));
                String id = String.valueOf(call(key, names.getProperty("key.id")));
                writer.record(tickCount, TICK_END, id)
                        .i64("gameTime", (Long) call(level, names.getProperty("level.gameTime")))
                        .i64("clockTime", (Long) call(level, names.getProperty("level.clockTime")))
                        .end();
            }
            if (tickCount >= tickLimit) {
                halted = true;
                close();
                System.out.println("[zenith-oracle] reached " + tickLimit + " ticks, stopping the server");
                Method halt = server.getClass().getMethod(names.getProperty("server.halt"), boolean.class);
                halt.invoke(server, false);
            }
        } catch (Throwable t) {
            System.err.println("[zenith-oracle] tickEnd failed: " + t);
            t.printStackTrace();
            halted = true;
            close();
        }
    }

    private static Object call(Object target, String method) throws ReflectiveOperationException {
        Method m = target.getClass().getMethod(method);
        return m.invoke(target);
    }

    static synchronized void close() {
        if (writer != null) {
            try {
                writer.close();
            } catch (IOException e) {
                System.err.println("[zenith-oracle] close failed: " + e);
            }
            writer = null;
        }
    }
}
