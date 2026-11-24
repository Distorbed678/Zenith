# Behavior specs (clean-room)

Every engine subsystem is implemented from a spec in this folder, **written in our own words**.

## Rules
1. Describe **what** happens and in **what order**: rules, state machines, edge cases, quirks, constants and IDs
   (facts needed for compatibility).
2. Never include code from the game, or pseudo-code that mirrors its structure. No pasted snippets, no
   "translated" methods.
3. Cite how each fact was established: public documentation, data-generator report, or oracle experiment
   (with the scenario id).
4. Every spec lists the parity suites that verify it.

## Template

```markdown
# <Area>: <Topic>
- Verified by suites: <suite ids>
- Sources: <public docs / report names / oracle scenarios>

## Summary
## Inputs and state
## Behavior (ordered rules)
## Edge cases and known quirks (with oracle scenario ids)
## Constants and IDs
## Open questions
```

Folder per area: `nbt/`, `storage/`, `registry/`, `codec/`, `math/`, `worldgen/`, `light/`, `world/`, `entity/`,
`item/`, `command/`, `rules/`, `protocol/`, `render/`, `gui/`, `audio/`, `client/`.
