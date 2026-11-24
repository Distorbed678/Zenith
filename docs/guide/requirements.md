# Requirements

Zenith runs **your own copy** of Minecraft: Java Edition. It ships none of the game's files.

## You need

1. **A legitimate copy of the matching game version, launched at least once** from a supported launcher:
   Prism Launcher, MultiMC, or the official launcher. Zenith 0.x targets **26.3**.
   Zenith checks three things before it starts:
   - the game jar is the official one (its hash matches Mojang's published manifest);
   - every asset file for that version is present and intact;
   - the version has really been started at least once.
2. **A GPU with Vulkan 1.3**: NVIDIA GTX 900 series or newer, AMD RX 400 or newer, Intel Gen9 or newer, Apple
   M-series (through MoltenVK), or a Steam Deck.
3. **One of these platforms:** Linux x86_64, Linux ARM64, Windows x86_64, or macOS on Apple Silicon.
4. **Optional:** a Microsoft account that owns the game. Online-mode servers need it once Microsoft sign-in is enabled.

## Check your install now

```bash
cargo run -p zenith-install -- check --version 26.3
```

Every line should say `pass`. The Microsoft line says `skip` until sign-in is enabled.

| Message | What to do |
|---------|------------|
| No launcher install found | Install the version in Prism, MultiMC or the official launcher, or set `ZENITH_LAUNCHER_ROOT` to your launcher folder |
| Jar hash does not match | Your jar was modified. Delete that version in your launcher and let it download again |
| Asset index missing or files missing | Launch the version once from your launcher, so it downloads every asset |
| No launch evidence | Start the version once from your launcher and reach the title screen |
