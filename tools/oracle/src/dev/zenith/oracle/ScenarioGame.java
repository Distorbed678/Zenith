// Zenith parity oracle — original source, MIT OR Apache-2.0.
package dev.zenith.oracle;

import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.HashMap;
import java.util.Map;
import java.util.Optional;

/**
 * Reflective access to the game classes the scenario runner needs. Only public names (plus the few private fields
 * listed in docs/specs/parity/scenarios.md: the level random, the random-tick LCG, the sub-tick counter and the
 * block-event queue) are used, so this compiles against the JDK alone.
 */
final class ScenarioGame {
    private ScenarioGame() {}

    static final Class<?> BLOCK_POS = cls("net.minecraft.core.BlockPos");
    static final Class<?> BLOCK = cls("net.minecraft.world.level.block.Block");
    static final Class<?> BLOCK_STATE = cls("net.minecraft.world.level.block.state.BlockState");
    static final Class<?> FLUID = cls("net.minecraft.world.level.material.Fluid");
    static final Class<?> TICK_PRIORITY = cls("net.minecraft.world.ticks.TickPriority");
    static final Class<?> LEVEL = cls("net.minecraft.world.level.Level");

    private static final Constructor<?> POS_CTOR = ctor(BLOCK_POS, int.class, int.class, int.class);
    private static final Map<String, Method> METHODS = new HashMap<>();
    private static Object blockRegistry;
    private static Object fluidRegistry;

    static Class<?> cls(String name) {
        try {
            return Class.forName(name);
        } catch (ClassNotFoundException e) {
            throw new IllegalStateException("game class not found: " + name, e);
        }
    }

    private static Constructor<?> ctor(Class<?> c, Class<?>... params) {
        try {
            return c.getConstructor(params);
        } catch (NoSuchMethodException e) {
            throw new IllegalStateException(e);
        }
    }

    static Object pos(int x, int y, int z) throws Exception {
        return POS_CTOR.newInstance(x, y, z);
    }

    /** Public instance method by name and exact parameter types (cached per receiver class). */
    static Method method(Class<?> c, String name, Class<?>... params) throws NoSuchMethodException {
        StringBuilder key = new StringBuilder(c.getName()).append('#').append(name);
        for (Class<?> p : params) {
            key.append(',').append(p.getName());
        }
        Method m = METHODS.get(key.toString());
        if (m == null) {
            m = c.getMethod(name, params);
            m.setAccessible(true);
            METHODS.put(key.toString(), m);
        }
        return m;
    }

    static Object call(Object target, String name) throws Exception {
        return method(target.getClass(), name).invoke(target);
    }

    static Object call(Object target, String name, Class<?>[] types, Object... args) throws Exception {
        return method(target.getClass(), name, types).invoke(target, args);
    }

    static Object staticField(Class<?> c, String name) throws Exception {
        return c.getField(name).get(null);
    }

    /** A declared (possibly private) instance field, searched up the class hierarchy. */
    static Field field(Class<?> c, String name) throws NoSuchFieldException {
        for (Class<?> k = c; k != null; k = k.getSuperclass()) {
            try {
                Field f = k.getDeclaredField(name);
                f.setAccessible(true);
                return f;
            } catch (NoSuchFieldException next) {
                // keep looking in the superclass
            }
        }
        throw new NoSuchFieldException(c.getName() + "." + name);
    }

    // ---- registries -----------------------------------------------------------------------------------------

    private static Object registry(String field) throws Exception {
        return staticField(cls("net.minecraft.core.registries.BuiltInRegistries"), field);
    }

    static Object blockRegistry() throws Exception {
        if (blockRegistry == null) {
            blockRegistry = registry("BLOCK");
        }
        return blockRegistry;
    }

    static Object fluidRegistry() throws Exception {
        if (fluidRegistry == null) {
            fluidRegistry = registry("FLUID");
        }
        return fluidRegistry;
    }

    static Object byId(Object registry, int id) throws Exception {
        Object v = call(registry, "byId", new Class<?>[] {int.class}, id);
        if (v == null) {
            throw new IllegalArgumentException("no registry entry with raw id " + id);
        }
        return v;
    }

    static int idOf(Object registry, Object value) throws Exception {
        return (Integer) call(registry, "getId", new Class<?>[] {Object.class}, value);
    }

    static String nameOf(Object registry, Object value) throws Exception {
        return String.valueOf(call(registry, "getKey", new Class<?>[] {Object.class}, value));
    }

