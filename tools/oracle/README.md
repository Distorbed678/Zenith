# Parity oracle agent

Our own Java agent. It is loaded into the vanilla game **started from the user's own install**, and writes `ZPD1`
state dumps (format: `docs/specs/parity/dump-format.md`) that Zenith's dumps are diffed against.

- Pure JDK 25 (`java.lang.classfile` for bytecode injection, plus reflection). No third-party libraries.
- Compiles **without** the game on the classpath: hook targets are named in `hooks/<version>.properties`
  (class/method/descriptor facts listed with `javap`), and game state is read reflectively by public method name.
- Never shipped to players. Built and run locally (and in the owner-approved nightly CI) by `cargo xtask oracle`.

```bash
cargo xtask oracle build
cargo xtask oracle run --ticks 100 --seed 12345 --out /tmp/vanilla.zpd   # needs EULA acceptance, see below
cargo run -p zenith-parity -- diff /tmp/a.zpd /tmp/b.zpd
```

The headless server only starts after the Minecraft EULA is accepted. The owner accepted it for local and CI runs
(decision 10.2). Other users opt in with `--accept-eula`, which writes `eula=true` only in the oracle's temporary
run folder.
