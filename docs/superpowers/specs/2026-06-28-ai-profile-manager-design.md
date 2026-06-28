# ai-profile-manager — Design Spec

**Date:** 2026-06-28
**Status:** Approved design, pre-implementation
**Working binary name:** `aipm` (changeable)

## Summary

`aipm` is an OS- and shell-agnostic CLI that lets a developer keep several
**personal** Claude Code configuration profiles inside a repo and switch between
them, layered on top of the team's committed config — without ever polluting
that committed config.

The tool does not merge configuration itself. It **projects** the active
personal profile into Claude Code's own gitignored "local" slots and lets Claude
Code's native precedence do the layering. Switching profiles is a precise
clear-then-project operation tracked by a manifest, so no personal config leaks
between profiles and the committed team config is never modified (beyond a single
deliberate, committed adoption hook).

It targets Claude Code first, behind a `Target` trait so adapters for other
assistants (Cursor, Copilot, Codex) can be added later without redesigning the
profile model.

## Goals

- Each developer keeps multiple named personal profiles and switches between them.
- A personal profile layers over the committed team config; it never pollutes it.
- Works identically across Linux, macOS, and Windows — no shell or symlink dependency.
- Profiles can carry: `settings.json` keys, `CLAUDE.md` instructions, MCP servers,
  and agents/skills.
- Extensible to other assistants without redesign.

## Non-goals (v1)

- **Committed team-shared profile menus.** v1's driver is personal
  personalization. A committed catalog of team profiles is future work.
- **Non–Claude-Code adapters.** The `Target` trait reserves the seam; only
  `ClaudeCodeTarget` is built.
- **Cross-repo / global profiles.** Profiles are stored per-checkout in the repo's
  gitignored `.claude-profiles/` directory.
- **Custom merge semantics.** Layering is delegated to Claude Code's native
  precedence, not reimplemented.

## Core model

### Two layers

1. **Team base** — committed, the tool never edits it: `.claude/settings.json`,
   `CLAUDE.md`, `.mcp.json`, `.claude/agents/`, `.claude/skills/`.
2. **Personal active profile** — gitignored, projected into Claude Code's local
   slots on activation.

### A profile is a directory

Profiles live in the gitignored `.claude-profiles/` directory:

```
.claude-profiles/
  deep-focus/
    settings.json     # personal settings keys
    CLAUDE.md         # personal instructions
    mcp.json          # personal MCP servers
    agents/           # personal agent files
    skills/           # personal skill files
  quick-fix/
    ...
  .state.json         # active profile name + projection manifest
```

Every part of a profile is optional. A profile that only sets a model and a
couple of permissions contains just `settings.json`.

The tool treats each projected local slot as **fully owned and regenerated** from
the active profile — it does not merge a profile into pre-existing local-slot
content. `settings.local.json` is written wholesale from the profile's
`settings.json`, `.claude/local.md` from its `CLAUDE.md`, and so on. A local slot
the tool does not own (not listed in any manifest) is treated as foreign and
protected, never silently overwritten (see Error handling).

### Activation projects into native local slots

`aipm use <name>` projects the active profile into Claude Code's own gitignored
slots. Claude Code then resolves precedence — the tool performs no merging.

| Profile part   | Projected into                        | Mechanism                                                                                          |
| -------------- | ------------------------------------- | ------------------------------------------------------------------------------------------------- |
| `settings.json`| `.claude/settings.local.json`         | Claude Code's native personal-settings override (already gitignored by convention).               |
| `CLAUDE.md`    | `.claude/local.md` (gitignored)       | Committed `CLAUDE.md` carries a one-time `@.claude/local.md` import; a missing target is skipped harmlessly. |
| `mcp.json`     | `local`-scope MCP servers             | Registered private-to-you for this project; lives in user config, not the committed `.mcp.json`.  |
| `agents/`,`skills/` | `.claude/agents/`, `.claude/skills/` | Files dropped and gitignored, tracked in the manifest so switching removes exactly what it added. |

### The manifest makes switching safe

`.state.json` records the active profile and the **exact** list of files and MCP
entries the tool projected. Switching is:

1. Clear the artifacts named in the old manifest.
2. Project the new profile, producing a new manifest.

This guarantees no orphaned artifacts leak between profiles, and the tool only
ever removes things it created.

### The single committed touch

Adopting the tool (`aipm init`) appends one idempotent `@.claude/local.md` import
line to the committed `CLAUDE.md`. This is a deliberate, one-time, team-committed
adoption step — not pollution from switching. Because Claude Code skips a missing
import target, committing this line is safe even for teammates who never create a
profile. (Verify the missing-import-skip behavior during implementation; if it
errors, `init` also creates an empty committed `.claude/local.md` placeholder or
the import is guarded another way.)

## Components (Rust crate layout)

Each module has one job and a clear interface; `target` and `slots` concentrate
the OS-specific surface and carry the heaviest tests.

