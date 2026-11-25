//! Find the player's own game install and prove they own it, without shipping or downloading any game content.
//!
//! Zenith only runs when every local proof check passes for the requested version:
//!
//! 1. **Official jar hash.** The client jar's SHA-1 matches Mojang's published manifest.
//! 2. **Full asset set.** The asset index matches Mojang's hash, and every object it lists is present and intact.
//! 3. **Launch evidence.** The game has really been started at least once (options or log written by that version).
//!
//! A fourth check, Microsoft account ownership, is pluggable. It reports `Skipped` until an approved Azure app ID is
//! configured (see `docs/plan/06-AUTH-OWNERSHIP.md`).

pub mod assets;
pub mod discover;
pub mod evidence;
pub mod jar;
pub mod java;
pub mod libraries;
pub mod manifest;
pub mod proof;

pub use discover::{GameInstall, LauncherKind, discover_installs};
pub use proof::{CheckResult, CheckStatus, OwnershipReport, ProofOptions, prove_ownership};

/// Zenith's per-user cache directory (`ZENITH_CACHE_DIR` overrides it).
pub fn cache_dir() -> std::path::PathBuf {
    if let Some(p) = std::env::var_os("ZENITH_CACHE_DIR") {
        return p.into();
    }
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("zenith")
}