    // ---- block states ---------------------------------------------------------------------------------------

    static Object stateById(int id) throws Exception {
        Object s = method(BLOCK, "stateById", int.class).invoke(null, id);
        if (s == null) {
            throw new IllegalArgumentException("no block state with global id " + id);
        }
        return s;
    }

    static int stateId(Object state) throws Exception {
        return (Integer) method(BLOCK, "getId", BLOCK_STATE).invoke(null, state);
    }

    static Object priority(int value) throws Exception {
        return method(TICK_PRIORITY, "byValue", int.class).invoke(null, value);
    }

    // ---- the pieces of level state a scenario pins or reads --------------------------------------------------

    /** {@code Level.random}: the level's legacy 48-bit generator. */
    static Object levelRandom(Object level) throws Exception {
        return field(LEVEL, "random").get(level);
    }

    /** The raw 48-bit internal state of a legacy random source (the value the next draw starts from). */
    static long randomState(Object random) throws Exception {
        Object atomic = field(random.getClass(), "seed").get(random);
        return (Long) call(atomic, "get");
    }

    static void setRandomState(Object random, long state) throws Exception {
        Object atomic = field(random.getClass(), "seed").get(random);
        call(atomic, "set", new Class<?>[] {long.class}, state & 0xFFFFFFFFFFFFL);
        // A raw state also forgets any cached Gaussian value, exactly like a seed change does.
        Object gauss = field(random.getClass(), "gaussianSource").get(random);
        call(gauss, "reset");
    }

    /** Cached second Gaussian of the Marsaglia polar method: {present, value}. */
    static Object[] gaussianCache(Object random) throws Exception {
        Object gauss = field(random.getClass(), "gaussianSource").get(random);
        boolean have = (Boolean) field(gauss.getClass(), "haveNextNextGaussian").get(gauss);
        double value = (Double) field(gauss.getClass(), "nextNextGaussian").get(gauss);
        return new Object[] {have, have ? value : 0.0};
    }

    static int randValue(Object level) throws Exception {
        return (Integer) field(LEVEL, "randValue").get(level);
    }

    static void setRandValue(Object level, int v) throws Exception {
        field(LEVEL, "randValue").setInt(level, v);
    }

    static long subTickCount(Object level) throws Exception {
        return (Long) field(LEVEL, "subTickCount").get(level);
    }

    static void setSubTickCount(Object level, long v) throws Exception {
        field(LEVEL, "subTickCount").setLong(level, v);
    }

    static long gameTime(Object level) throws Exception {
        return (Long) call(level, "getGameTime");
    }

    /** Sets the game time held by the overworld's level data (other dimensions read it through a derived view). */
    static void setOverworldGameTime(Object server, long time) throws Exception {
        Object overworld = call(server, "overworld");
        Object data = field(overworld.getClass(), "serverLevelData").get(overworld);
        method(data.getClass(), "setGameTime", long.class).invoke(data, time);
    }

    static String dimensionId(Object level) throws Exception {
        return String.valueOf(call(call(level, "dimension"), "identifier"));
    }

    /** Finds a game rule by its short id (for example {@code random_tick_speed}) and sets it from command text. */
    static void setGameRule(Object server, String id, String text) throws Exception {
        Class<?> rules = cls("net.minecraft.world.level.gamerules.GameRules");
        Class<?> ruleClass = cls("net.minecraft.world.level.gamerules.GameRule");
        Object rule = null;
        for (Field f : rules.getFields()) {
            if (java.lang.reflect.Modifier.isStatic(f.getModifiers()) && ruleClass.isAssignableFrom(f.getType())) {
                Object r = f.get(null);
                if (id.equals(String.valueOf(call(r, "id")))) {
                    rule = r;
                    break;
                }
            }
        }
        if (rule == null) {
            throw new IllegalArgumentException("unknown game rule '" + id + "'");
        }
        Object parsed = call(rule, "deserialize", new Class<?>[] {String.class}, text);
        Optional<?> value = (Optional<?>) call(parsed, "result");
        if (value.isEmpty()) {
            throw new IllegalArgumentException("bad value '" + text + "' for game rule '" + id + "'");
        }
        Object gameRules = call(server, "getGameRules");
        method(gameRules.getClass(), "set", ruleClass, Object.class, cls("net.minecraft.server.MinecraftServer"))
                .invoke(gameRules, rule, value.get(), server);
    }
}