- **`cli`** — `clap` arg parsing and command dispatch. Commands: `init`,
  `new <name>`, `list`, `use <name>`, `status`, `deactivate`, `edit <name>`,
  `remove <name>`. Orchestration only; no file logic.
- **`profile`** — loads and represents a profile from `.claude-profiles/<name>/`.
  Tool-neutral data model (settings blob, markdown, MCP entries, agent/skill file
  paths). Knows nothing about Claude Code.
- **`target`** — the extensibility seam: the `Target` trait
  (`project(&Profile) -> Manifest`, `clear(&Manifest)`) and the
  `ClaudeCodeTarget` implementation. The only module that knows Claude Code's slot
  mapping.
- **`manifest`** — the record of what a target projected (file paths + MCP entry
  ids). Serialized into `.state.json`. Drives precise cleanup and drift detection.
- **`slots`** — low-level, cross-platform write helpers used by `ClaudeCodeTarget`:
  atomic JSON write for `settings.local.json`, local-scope MCP read-modify-write
  against user config, `CLAUDE.md` import-hook insertion, agent/skill file drops.
  All paths via `std::path`; no shell, no hardcoded separators.
- **`gitignore`** — idempotently ensures `.claude-profiles/`,
  `.claude/settings.local.json`, `.claude/local.md`, and projected agent/skill
  files are ignored.
- **`state`** — atomic read/write of `.state.json` (active profile + manifest).

## Data flow

### `aipm init` (one-time, committed)

1. Verify a repo and `.claude/` exist (offer to create `.claude/`).
2. Append the `@.claude/local.md` import to committed `CLAUDE.md` (idempotent) —
   the single intentional committed change.
3. Add gitignore entries.
4. Scaffold `.claude-profiles/` with a `default` profile.

### `aipm use <name>` (hot path)

1. Load profile `<name>`; if missing, error early and list available profiles.
2. Read `.state.json` for the current manifest.
3. `ClaudeCodeTarget.clear(old_manifest)` — remove previously projected local
   files, deregister local-scope MCP entries.
4. `ClaudeCodeTarget.project(profile)` — write slots, return the new manifest.
5. Persist `.state.json` (active = `<name>`, new manifest).
6. Print a diff-style summary of what changed.

### `aipm status`

Show the active profile, the projected manifest, and a **drift check**: did
anything the tool owns get hand-edited or deleted out from under it?

### `aipm deactivate`

Clear the current manifest; leave no personal overlay. Committed team config
stands alone.

Projection writes to temp files plus atomic rename where the OS allows, so a
crash mid-switch leaves the prior state recoverable from `.state.json`.

## Error handling

Guiding rule: **never silently destroy anything the tool does not own.**

- **Not in a repo / no `.claude/`** → instruct to run `aipm init`; don't guess.
- **Unknown profile** → list available profiles.
- **Foreign `settings.local.json` or local files not in our manifest** → these were
  hand-made by the developer. Refuse to clobber; back up to `*.bak` or require
  `--force`. Surface clearly.
- **MCP user-config (`~/.claude.json`) malformed or locked** → validate, then
  atomic-write. On failure, abort that step without corrupting the file and report
  which parts of the profile applied.
- **Partial projection failure** → roll back to the prior manifest where feasible;
  otherwise report the precise half-applied state rather than claiming success.
- **Cross-platform path/permission errors** → explicit, actionable messages.

`status`'s drift check makes half-applied or hand-edited states visible instead of
mysterious.

## Testing

- **Unit:** profile loading, manifest diffing, gitignore idempotency, JSON atomic
  write, MCP local-scope (de)serialization, import-hook insertion idempotency.
- **Integration:** a temp dir as a fake repo; run
  `init → new → use → status → use(other) → deactivate`; assert exact files
  written and cleaned, **committed files untouched**, and **zero orphans after
  switching**.
- **Golden tests:** snapshot the projected slot outputs for a known profile.
- **Cross-platform CI matrix:** Linux, macOS, Windows. With no symlinks the surface
  is mostly path handling — caught here.

## Build order

All four artifact types are in v1, sequenced by risk so each step ships working:

1. **Settings + CLAUDE.md** — lowest risk, highest value; proves the model
   end-to-end.
2. **MCP servers** — touches `~/.claude.json`; needs the validate/atomic-write care
   above.
3. **Agents & skills** — needs manifest-driven cleanup to avoid orphans.

## Open items to verify during implementation

- Exact Claude Code precedence for `settings.local.json` over `settings.json`
  (assumed: local overrides shared).
- Behavior of a `@import` pointing at a missing file (assumed: skipped silently).
- The precise on-disk shape and location of `local`-scope MCP servers in user
  config (assumed: `~/.claude.json`, keyed by project).
- Whether Claude Code discovers agents/skills placed in `.claude/agents/` and
  `.claude/skills/` without restart, and any naming constraints.
