# ai-profile-manager (`aipm`)

An OS- and shell-agnostic switcher for **personal Claude Code configuration**, kept in the repo, for teams.

A team commits one shared Claude Code setup. Each developer wants their own tweaks — a different model, extra rules, their own MCP servers, a personal agent — without those leaking into the committed config or fighting with teammates. `aipm` lets each developer keep several named **personal profiles** in the repo and switch between them with one command, layered on top of the committed team config.

> **Status: experimental.** The tool is fully built and tested (single static Rust binary, 32 tests), but a few behaviors depend on real Claude Code internals that still need a live smoke test before production use — see [Caveats](#caveats) and issue #2.

## How it works

`aipm` does **not** merge configuration itself. It *projects* the active profile into Claude Code's own gitignored "local" slots and lets Claude Code's native precedence do the layering. Two layers:

1. **Team base** — committed, never modified by the tool: `.claude/settings.json`, `CLAUDE.md`, `.mcp.json`, `.claude/agents/`, `.claude/skills/`.
2. **Personal active profile** — gitignored, projected into the local slots on activation.

Switching profiles is a precise **clear-then-project**: the previous profile's artifacts (tracked in a manifest) are removed, then the new profile is projected. Nothing leaks between profiles, and the committed team config is never touched — except the single, idempotent `@.claude/local.md` import line that `aipm init` adds to `CLAUDE.md`.

```
.claude-profiles/            # gitignored, one dir per profile
  deep-focus/
    settings.json            # personal settings keys
    CLAUDE.md                # personal instructions
    mcp.json                 # personal MCP servers
    agents/                  # personal agent files
    skills/                  # personal skill files
  quick-fix/ ...
  .state.json                # active profile + projection manifest
```

Every part of a profile is optional. A profile that only sets a model contains just `settings.json`.

### What a profile projects into

| Profile part        | Projected into                          | Mechanism                                                                   |
| ------------------- | --------------------------------------- | --------------------------------------------------------------------------- |
| `settings.json`     | `.claude/settings.local.json`           | Claude Code's native personal-settings override                             |
| `CLAUDE.md`         | `.claude/local.md`                       | Read via the `@.claude/local.md` import in the committed `CLAUDE.md`         |
| `mcp.json`          | `local`-scope MCP servers                | `~/.claude.json` under `projects.<repo>.mcpServers` — private to you         |
| `agents/`, `skills/`| `.claude/agents/`, `.claude/skills/`     | Files dropped and gitignored, tracked in the manifest for clean removal     |

## Install

### Prebuilt binary (recommended)

No Rust toolchain required. The installer downloads a binary for your platform
from the latest [GitHub Release](https://github.com/jedymatt/ai-profile-manager/releases)
and places it in `~/.local/bin`.

**macOS / Linux:**

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/jedymatt/ai-profile-manager/releases/latest/download/aipm-installer.sh | sh
```

**Windows (PowerShell):**

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/jedymatt/ai-profile-manager/releases/latest/download/aipm-installer.ps1 | iex"
```

If `~/.local/bin` is not on your `PATH`, the installer prints how to add it.

### From source

Requires a [Rust toolchain](https://rustup.rs).

```bash
# from a clone of this repo
cargo install --path .

# or build the binary directly
cargo build --release   # -> target/release/aipm
```

## Quick start

```bash
cd your-project
aipm init                       # one-time setup (gitignore, CLAUDE.md import, default profile)

aipm new deep-focus             # create a profile
$EDITOR .claude-profiles/deep-focus/settings.json   # e.g. {"model": "opus"}

aipm use deep-focus             # activate it — projects into the local slots
aipm status                     # show the active profile and what's projected
aipm use quick-fix              # switch — old profile is cleared, new one projected
aipm deactivate                 # remove the personal overlay; team config stands alone
```

`aipm init` is the one step the team commits. It:

- creates `.claude/` and a `.claude-profiles/default/` profile,
- adds `.claude-profiles/`, `.claude/settings.local.json`, and `.claude/local.md` to `.gitignore`,
- appends a single `@.claude/local.md` import line to `CLAUDE.md` (idempotent; a missing/empty target is harmless).

## Commands

| Command                    | Description                                                              |
| -------------------------- | ------------------------------------------------------------------------ |
| `aipm init`                | Set up `aipm` in this repo (gitignore, `CLAUDE.md` import, `default` profile) |
| `aipm new <name>`          | Create a new empty profile                                               |
| `aipm list`                | List profiles (marks the active one with `*`)                            |
| `aipm use <name> [--force]`| Activate a profile, switching from any current one                       |
| `aipm status`              | Show the active profile and projected artifacts, with a drift check      |
| `aipm deactivate`          | Remove the active personal overlay, leaving only committed team config   |
| `aipm edit <name> [--print-path]` | Open a profile directory in `$EDITOR` (or print its path)         |
| `aipm remove <name>`       | Delete a profile (deactivating it first if it is active)                 |

## Safety guarantees

- **Never destroys what it doesn't own.** A local slot the tool didn't write (a hand-made `settings.local.json`, a committed agent of the same name, a foreign MCP server) is *foreign* and protected: `aipm` errors and tells you to rerun with `--force`.
- **`--force` backs up, never deletes blind.** Foreign files are renamed to `<name>.bak`; a displaced foreign MCP server is saved to the gitignored `.claude-profiles/.mcp-backups.json` before being overwritten.
- **Atomic writes.** Every slot write goes through a temp file + rename, so an interrupted switch leaves a recoverable state.
- **Clean switching.** A manifest tracks exactly what was projected, so switching leaves zero orphans.
- **No symlinks.** Pure file I/O — works identically on Linux, macOS, and Windows.

## Configuration

Two environment variables override the default locations (mainly for testing or non-standard setups):

- `AIPM_REPO_ROOT` — repo root (default: nearest ancestor containing `.git`)
- `AIPM_USER_CONFIG` — path to the user config that holds local-scope MCP servers (default: `~/.claude.json`)

## Extensibility

`aipm` targets Claude Code first, behind a `Target` trait that isolates all Claude-Code-specific knowledge. Adapters for other assistants (Cursor, Copilot, Codex) can be added as new `Target` implementations without changing the tool-neutral profile model.

## Caveats

The test suite is self-contained, but these behaviors depend on **real Claude Code internals** and should be smoke-tested in a live repo before relying on the tool (tracked in issue #2):

- `.claude/settings.local.json` overriding `.claude/settings.json`,
- a `@import` to an empty/missing file being silently skipped,
- local-scope MCP servers living at `projects.<repo>.mcpServers` in `~/.claude.json`,
- Claude Code discovering dropped `.claude/agents/` and `.claude/skills/` files.

## Development

```bash
cargo test --all                          # unit + integration tests
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

### Cutting a release

Releases are built and published by [`dist`](https://github.com/axodotdev/cargo-dist)
on tag push. The tag must match the `version` in `Cargo.toml`:

```bash
# bump version in Cargo.toml first, then:
git tag vX.Y.Z
git push --tags
```

The `release.yml` workflow builds the per-platform binaries and publishes a GitHub
Release with the binaries, checksums, and the `aipm-installer.sh` / `aipm-installer.ps1`
scripts. The workflow is generated — to change build targets or installers, edit the
dist config and run `dist generate`, never hand-edit `.github/workflows/release.yml`.

Design and implementation notes live in `docs/superpowers/specs/` and `docs/superpowers/plans/`.

## License

See [LICENSE](LICENSE).
