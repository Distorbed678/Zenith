# 0003: Ownership = local proof + Microsoft sign-in

- Status: accepted
- Date: 2026-11-24
- Owner answer reference: 1.2 (superseded), 2.2, 2.3

## Decision
Three local checks (official jar SHA-1, full asset set hash-verified, launch evidence) gate every launch. Microsoft
sign-in (device code → Xbox Live → XSTS → Minecraft services → entitlement) is added once an Azure app ID is approved
by Mojang. Until then, the `microsoft` check reports `Skipped`. Tokens live only in the OS keyring.

## Consequences
+ Works today without any approval; becomes strong proof once approved.
− Online-mode servers need the approval to come through first.
