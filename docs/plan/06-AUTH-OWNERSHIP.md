# 06: Ownership and authentication

Owner decisions 1.2 → 2.2 → 2.3: **local proof (three checks) + Microsoft sign-in.** The Azure app registration is free.
Mojang must approve the app ID before the Minecraft-services step works. Until then, Zenith runs on local proof alone.

## Local proof: `zenith-install` (✅ implemented in M0)

| Check | How | Fails when |
|-------|-----|------------|
| **Official jar hash** | SHA-1 of the client jar vs Mojang's version manifest (live → cached → built-in table). The launcher's own JSON is never trusted for hashes. | The jar was modified or isn't official |
| **Full asset set verified** | The asset index file's SHA-1 must match the manifest; then every listed object must exist under `assets/objects/xx/<hash>` with the right size and SHA-1 (hashed in parallel; ~5,100 objects / 465 MiB for 26.3) | Assets were never downloaded (the game was never launched by a launcher) or are corrupt |
| **Launch evidence** | In any instance running this version: `options.txt` contains `version:<world_version>` (written only by the game), **or** a log (`latest.log`/`*.log.gz`) mentions starting exactly this version. Prism `lastLaunchTime` is supporting evidence only. | The version was installed but never started |

Verified against the owner's install on 2026-11-24: all three pass (`cargo run -p zenith-install -- check`).

Supported layouts: Prism Launcher (native + Flatpak), MultiMC, and the official launcher on Linux/Windows/macOS.
`ZENITH_LAUNCHER_ROOT` adds a custom root. The launcher (M26) adds the Modrinth App and CurseForge App layouts when
importing.

**Honest limitation:** local proof shows a real install that has been launched, not a purchase. The official jar can be
downloaded without buying the game, and a determined person could hand-write an `options.txt`. That is why Microsoft
sign-in is added on top.

## Microsoft sign-in: `zenith-auth` (M23)

### One-time setup (owner action; it blocks M23)
1. Create an Azure account (free; Azure asks for a card to verify identity, and does not charge for this).
2. In Microsoft Entra ID → App registrations → New registration:
   name "Zenith Launcher"; supported accounts: **personal Microsoft accounts only**;
   platform "Mobile and desktop applications"; redirect `http://localhost` (loopback); enable **public client flows**
   (device code).
3. Copy the **Application (client) ID**. It is not a secret and is safe to embed in the launcher.
4. Submit the app ID to Mojang's review form for Minecraft-services API access (`aka.ms/mce-reviewappid`). Approval can
   take days to weeks.
5. Put the client ID in `crates/zenith-auth/client_id.txt` (or the `ZENITH_MSA_CLIENT_ID` build env).

### Flow (implemented in Rust, no embedded browser)
```
device code (or loopback browser) OAuth 2.0 with scope XboxLive.signin offline_access
  → Xbox Live user token (user.auth.xboxlive.com)
  → XSTS token for the Minecraft relying party (xsts.auth.xboxlive.com)
  → Minecraft services login_with_xbox → Minecraft access token
  → entitlements check (game_minecraft / product_minecraft) → profile (uuid, name, skins, capes)
```
- Tokens are stored **only in the OS keyring** (Secret Service on Linux, Credential Manager on Windows, Keychain on
  macOS; suggestion 8.4). Refresh tokens rotate. Nothing is written to plain files.
- **Offline grace:** after a successful online check, offline play is allowed for N days (default 30, like vanilla's
  offline mode). Local proof is still required on every launch.
- Multiplayer: session join for online-mode servers; profile/skin for the 3D skin viewer (suggestion 7.4); chat signing
  keys for secure chat.

### Check matrix

| Mode | Local proof | MS sign-in | Can play |
|------|-------------|------------|----------|
| Before Azure approval | required | shows `skip` | Singleplayer, LAN, offline-mode servers, Zenith servers in offline mode |
| After approval, signed in | required | required once, then grace period | Everything incl. online-mode servers |
| After approval, never signed in | required | missing | Singleplayer only (with a sign-in prompt) |

The `microsoft` check already exists in the proof report as `Skipped`, so turning it on is a configuration change, not
a redesign.
