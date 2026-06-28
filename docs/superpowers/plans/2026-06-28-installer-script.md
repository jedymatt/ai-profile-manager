# aipm Installer (prebuilt binary via dist) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let users install `aipm` with a one-line `curl … | sh` / PowerShell command that downloads a prebuilt binary from GitHub Releases — no Rust toolchain required.

**Architecture:** Adopt [`dist`](https://github.com/axodotdev/cargo-dist). It reads config from `Cargo.toml` and generates the installer scripts plus a tag-triggered GitHub Actions release workflow that builds per-target binaries and publishes them (with checksums and the generated installers) to a GitHub Release. We add package metadata + dist config; the workflow is generated, never hand-edited.

**Tech Stack:** Rust (cargo), `dist` (formerly cargo-dist), GitHub Actions, GitHub Releases.

## Global Constraints

- **Rust toolchain not on PATH.** Prefix every `cargo` and `dist` command with `. "$HOME/.cargo/env" &&`. This applies to subagents too.
- **`cargo-dist-version` must equal the locally installed `dist` version exactly** — a mismatch makes `dist` error during planning. Pin it to the output of `dist --version`.
- **Targets (exact, no more, no fewer):** `x86_64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`.
- **Installers:** `shell`, `powershell`. **CI backend:** `github`. **Install path:** `~/.local/bin`.
- **Repository URL:** `https://github.com/jedymatt/ai-profile-manager`. **License:** `MIT`.
- **Never hand-edit `.github/workflows/release.yml`** — change dist config and regenerate with `dist generate`.
- **Do not touch `.github/workflows/ci.yml`** — testing and releasing stay decoupled.
- **Existing checks must stay green:** `cargo test --all`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all -- --check`.
- Work happens on the `installer-script` branch (already created).

---

## File Structure

- `Cargo.toml` (modify) — add `[package]` metadata; gains `[workspace.metadata.dist]` (or a sibling `dist-workspace.toml`) and `[profile.dist]` from `dist init`.
- `dist-workspace.toml` (possibly created by `dist init`, depending on the installed dist version) — holds the `[dist]` config table if dist puts it here instead of in `Cargo.toml`. Either location is acceptable; the values are what matter.
- `.github/workflows/release.yml` (create, generated) — the release pipeline.
- `README.md` (modify) — promote prebuilt one-liners as the primary install path; document the release procedure.

---

## Task 1: Add package metadata to Cargo.toml

`dist init` errors without a repository URL, and these fields are good crate hygiene regardless. This task is the prerequisite that unblocks dist.

**Files:**
- Modify: `Cargo.toml` (the `[package]` table, lines 1-4)

**Interfaces:**
- Consumes: nothing.
- Produces: a `[package]` table with `description`, `repository`, `license`, `readme` set — consumed by Task 2 (`dist init` reads `repository`).

- [ ] **Step 1: Verify dist would currently complain (baseline)**

Confirm the metadata is absent so we know the change is meaningful.

Run: `grep -E '^(repository|license|description|readme) *=' Cargo.toml; echo "exit:$?"`
Expected: no matching lines, `exit:1` (none present yet).

- [ ] **Step 2: Add the metadata fields**

Edit the `[package]` table in `Cargo.toml` so it reads exactly:

```toml
[package]
name = "aipm"
version = "0.1.0"
edition = "2021"
description = "An OS- and shell-agnostic switcher for personal Claude Code configuration, kept in the repo, for teams."
repository = "https://github.com/jedymatt/ai-profile-manager"
license = "MIT"
readme = "README.md"
```

- [ ] **Step 3: Verify the manifest still parses**

Run: `. "$HOME/.cargo/env" && cargo metadata --no-deps --format-version 1 >/dev/null && echo OK`
Expected: `OK` (manifest is valid).

- [ ] **Step 4: Confirm fields are present**

Run: `grep -E '^(repository|license|description|readme) *=' Cargo.toml`
Expected: all four lines printed.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml
git commit -m "chore: add package metadata required for dist"
```

---

## Task 2: Install dist, configure it, and generate the release workflow

Deliverable: a valid dist config matching our decisions, a generated `release.yml`, and a clean `dist plan`. Installing dist is setup folded into this task.

**Files:**
- Modify: `Cargo.toml` (adds `[workspace.metadata.dist]` and `[profile.dist]`) — OR
- Create: `dist-workspace.toml` (if the installed dist version stores config there)
- Create: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: the `repository` field from Task 1.
- Produces: `.github/workflows/release.yml` and a dist config table whose `installers`, `targets`, `install-path`, `ci`, and `cargo-dist-version` match the Global Constraints. Consumed by Task 4's verification gate.

- [ ] **Step 1: Install dist**

Use cargo (the toolchain is present in this environment) for a reliable install:

Run: `. "$HOME/.cargo/env" && cargo install cargo-dist --locked`
Expected: builds and installs the `dist` binary into `~/.cargo/bin`.
(Alternative if preferred: the official installer at `https://github.com/axodotdev/cargo-dist` — but `cargo install` avoids guessing a version-specific URL.)

- [ ] **Step 2: Record the installed version**

Run: `. "$HOME/.cargo/env" && dist --version`
Expected: prints e.g. `dist X.Y.Z`. **Note this `X.Y.Z`** — it is the value `cargo-dist-version` must equal.

- [ ] **Step 3: Run dist init to scaffold config + workflow**

Run: `. "$HOME/.cargo/env" && dist init --yes`
Expected: succeeds; reports creating/updating the dist config and `.github/workflows/release.yml`. It also adds a `[profile.dist]` to `Cargo.toml`. If it errors about the repository URL, recheck Task 1.

- [ ] **Step 4: Locate the dist config table**

Run: `grep -rln 'cargo-dist-version\|^\[dist\]\|metadata.dist' Cargo.toml dist-workspace.toml 2>/dev/null`
Expected: prints the file holding the dist config (`Cargo.toml` for `[workspace.metadata.dist]`, or `dist-workspace.toml` for `[dist]`). Edit that file in the next step.

- [ ] **Step 5: Set the config values to our decisions**

In the dist config table found in Step 4, ensure these keys are set to exactly these values (key names are identical whether the table header is `[workspace.metadata.dist]` or `[dist]`); set `cargo-dist-version` to the `X.Y.Z` from Step 2:

```toml
cargo-dist-version = "X.Y.Z"
ci = ["github"]
installers = ["shell", "powershell"]
targets = [
    "x86_64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc",
]
install-path = "~/.local/bin"
```

- [ ] **Step 6: Regenerate the workflow from the updated config**

Run: `. "$HOME/.cargo/env" && dist generate`
Expected: rewrites `.github/workflows/release.yml` to match the config (the four build jobs + plan/publish).

- [ ] **Step 7: Verify the plan is valid**

Run: `. "$HOME/.cargo/env" && dist plan`
Expected: prints a planned release listing all four targets and the `shell` + `powershell` installers; exits 0 with no config errors.

- [ ] **Step 8: Verify no drift between config and generated workflow**

Run: `. "$HOME/.cargo/env" && dist generate --check`
Expected: exits 0, reporting the generated files are up to date (no diff).

- [ ] **Step 9: Confirm ci.yml is untouched**

Run: `git status --porcelain .github/workflows/ci.yml`
Expected: no output (ci.yml unchanged).

- [ ] **Step 10: Commit**

```bash
git add Cargo.toml dist-workspace.toml .github/workflows/release.yml
git commit -m "ci: prebuilt-binary release pipeline and installers via dist"
```
(If `dist-workspace.toml` was not created, the `git add` simply skips it.)

---

## Task 3: Update README install and release documentation

**Files:**
- Modify: `README.md` (the `## Install` section, lines 41-51; the `## Development` section, lines 115-123)

**Interfaces:**
- Consumes: the repository URL and the fact that Releases will host `aipm-installer.sh` / `aipm-installer.ps1` (dist's default installer artifact names).
- Produces: user-facing install instructions. No downstream consumer.

- [ ] **Step 1: Replace the Install section body**

Replace the content under `## Install` (currently the "Requires a Rust toolchain" line and the two code blocks) with:

````markdown
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
````

- [ ] **Step 2: Add the release procedure to the Development section**

Append this subsection at the end of the `## Development` section (after the `cargo fmt … --check` block, before `## License`):

````markdown
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
````

- [ ] **Step 3: Verify the install one-liners are present and well-formed**

Run: `grep -c 'releases/latest/download/aipm-installer' README.md`
Expected: `2` (the shell and powershell one-liners).

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs: document prebuilt-binary install and release procedure"
```

---

## Task 4: Final verification gate

Deliverable: confirmation that the source checks and dist checks are all green together. No code changes — this is the definition-of-done gate.

**Files:** none modified.

**Interfaces:**
- Consumes: everything from Tasks 1-3.
- Produces: a green build. No downstream consumer.

- [ ] **Step 1: Formatting check**

Run: `. "$HOME/.cargo/env" && cargo fmt --all -- --check && echo FMT_OK`
Expected: `FMT_OK` (our changes are TOML/markdown only, so this stays green).

- [ ] **Step 2: Lints**

Run: `. "$HOME/.cargo/env" && cargo clippy --all-targets -- -D warnings && echo CLIPPY_OK`
Expected: `CLIPPY_OK`.

- [ ] **Step 3: Tests**

Run: `. "$HOME/.cargo/env" && cargo test --all && echo TESTS_OK`
Expected: existing suite passes, `TESTS_OK`.

- [ ] **Step 4: dist plan + drift check together**

Run: `. "$HOME/.cargo/env" && dist plan && dist generate --check && echo DIST_OK`
Expected: `DIST_OK` (valid plan, no workflow drift).

- [ ] **Step 5: Confirm a clean working tree**

Run: `git status --porcelain`
Expected: no output (everything committed across Tasks 1-3).

---

## Post-implementation (manual, cannot be done pre-release)

The download path can only be fully exercised once a real GitHub Release exists. After merge:

1. Bump `version`, tag `vX.Y.Z`, push the tag; confirm `release.yml` publishes a Release with all four target archives, checksums, and both installer scripts.
2. Run each install one-liner on a clean machine/container and confirm `aipm --help` works from `~/.local/bin`.

This mirrors the README's existing "Caveats" stance on behaviors that need a live environment.
