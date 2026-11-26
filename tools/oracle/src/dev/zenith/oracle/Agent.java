// Zenith parity oracle — original source, MIT OR Apache-2.0.
package dev.zenith.oracle;

import java.io.IOException;
import java.io.InputStream;
import java.lang.instrument.Instrumentation;
import java.nio.file.Path;
import java.util.HashMap;
import java.util.Map;
import java.util.Properties;

/**
 * Java agent entry point. Arguments: comma-separated {@code key=value} pairs:
 * {@code out} (dump path), {@code ticks} (stop after this many), {@code seed}, {@code version}, {@code scenario}, and
 * optionally {@code chunks} + {@code chunks_out} (dump requested chunks on the first tick, then stop; see
 * {@link ChunkDump}). With {@code scenario=<file>,scenario_out=<file>} the agent runs a tick-by-tick scenario instead
 * (see {@link ScenarioRunner}); {@code out} and {@code ticks} are then not needed.
 */
public final class Agent {
    private Agent() {}

    public static void premain(String args, Instrumentation inst) throws IOException {
        Map<String, String> opts = parseArgs(args);
        String version = opts.getOrDefault("version", "26.3");
        Properties hooks = loadHooks(version);

        // Scenario mode (see ScenarioRunner): `scenario` is then the scenario file, not a label, and the ZPD dump
        // writer is not used.
        if (opts.containsKey("scenario_out")) {
            ScenarioRunner.configure(Path.of(require(opts, "scenario")), Path.of(opts.get("scenario_out")), hooks);
            inst.addTransformer(new TickTransformer(
                    hooks.getProperty("tick.class"),
                    hooks.getProperty("tick.method"),
                    hooks.getProperty("tick.descriptor")));
            System.out.println("[zenith-oracle] attached for " + version + " in scenario mode");
            return;
        }

        Hooks.configure(
                Path.of(require(opts, "out")),
                Integer.parseInt(opts.getOrDefault("ticks", "20")),
                Long.parseLong(opts.getOrDefault("seed", "0")),
                version,
                opts.getOrDefault("scenario", "tick-baseline"),
                hooks);

        if (opts.containsKey("chunks")) {
            Hooks.configureChunks(Path.of(opts.get("chunks")), Path.of(require(opts, "chunks_out")),
                    Integer.parseInt(opts.getOrDefault("chunks_extra", "0")));
        }

        inst.addTransformer(new TickTransformer(
                hooks.getProperty("tick.class"),
                hooks.getProperty("tick.method"),
                hooks.getProperty("tick.descriptor")));
        System.out.println("[zenith-oracle] attached for " + version + ", dumping to " + opts.get("out"));
    }

    private static Map<String, String> parseArgs(String args) {
        Map<String, String> out = new HashMap<>();
        if (args == null || args.isBlank()) {
            return out;
        }
        for (String part : args.split(",")) {
            int eq = part.indexOf('=');
            if (eq > 0) {
                out.put(part.substring(0, eq).trim(), part.substring(eq + 1).trim());
            }
        }
        return out;
    }

    private static String require(Map<String, String> opts, String key) {
        String v = opts.get(key);
        if (v == null) {
            throw new IllegalArgumentException("zenith-oracle: missing agent argument '" + key + "'");
        }
        return v;
    }

    private static Properties loadHooks(String version) throws IOException {
        Properties p = new Properties();
        try (InputStream in = Agent.class.getResourceAsStream("/hooks/" + version + ".properties")) {
            if (in == null) {
                throw new IOException("zenith-oracle: no hook file for version " + version);
            }
            p.load(in);
        }
        return p;
    }
}
