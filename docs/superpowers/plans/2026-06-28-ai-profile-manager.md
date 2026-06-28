# ai-profile-manager Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `aipm`, a Rust CLI that stores multiple personal Claude Code config profiles per checkout and switches between them by projecting the active profile into Claude Code's native gitignored local slots — never touching committed team config.

**Architecture:** Profiles are gitignored directories under `.claude-profiles/`. Activation *projects* a profile (settings, CLAUDE.md, MCP servers, agents/skills) into Claude Code's own local slots (`settings.local.json`, an imported `local.md`, `local`-scope MCP in `~/.claude.json`, dropped agent/skill files). A serialized `Manifest` records exactly what was projected so switching is a precise clear-then-project. A `Target` trait isolates all Claude-Code-specific knowledge so future assistants become new implementations.

**Tech Stack:** Rust (2021 edition, stable). `clap` (derive) for CLI, `serde`/`serde_json` for config, `anyhow` for errors, `dirs` for home resolution, `tempfile` for atomic writes. Tests: inline `#[cfg(test)]` unit tests + `assert_cmd`/`predicates` integration tests.

## Global Constraints

- Language: Rust, 2021 edition, stable toolchain. Single static binary named `aipm`.
- Cross-platform: Linux, macOS, Windows. **No symlinks.** All paths via `std::path` — never hardcode `/` or `\`.
- **Never silently destroy anything the tool does not own.** Files/MCP entries not recorded in a manifest are "foreign" and are protected (error or `--force` backup), never overwritten blind.
- The tool **delegates layering to Claude Code** — it does not merge config itself. Each local slot is fully owned and regenerated from the active profile, not merged into.
- Committed team config (`.claude/settings.json`, `CLAUDE.md`, `.mcp.json`, `.claude/agents/`, `.claude/skills/`) is never modified, except the one idempotent `@.claude/local.md` import line added to `CLAUDE.md` by `aipm init`.
- Profiles live in the gitignored `.claude-profiles/` directory, one subdirectory per profile.
- Every part of a profile is optional.

## Cross-task API (defined here, used throughout)

These signatures are the contract between tasks. Later tasks rely on these exact names/types.

```rust
// context.rs
pub struct Context { pub repo_root: PathBuf, pub user_config: PathBuf }
impl Context {
    pub fn new(repo_root: PathBuf, user_config: PathBuf) -> Context;
    pub fn discover() -> anyhow::Result<Context>;     // honors AIPM_REPO_ROOT / AIPM_USER_CONFIG envs
    pub fn claude_dir(&self) -> PathBuf;              // repo_root/.claude
    pub fn profiles_dir(&self) -> PathBuf;            // repo_root/.claude-profiles
    pub fn state_path(&self) -> PathBuf;              // profiles_dir/.state.json
    pub fn settings_local_path(&self) -> PathBuf;     // .claude/settings.local.json
    pub fn local_md_path(&self) -> PathBuf;           // .claude/local.md
    pub fn claude_md_path(&self) -> PathBuf;          // repo_root/CLAUDE.md
    pub fn agents_dir(&self) -> PathBuf;              // .claude/agents
    pub fn skills_dir(&self) -> PathBuf;              // .claude/skills
}

// profile.rs
pub struct ProfileFile { pub rel: PathBuf, pub contents: Vec<u8> }
pub struct Profile {
    pub name: String,
    pub settings: Option<serde_json::Value>,
    pub claude_md: Option<String>,
    pub mcp_servers: serde_json::Map<String, serde_json::Value>,
    pub agents: Vec<ProfileFile>,
    pub skills: Vec<ProfileFile>,
}
impl Profile {
    pub fn dir(ctx: &Context, name: &str) -> PathBuf;          // profiles_dir/name
    pub fn load(ctx: &Context, name: &str) -> anyhow::Result<Profile>;
}

// manifest.rs
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Manifest { pub files: Vec<PathBuf>, pub mcp_servers: Vec<String> } // files relative to repo_root

// state.rs
#[derive(Default, Serialize, Deserialize)]
pub struct State { pub active: Option<String>, pub manifest: Manifest }
impl State {
    pub fn load(ctx: &Context) -> anyhow::Result<State>;   // default if file missing
    pub fn save(&self, ctx: &Context) -> anyhow::Result<()>;
}

// slots.rs
pub fn atomic_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()>;
pub fn write_json(path: &Path, value: &serde_json::Value) -> anyhow::Result<()>;
pub fn read_json(path: &Path) -> anyhow::Result<serde_json::Value>; // {} if file missing
pub fn remove_if_exists(path: &Path) -> anyhow::Result<()>;

// gitignore.rs
pub fn ensure_ignored(repo_root: &Path, entries: &[&str]) -> anyhow::Result<()>;

// target/mod.rs
pub trait Target {
    fn project(&self, ctx: &Context, profile: &Profile) -> anyhow::Result<Manifest>;
    fn clear(&self, ctx: &Context, manifest: &Manifest) -> anyhow::Result<()>;
}
pub struct ClaudeCodeTarget { pub force: bool }
impl ClaudeCodeTarget { pub fn new(force: bool) -> ClaudeCodeTarget; }
```

**Ordering contract:** `use` always calls `target.clear(old_manifest)` *before* `target.project(profile)`. Foreign-protection inside `project` relies on owned artifacts already being removed, so anything still present at projection time is foreign.

---

### Task 1: Cargo project + Context module

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `src/context.rs`

**Interfaces:**
- Produces: `Context` with `new`, `discover`, and all path accessors listed in the Cross-task API.

- [ ] **Step 1: Create the Cargo manifest**

Create `Cargo.toml`:

```toml
[package]
name = "aipm"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "aipm"
path = "src/main.rs"

[dependencies]
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
dirs = "5"
tempfile = "3"

[dev-dependencies]
assert_cmd = "2"
predicates = "3"
```

- [ ] **Step 2: Write the failing test for Context**

Create `src/context.rs`:

```rust
use std::path::PathBuf;
use std::env;
use anyhow::{anyhow, Result};

pub struct Context {
    pub repo_root: PathBuf,
    pub user_config: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_paths_from_repo_root() {
        let ctx = Context::new(PathBuf::from("/repo"), PathBuf::from("/home/.claude.json"));
        assert_eq!(ctx.claude_dir(), PathBuf::from("/repo/.claude"));
        assert_eq!(ctx.profiles_dir(), PathBuf::from("/repo/.claude-profiles"));
        assert_eq!(ctx.state_path(), PathBuf::from("/repo/.claude-profiles/.state.json"));
        assert_eq!(ctx.settings_local_path(), PathBuf::from("/repo/.claude/settings.local.json"));
        assert_eq!(ctx.local_md_path(), PathBuf::from("/repo/.claude/local.md"));
        assert_eq!(ctx.claude_md_path(), PathBuf::from("/repo/CLAUDE.md"));
        assert_eq!(ctx.agents_dir(), PathBuf::from("/repo/.claude/agents"));
        assert_eq!(ctx.skills_dir(), PathBuf::from("/repo/.claude/skills"));
    }
}
```

Add `mod context;` to a stub `src/main.rs`:

```rust
mod context;
fn main() {}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test context::`
Expected: FAIL — `no method named claude_dir`.

- [ ] **Step 4: Implement Context**

Replace the top of `src/context.rs` (above the `#[cfg(test)]` block) with:

```rust
use std::path::{Path, PathBuf};
use std::env;
use anyhow::{anyhow, Result};

pub struct Context {
    pub repo_root: PathBuf,
    pub user_config: PathBuf,
}

impl Context {
    pub fn new(repo_root: PathBuf, user_config: PathBuf) -> Context {
        Context { repo_root, user_config }
    }

    pub fn discover() -> Result<Context> {
        let repo_root = match env::var_os("AIPM_REPO_ROOT") {
            Some(p) => PathBuf::from(p),
            None => find_repo_root(&env::current_dir()?)?,
        };
        let user_config = match env::var_os("AIPM_USER_CONFIG") {
            Some(p) => PathBuf::from(p),
            None => dirs::home_dir()
                .ok_or_else(|| anyhow!("cannot resolve home directory"))?
                .join(".claude.json"),
        };
        Ok(Context::new(repo_root, user_config))
    }

    pub fn claude_dir(&self) -> PathBuf { self.repo_root.join(".claude") }
    pub fn profiles_dir(&self) -> PathBuf { self.repo_root.join(".claude-profiles") }
    pub fn state_path(&self) -> PathBuf { self.profiles_dir().join(".state.json") }
    pub fn settings_local_path(&self) -> PathBuf { self.claude_dir().join("settings.local.json") }
    pub fn local_md_path(&self) -> PathBuf { self.claude_dir().join("local.md") }
    pub fn claude_md_path(&self) -> PathBuf { self.repo_root.join("CLAUDE.md") }
    pub fn agents_dir(&self) -> PathBuf { self.claude_dir().join("agents") }
    pub fn skills_dir(&self) -> PathBuf { self.claude_dir().join("skills") }
}

fn find_repo_root(start: &Path) -> Result<PathBuf> {
    let mut cur = Some(start);
    while let Some(dir) = cur {
        if dir.join(".git").exists() {
            return Ok(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    Err(anyhow!("not inside a git repository (no .git found); run from a repo or set AIPM_REPO_ROOT"))
}
```

- [ ] **Step 5: Run the test to verify it passes, then commit**

Run: `cargo test context::`
Expected: PASS.

```bash
git add Cargo.toml Cargo.lock src/main.rs src/context.rs
git commit -m "feat: cargo project scaffold and Context path resolution"
```

---

### Task 2: Profile model + loader

**Files:**
- Create: `src/profile.rs`
- Modify: `src/main.rs` (add `mod profile;`)

**Interfaces:**
- Consumes: `Context` (path accessors).
- Produces: `Profile`, `ProfileFile`, `Profile::dir`, `Profile::load`.

- [ ] **Step 1: Write the failing test**

Create `src/profile.rs`:

```rust
use std::fs;
use std::path::{Path, PathBuf};
use anyhow::{Context as _, Result};
use serde_json::{Map, Value};
use crate::context::Context;

pub struct ProfileFile { pub rel: PathBuf, pub contents: Vec<u8> }

pub struct Profile {
    pub name: String,
    pub settings: Option<Value>,
    pub claude_md: Option<String>,
    pub mcp_servers: Map<String, Value>,
    pub agents: Vec<ProfileFile>,
    pub skills: Vec<ProfileFile>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn ctx_for(root: &Path) -> Context {
        Context::new(root.to_path_buf(), root.join("user.json"))
    }

    #[test]
    fn loads_all_parts_when_present() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path().join(".claude-profiles").join("focus");
        fs::create_dir_all(dir.join("agents")).unwrap();
        fs::create_dir_all(dir.join("skills")).unwrap();
        fs::write(dir.join("settings.json"), r#"{"model":"opus"}"#).unwrap();
        fs::write(dir.join("CLAUDE.md"), "be terse").unwrap();
        fs::write(dir.join("mcp.json"), r#"{"db":{"command":"x"}}"#).unwrap();
        fs::write(dir.join("agents").join("rev.md"), "agent body").unwrap();
        fs::write(dir.join("skills").join("s.md"), "skill body").unwrap();

        let p = Profile::load(&ctx_for(tmp.path()), "focus").unwrap();
        assert_eq!(p.name, "focus");
        assert_eq!(p.settings.unwrap()["model"], "opus");
        assert_eq!(p.claude_md.unwrap(), "be terse");
        assert!(p.mcp_servers.contains_key("db"));
        assert_eq!(p.agents.len(), 1);
        assert_eq!(p.agents[0].rel, PathBuf::from("rev.md"));
        assert_eq!(p.skills.len(), 1);
    }

    #[test]
    fn empty_profile_loads_with_all_parts_absent() {
        let tmp = tempdir().unwrap();
        fs::create_dir_all(tmp.path().join(".claude-profiles").join("bare")).unwrap();
        let p = Profile::load(&ctx_for(tmp.path()), "bare").unwrap();
        assert!(p.settings.is_none());
        assert!(p.claude_md.is_none());
        assert!(p.mcp_servers.is_empty());
        assert!(p.agents.is_empty() && p.skills.is_empty());
    }

    #[test]
    fn missing_profile_is_error() {
        let tmp = tempdir().unwrap();
        assert!(Profile::load(&ctx_for(tmp.path()), "nope").is_err());
    }
}
```

Add `mod profile;` to `src/main.rs`.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test profile::`
Expected: FAIL — `Profile::load` not found.

- [ ] **Step 3: Implement the loader**

Insert into `src/profile.rs` above the test module:

```rust
impl Profile {
    pub fn dir(ctx: &Context, name: &str) -> PathBuf {
        ctx.profiles_dir().join(name)
    }

    pub fn load(ctx: &Context, name: &str) -> Result<Profile> {
        let dir = Profile::dir(ctx, name);
        if !dir.is_dir() {
            anyhow::bail!("profile '{}' not found at {}", name, dir.display());
        }

        let settings = read_json_opt(&dir.join("settings.json"))?;

        let claude_md = read_string_opt(&dir.join("CLAUDE.md"))?;

        let mcp_servers = match read_json_opt(&dir.join("mcp.json"))? {
            Some(Value::Object(m)) => m,
            Some(_) => anyhow::bail!("{}/mcp.json must be a JSON object of server definitions", name),
            None => Map::new(),
        };

        let agents = read_tree(&dir.join("agents"))?;
        let skills = read_tree(&dir.join("skills"))?;

        Ok(Profile { name: name.to_string(), settings, claude_md, mcp_servers, agents, skills })
    }
}

fn read_string_opt(path: &Path) -> Result<Option<String>> {
    if path.is_file() {
        Ok(Some(fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?))
    } else {
        Ok(None)
    }
}

fn read_json_opt(path: &Path) -> Result<Option<Value>> {
    match read_string_opt(path)? {
        Some(s) => Ok(Some(serde_json::from_str(&s)
            .with_context(|| format!("parsing {}", path.display()))?)),
        None => Ok(None),
    }
}

fn read_tree(root: &Path) -> Result<Vec<ProfileFile>> {
    let mut out = Vec::new();
    if !root.is_dir() {
        return Ok(out);
    }
    collect(root, root, &mut out)?;
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(out)
}

fn collect(base: &Path, dir: &Path, out: &mut Vec<ProfileFile>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            collect(base, &path, out)?;
        } else {
            let rel = path.strip_prefix(base).unwrap().to_path_buf();
            out.push(ProfileFile { rel, contents: fs::read(&path)? });
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test profile::`
Expected: PASS (all three tests).

- [ ] **Step 5: Commit**

```bash
git add src/profile.rs src/main.rs
git commit -m "feat: profile model and directory loader"
```

---

### Task 3: Manifest + State

**Files:**
- Create: `src/manifest.rs`
- Create: `src/state.rs`
- Modify: `src/main.rs` (add `mod manifest;` and `mod state;`)

**Interfaces:**
- Consumes: `Context`.
- Produces: `Manifest`, `State`, `State::load`, `State::save`.

- [ ] **Step 1: Write the failing tests**

Create `src/manifest.rs`:

```rust
use std::path::PathBuf;
use serde::{Serialize, Deserialize};

#[derive(Default, Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct Manifest {
    #[serde(default)]
    pub files: Vec<PathBuf>,
    #[serde(default)]
    pub mcp_servers: Vec<String>,
}
```

Create `src/state.rs`:

```rust
use std::fs;
use anyhow::{Context as _, Result};
use serde::{Serialize, Deserialize};
use crate::context::Context;
use crate::manifest::Manifest;

#[derive(Default, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub active: Option<String>,
    #[serde(default)]
    pub manifest: Manifest,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn ctx_for(root: &std::path::Path) -> Context {
        Context::new(root.to_path_buf(), root.join("user.json"))
    }

    #[test]
    fn missing_state_loads_default() {
        let tmp = tempdir().unwrap();
        let s = State::load(&ctx_for(tmp.path())).unwrap();
        assert!(s.active.is_none());
        assert!(s.manifest.files.is_empty());
    }

    #[test]
    fn save_then_load_roundtrips() {
        let tmp = tempdir().unwrap();
        let ctx = ctx_for(tmp.path());
        let mut s = State::default();
        s.active = Some("focus".into());
        s.manifest.files.push(PathBuf::from(".claude/settings.local.json"));
        s.manifest.mcp_servers.push("db".into());
        s.save(&ctx).unwrap();

        let loaded = State::load(&ctx).unwrap();
        assert_eq!(loaded.active.as_deref(), Some("focus"));
        assert_eq!(loaded.manifest.files.len(), 1);
        assert_eq!(loaded.manifest.mcp_servers, vec!["db".to_string()]);
    }
}
```

Add `mod manifest;` and `mod state;` to `src/main.rs`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test state::`
Expected: FAIL — `State::load` / `State::save` not found.

- [ ] **Step 3: Implement load/save**

Insert into `src/state.rs` above the test module:

```rust
impl State {
    pub fn load(ctx: &Context) -> Result<State> {
        let path = ctx.state_path();
        if !path.is_file() {
            return Ok(State::default());
        }
        let raw = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn save(&self, ctx: &Context) -> Result<()> {
        let path = ctx.state_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec_pretty(self)?;
        crate::slots::atomic_write(&path, &json)
    }
}
```

Note: `State::save` calls `crate::slots::atomic_write`, defined in Task 4. To keep this task compiling on its own, temporarily implement `save` with `fs::write(&path, json)` and replace it with `atomic_write` in Task 4 Step 5.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test state:: manifest::`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/manifest.rs src/state.rs src/main.rs
git commit -m "feat: manifest and persisted activation state"
```

---

### Task 4: slots primitives + gitignore

**Files:**
- Create: `src/slots.rs`
- Create: `src/gitignore.rs`
- Modify: `src/main.rs` (add `mod slots;` and `mod gitignore;`)
- Modify: `src/state.rs` (switch `save` to `atomic_write`)

**Interfaces:**
- Consumes: nothing project-specific.
- Produces: `slots::atomic_write`, `slots::write_json`, `slots::read_json`, `slots::remove_if_exists`, `gitignore::ensure_ignored`.

- [ ] **Step 1: Write the failing tests**

Create `src/slots.rs`:

```rust
use std::fs;
use std::io::Write;
use std::path::Path;
use anyhow::{anyhow, Context as _, Result};
use serde_json::Value;
use tempfile::NamedTempFile;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn atomic_write_creates_nested_file() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("a/b/c.txt");
        atomic_write(&path, b"hello").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello");
    }

    #[test]
    fn read_json_returns_empty_object_when_missing() {
        let tmp = tempdir().unwrap();
        let v = read_json(&tmp.path().join("nope.json")).unwrap();
        assert!(v.as_object().unwrap().is_empty());
    }

    #[test]
    fn remove_if_exists_is_idempotent() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("x.txt");
        atomic_write(&path, b"y").unwrap();
        remove_if_exists(&path).unwrap();
        remove_if_exists(&path).unwrap(); // second call must not error
        assert!(!path.exists());
    }
}
```

Create `src/gitignore.rs`:

```rust
use std::fs;
use std::path::Path;
use anyhow::{Context as _, Result};

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn adds_each_entry_once_and_is_idempotent() {
        let tmp = tempdir().unwrap();
        ensure_ignored(tmp.path(), &[".claude-profiles/", ".claude/local.md"]).unwrap();
        ensure_ignored(tmp.path(), &[".claude-profiles/", ".claude/local.md"]).unwrap();
        let body = fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        assert_eq!(body.matches(".claude-profiles/").count(), 1);
        assert_eq!(body.matches(".claude/local.md").count(), 1);
    }
}
```

Add `mod slots;` and `mod gitignore;` to `src/main.rs`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test slots:: gitignore::`
Expected: FAIL — functions not found.

- [ ] **Step 3: Implement slots**

Insert into `src/slots.rs` above the test module:

```rust
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| anyhow!("no parent for {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let mut tmp = NamedTempFile::new_in(parent)?;
    tmp.write_all(bytes)?;
    tmp.flush()?;
    tmp.persist(path).map_err(|e| anyhow!("writing {}: {}", path.display(), e))?;
    Ok(())
}

pub fn write_json(path: &Path, value: &Value) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    atomic_write(path, &bytes)
}

pub fn read_json(path: &Path) -> Result<Value> {
    if !path.is_file() {
        return Ok(Value::Object(Default::default()));
    }
    let raw = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    if raw.trim().is_empty() {
        return Ok(Value::Object(Default::default()));
    }
    serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
}

pub fn remove_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("removing {}", path.display())),
    }
}
```

- [ ] **Step 4: Implement gitignore**

Insert into `src/gitignore.rs` above the test module:

```rust
pub fn ensure_ignored(repo_root: &Path, entries: &[&str]) -> Result<()> {
    let path = repo_root.join(".gitignore");
    let mut body = if path.is_file() {
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?
    } else {
        String::new()
    };
    let existing: Vec<&str> = body.lines().map(|l| l.trim()).collect();
    let mut changed = false;
    for entry in entries {
        if !existing.iter().any(|l| l == entry) {
            if !body.is_empty() && !body.ends_with('\n') {
                body.push('\n');
            }
            body.push_str(entry);
            body.push('\n');
            changed = true;
        }
    }
    if changed {
        crate::slots::atomic_write(&path, body.as_bytes())?;
    }
    Ok(())
}
```

- [ ] **Step 5: Switch State::save to atomic_write, then test and commit**

In `src/state.rs`, ensure `save` uses `crate::slots::atomic_write` (replace any temporary `fs::write`).

Run: `cargo test`
Expected: PASS (all tests so far).

```bash
git add src/slots.rs src/gitignore.rs src/main.rs src/state.rs
git commit -m "feat: atomic file primitives and idempotent gitignore management"
```

---

### Task 5: Target trait + ClaudeCodeTarget (settings + CLAUDE.md)

**Files:**
- Create: `src/target/mod.rs`
- Create: `src/target/claude_code.rs`
- Modify: `src/main.rs` (add `mod target;`)

**Interfaces:**
- Consumes: `Context`, `Profile`, `Manifest`, `slots::*`.
- Produces: `Target` trait, `ClaudeCodeTarget::new(force)`, `project`, `clear`. This task handles only the `settings.json` and `CLAUDE.md` parts; MCP (Task 10) and agents/skills (Task 11) extend the same methods.

- [ ] **Step 1: Write the failing tests**

Create `src/target/mod.rs`:

```rust
use anyhow::Result;
use crate::context::Context;
use crate::manifest::Manifest;
use crate::profile::Profile;

pub mod claude_code;
pub use claude_code::ClaudeCodeTarget;

pub trait Target {
    fn project(&self, ctx: &Context, profile: &Profile) -> Result<Manifest>;
    fn clear(&self, ctx: &Context, manifest: &Manifest) -> Result<()>;
}
```

Create `src/target/claude_code.rs`:

```rust
use std::fs;
use std::path::{Path, PathBuf};
use anyhow::{anyhow, Result};
use serde_json::Value;
use crate::context::Context;
use crate::manifest::Manifest;
use crate::profile::Profile;
use crate::slots;
use super::Target;

pub struct ClaudeCodeTarget { pub force: bool }

impl ClaudeCodeTarget {
    pub fn new(force: bool) -> ClaudeCodeTarget { ClaudeCodeTarget { force } }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn ctx_for(root: &Path) -> Context {
        Context::new(root.to_path_buf(), root.join("user.json"))
    }

    fn profile_with(settings: Option<Value>, md: Option<&str>) -> Profile {
        Profile {
            name: "focus".into(),
            settings,
            claude_md: md.map(|s| s.to_string()),
            mcp_servers: Default::default(),
            agents: vec![],
            skills: vec![],
        }
    }

    #[test]
    fn project_writes_settings_and_local_md() {
        let tmp = tempdir().unwrap();
        let ctx = ctx_for(tmp.path());
        let p = profile_with(Some(json!({"model":"opus"})), Some("be terse"));
        let m = ClaudeCodeTarget::new(false).project(&ctx, &p).unwrap();

        assert!(fs::read_to_string(ctx.settings_local_path()).unwrap().contains("opus"));
        assert_eq!(fs::read_to_string(ctx.local_md_path()).unwrap(), "be terse");
        assert!(m.files.contains(&PathBuf::from(".claude/settings.local.json")));
        // local.md is tool-owned and written unconditionally; intentionally NOT in the manifest
        assert!(!m.files.contains(&PathBuf::from(".claude/local.md")));
    }

    #[test]
    fn clear_removes_projected_files() {
        let tmp = tempdir().unwrap();
        let ctx = ctx_for(tmp.path());
        let p = profile_with(Some(json!({"model":"opus"})), Some("hi"));
        let t = ClaudeCodeTarget::new(false);
        let m = t.project(&ctx, &p).unwrap();
        t.clear(&ctx, &m).unwrap();
        assert!(!ctx.settings_local_path().exists());
        assert_eq!(fs::read_to_string(ctx.local_md_path()).unwrap(), ""); // reset, not deleted
    }

    #[test]
    fn foreign_settings_file_is_protected() {
        let tmp = tempdir().unwrap();
        let ctx = ctx_for(tmp.path());
        fs::create_dir_all(ctx.claude_dir()).unwrap();
        fs::write(ctx.settings_local_path(), "{\"hand\":\"made\"}").unwrap();
        let p = profile_with(Some(json!({"model":"opus"})), None);
        let err = ClaudeCodeTarget::new(false).project(&ctx, &p).unwrap_err();
        assert!(err.to_string().contains("settings.local.json"));
    }

    #[test]
    fn force_backs_up_foreign_file() {
        let tmp = tempdir().unwrap();
        let ctx = ctx_for(tmp.path());
        fs::create_dir_all(ctx.claude_dir()).unwrap();
        fs::write(ctx.settings_local_path(), "{\"hand\":\"made\"}").unwrap();
        let p = profile_with(Some(json!({"model":"opus"})), None);
        ClaudeCodeTarget::new(true).project(&ctx, &p).unwrap();
        let bak = ctx.settings_local_path().with_extension("json.bak");
        assert!(bak.exists());
    }
}
```

Add `mod target;` to `src/main.rs`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test target::`
Expected: FAIL — `project`/`clear` not implemented for `ClaudeCodeTarget`.

- [ ] **Step 3: Implement project/clear (settings + CLAUDE.md only)**

Insert into `src/target/claude_code.rs` above the test module:

```rust
impl Target for ClaudeCodeTarget {
    fn project(&self, ctx: &Context, profile: &Profile) -> Result<Manifest> {
        let mut manifest = Manifest::default();

        if let Some(settings) = &profile.settings {
            let path = ctx.settings_local_path();
            self.guard_foreign(&path)?;
            slots::write_json(&path, settings)?;
            manifest.files.push(rel(ctx, &path));
        }

        // local.md is always tool-owned (created by `init`): write the profile's
        // markdown, or empty when it has none. Never guarded, never in the manifest —
        // `clear` resets it rather than deleting, so the committed `@.claude/local.md`
        // import never dangles.
        slots::atomic_write(
            &ctx.local_md_path(),
            profile.claude_md.as_deref().unwrap_or("").as_bytes(),
        )?;

        Ok(manifest)
    }

    fn clear(&self, ctx: &Context, manifest: &Manifest) -> Result<()> {
        for rel_path in &manifest.files {
            slots::remove_if_exists(&ctx.repo_root.join(rel_path))?;
        }
        // reset the tool-owned import target instead of deleting it
        if ctx.local_md_path().exists() {
            slots::atomic_write(&ctx.local_md_path(), b"")?;
        }
        Ok(())
    }
}

impl ClaudeCodeTarget {
    /// A target file present here is foreign (owned files were cleared first).
    /// Protect it unless `force`, in which case back it up to `<name>.bak`.
    fn guard_foreign(&self, path: &Path) -> Result<()> {
        if !path.exists() {
            return Ok(());
        }
        if !self.force {
            return Err(anyhow!(
                "{} already exists and is not managed by aipm; rerun with --force to back it up and overwrite",
                path.display()
            ));
        }
        let mut bak = path.as_os_str().to_owned();
        bak.push(".bak");
        fs::rename(path, PathBuf::from(bak))?;
        Ok(())
    }
}

fn rel(ctx: &Context, path: &Path) -> PathBuf {
    path.strip_prefix(&ctx.repo_root).unwrap_or(path).to_path_buf()
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test target::`
Expected: PASS (all four tests).

- [ ] **Step 5: Commit**

```bash
git add src/target/mod.rs src/target/claude_code.rs src/main.rs
git commit -m "feat: Target trait and ClaudeCodeTarget for settings and CLAUDE.md"
```

---

### Task 6: CLI scaffold + `aipm init`

**Files:**
- Create: `src/cli.rs`
- Rewrite: `src/main.rs` (real dispatch)
- Create: `tests/integration.rs`

**Interfaces:**
- Consumes: `Context`, `gitignore::ensure_ignored`, `slots::atomic_write`.
- Produces: the `clap` `Cli`/`Command` enum and `cli::run`; `cmd_init`.

- [ ] **Step 1: Write the failing integration test**

Create `tests/integration.rs`:

```rust
use assert_cmd::Command;
use tempfile::tempdir;
use std::fs;

fn aipm(repo: &std::path::Path, user_config: &std::path::Path) -> Command {
    let mut c = Command::cargo_bin("aipm").unwrap();
    c.env("AIPM_REPO_ROOT", repo).env("AIPM_USER_CONFIG", user_config);
    c
}

#[test]
fn init_sets_up_gitignore_import_and_default_profile() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    let user = repo.join("user.json");

    aipm(repo, &user).arg("init").assert().success();

    let gi = fs::read_to_string(repo.join(".gitignore")).unwrap();
    assert!(gi.contains(".claude-profiles/"));
    assert!(gi.contains(".claude/settings.local.json"));
    assert!(gi.contains(".claude/local.md"));

    let claude_md = fs::read_to_string(repo.join("CLAUDE.md")).unwrap();
    assert!(claude_md.contains("@.claude/local.md"));

    assert!(repo.join(".claude-profiles/default").is_dir());
    assert!(repo.join(".claude/local.md").is_file()); // placeholder so the import never errors

    // idempotent
    aipm(repo, &user).arg("init").assert().success();
    let claude_md2 = fs::read_to_string(repo.join("CLAUDE.md")).unwrap();
    assert_eq!(claude_md2.matches("@.claude/local.md").count(), 1);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test integration init_sets_up`
Expected: FAIL — no `init` subcommand.

- [ ] **Step 3: Implement the CLI scaffold and `init`**

Create `src/cli.rs`:

```rust
use anyhow::Result;
use clap::{Parser, Subcommand};
use crate::context::Context;
use crate::{gitignore, slots};

#[derive(Parser)]
#[command(name = "aipm", about = "Personal Claude Code profile switcher")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Set up aipm in this repo (gitignore, CLAUDE.md import, default profile)
    Init,
}

const IMPORT_LINE: &str = "@.claude/local.md";
const IGNORES: &[&str] = &[
    ".claude-profiles/",
    ".claude/settings.local.json",
    ".claude/local.md",
];

pub fn run(cli: Cli) -> Result<()> {
    let ctx = Context::discover()?;
    match cli.command {
        Command::Init => cmd_init(&ctx),
    }
}

fn cmd_init(ctx: &Context) -> Result<()> {
    std::fs::create_dir_all(ctx.claude_dir())?;
    std::fs::create_dir_all(default_profile_dir(ctx))?;

    gitignore::ensure_ignored(&ctx.repo_root, IGNORES)?;

    // placeholder so the import target always exists
    if !ctx.local_md_path().exists() {
        slots::atomic_write(&ctx.local_md_path(), b"")?;
    }

    ensure_import(ctx)?;

    println!("aipm initialized. Create a profile with `aipm new <name>`.");
    Ok(())
}

fn default_profile_dir(ctx: &Context) -> std::path::PathBuf {
    ctx.profiles_dir().join("default")
}

fn ensure_import(ctx: &Context) -> Result<()> {
    let path = ctx.claude_md_path();
    let mut body = if path.is_file() { std::fs::read_to_string(&path)? } else { String::new() };
    if body.lines().any(|l| l.trim() == IMPORT_LINE) {
        return Ok(());
    }
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    body.push_str(IMPORT_LINE);
    body.push('\n');
    slots::atomic_write(&path, body.as_bytes())
}
```

Rewrite `src/main.rs`:

```rust
mod context;
mod profile;
mod manifest;
mod state;
mod slots;
mod gitignore;
mod target;
mod cli;

use clap::Parser;

fn main() {
    if let Err(e) = cli::run(cli::Cli::parse()) {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --test integration init_sets_up`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs src/main.rs tests/integration.rs
git commit -m "feat: CLI scaffold and aipm init"
```

---

### Task 7: `aipm new` + `aipm list`

**Files:**
- Modify: `src/cli.rs` (add `New`, `List` variants + handlers)
- Modify: `tests/integration.rs`

**Interfaces:**
- Consumes: `Context`, `Profile::dir`, `State::load`.
- Produces: `cmd_new`, `cmd_list`.

- [ ] **Step 1: Write the failing test**

Append to `tests/integration.rs`:

```rust
#[test]
fn new_creates_profile_and_list_shows_it() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    let user = repo.join("user.json");

    aipm(repo, &user).arg("init").assert().success();
    aipm(repo, &user).args(["new", "focus"]).assert().success();
    assert!(repo.join(".claude-profiles/focus").is_dir());

    // duplicate is rejected
    aipm(repo, &user).args(["new", "focus"]).assert().failure();

    aipm(repo, &user)
        .arg("list")
        .assert()
        .success()
        .stdout(predicates::str::contains("focus"));
}
```

`predicates` is referenced fully-qualified (`predicates::str::contains`), so no `use` import is needed; the dev-dependency is already linked.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test integration new_creates_profile`
Expected: FAIL — no `new`/`list` subcommand.

- [ ] **Step 3: Implement `new` and `list`**

In `src/cli.rs`, extend the `Command` enum:

```rust
    /// Create a new empty profile
    New { name: String },
    /// List profiles (marks the active one)
    List,
```

Extend `run`'s match:

```rust
        Command::New { name } => cmd_new(&ctx, &name),
        Command::List => cmd_list(&ctx),
```

Add handlers:

```rust
use crate::state::State;

fn cmd_new(ctx: &Context, name: &str) -> Result<()> {
    let dir = crate::profile::Profile::dir(ctx, name);
    if dir.exists() {
        anyhow::bail!("profile '{}' already exists", name);
    }
    std::fs::create_dir_all(&dir)?;
    println!("Created profile '{}' at {}", name, dir.display());
    println!("Add settings.json, CLAUDE.md, mcp.json, agents/, or skills/ inside it.");
    Ok(())
}

fn cmd_list(ctx: &Context) -> Result<()> {
    let active = State::load(ctx)?.active;
    let dir = ctx.profiles_dir();
    if !dir.is_dir() {
        println!("No profiles. Run `aipm init`.");
        return Ok(());
    }
    let mut names: Vec<String> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    for name in names {
        let marker = if active.as_deref() == Some(name.as_str()) { "* " } else { "  " };
        println!("{marker}{name}");
    }
    Ok(())
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --test integration new_creates_profile`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs tests/integration.rs
git commit -m "feat: aipm new and aipm list"
```

---

### Task 8: `aipm use` + `aipm deactivate`

**Files:**
- Modify: `src/cli.rs` (add `Use`, `Deactivate` + handlers)
- Modify: `tests/integration.rs`

**Interfaces:**
- Consumes: `Profile::load`, `State::load/save`, `ClaudeCodeTarget::new`, `Target::project/clear`.
- Produces: `cmd_use`, `cmd_deactivate`.

- [ ] **Step 1: Write the failing test**

Append to `tests/integration.rs`:

```rust
fn write_profile_settings(repo: &std::path::Path, name: &str, json: &str) {
    let dir = repo.join(".claude-profiles").join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("settings.json"), json).unwrap();
}

#[test]
fn use_projects_switches_cleanly_and_deactivate_clears() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    let user = repo.join("user.json");

    aipm(repo, &user).arg("init").assert().success();
    write_profile_settings(repo, "focus", r#"{"model":"opus"}"#);
    write_profile_settings(repo, "quick", r#"{"model":"haiku"}"#);

    aipm(repo, &user).args(["use", "focus"]).assert().success();
    let s = fs::read_to_string(repo.join(".claude/settings.local.json")).unwrap();
    assert!(s.contains("opus"));

    // switching replaces, leaves no orphan content from the previous profile
    aipm(repo, &user).args(["use", "quick"]).assert().success();
    let s = fs::read_to_string(repo.join(".claude/settings.local.json")).unwrap();
    assert!(s.contains("haiku") && !s.contains("opus"));

    aipm(repo, &user).arg("deactivate").assert().success();
    assert!(!repo.join(".claude/settings.local.json").exists());

    // committed config untouched: CLAUDE.md still has exactly the import line we added
    let claude_md = fs::read_to_string(repo.join("CLAUDE.md")).unwrap();
    assert_eq!(claude_md.matches("@.claude/local.md").count(), 1);
}

#[test]
fn use_unknown_profile_fails() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    let user = repo.join("user.json");
    aipm(repo, &user).arg("init").assert().success();
    aipm(repo, &user).args(["use", "ghost"]).assert().failure();
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test integration use_projects`
Expected: FAIL — no `use` subcommand.

- [ ] **Step 3: Implement `use` and `deactivate`**

In `src/cli.rs`, extend the `Command` enum:

```rust
    /// Activate a profile (switching from any current one)
    Use {
        name: String,
        /// Back up and overwrite foreign (hand-made) local slot files
        #[arg(long)]
        force: bool,
    },
    /// Remove the active personal overlay, leaving only committed team config
    Deactivate,
```

Extend `run`'s match:

```rust
        Command::Use { name, force } => cmd_use(&ctx, &name, force),
        Command::Deactivate => cmd_deactivate(&ctx),
```

Add handlers:

```rust
use crate::profile::Profile;
use crate::target::{ClaudeCodeTarget, Target};

fn cmd_use(ctx: &Context, name: &str, force: bool) -> Result<()> {
    let profile = Profile::load(ctx, name)?;
    let mut state = State::load(ctx)?;
    let target = ClaudeCodeTarget::new(force);

    target.clear(ctx, &state.manifest)?;       // remove previously projected artifacts (owned only)
    let manifest = target.project(ctx, &profile)?;  // foreign-protection lives in project()

    state.active = Some(name.to_string());
    state.manifest = manifest;
    state.save(ctx)?;

    println!("Activated profile '{name}'.");
    Ok(())
}

fn cmd_deactivate(ctx: &Context) -> Result<()> {
    let mut state = State::load(ctx)?;
    ClaudeCodeTarget::new(false).clear(ctx, &state.manifest)?;
    state.active = None;
    state.manifest = Default::default();
    state.save(ctx)?;
    println!("Deactivated. Committed team config is unchanged.");
    Ok(())
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --test integration`
Expected: PASS (all integration tests so far).

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs tests/integration.rs
git commit -m "feat: aipm use and aipm deactivate with clean switching"
```

---

### Task 9: `aipm status`

**Files:**
- Modify: `src/cli.rs` (add `Status` + handler)
- Modify: `tests/integration.rs`

**Interfaces:**
- Consumes: `State::load`, `Context`.
- Produces: `cmd_status`.

- [ ] **Step 1: Write the failing test**

Append to `tests/integration.rs`:

```rust
#[test]
fn status_reports_active_and_detects_drift() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    let user = repo.join("user.json");

    aipm(repo, &user).arg("init").assert().success();
    write_profile_settings(repo, "focus", r#"{"model":"opus"}"#);
    aipm(repo, &user).args(["use", "focus"]).assert().success();

    aipm(repo, &user)
        .arg("status")
        .assert()
        .success()
        .stdout(predicates::str::contains("focus"))
        .stdout(predicates::str::contains(".claude/settings.local.json"));

    // delete an owned file out from under the tool -> drift reported
    fs::remove_file(repo.join(".claude/settings.local.json")).unwrap();
    aipm(repo, &user)
        .arg("status")
        .assert()
        .success()
        .stdout(predicates::str::contains("missing"));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test integration status_reports`
Expected: FAIL — no `status` subcommand.

- [ ] **Step 3: Implement `status`**

In `src/cli.rs`, extend the `Command` enum and `run` match:

```rust
    /// Show the active profile and what is projected (with a drift check)
    Status,
```

```rust
        Command::Status => cmd_status(&ctx),
```

Add handler:

```rust
fn cmd_status(ctx: &Context) -> Result<()> {
    let state = State::load(ctx)?;
    match &state.active {
        Some(name) => println!("Active profile: {name}"),
        None => { println!("No active profile."); return Ok(()); }
    }
    println!("Projected files:");
    for rel in &state.manifest.files {
        let present = ctx.repo_root.join(rel).exists();
        let tag = if present { "ok" } else { "missing (drift)" };
        println!("  {} [{}]", rel.display(), tag);
    }
    if !state.manifest.mcp_servers.is_empty() {
        println!("Projected MCP servers: {}", state.manifest.mcp_servers.join(", "));
    }
    Ok(())
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --test integration status_reports`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs tests/integration.rs
git commit -m "feat: aipm status with drift detection"
```

---

### Task 10: ClaudeCodeTarget — MCP servers

**Files:**
- Modify: `src/target/claude_code.rs` (extend `project`/`clear` for MCP)

**Interfaces:**
- Consumes: `slots::read_json`, `slots::write_json`, `Profile::mcp_servers`, `Context::user_config`, `Manifest::mcp_servers`.
- Produces: MCP handling inside the existing `project`/`clear`. MCP servers are written to `user_config` under `projects.<repo_root>.mcpServers`.

- [ ] **Step 1: Write the failing tests**

Add to the test module in `src/target/claude_code.rs`:

```rust
    fn profile_with_mcp(servers: Value) -> Profile {
        Profile {
            name: "focus".into(),
            settings: None,
            claude_md: None,
            mcp_servers: servers.as_object().unwrap().clone(),
            agents: vec![],
            skills: vec![],
        }
    }

    #[test]
    fn project_registers_local_scope_mcp_under_project_key() {
        let tmp = tempdir().unwrap();
        let ctx = ctx_for(tmp.path());
        let p = profile_with_mcp(json!({"db": {"command": "run-db"}}));
        let m = ClaudeCodeTarget::new(false).project(&ctx, &p).unwrap();

        let cfg: Value = serde_json::from_str(&fs::read_to_string(&ctx.user_config).unwrap()).unwrap();
        let key = ctx.repo_root.to_string_lossy().to_string();
        assert_eq!(cfg["projects"][&key]["mcpServers"]["db"]["command"], "run-db");
        assert!(m.mcp_servers.contains(&"db".to_string()));
    }

    #[test]
    fn clear_removes_only_owned_mcp_servers() {
        let tmp = tempdir().unwrap();
        let ctx = ctx_for(tmp.path());
        let key = ctx.repo_root.to_string_lossy().to_string();
        // pre-existing foreign server the tool must not touch
        let pre = json!({"projects": {&key: {"mcpServers": {"keep": {"command": "x"}}}}});
        fs::write(&ctx.user_config, serde_json::to_string(&pre).unwrap()).unwrap();

        let t = ClaudeCodeTarget::new(false);
        let p = profile_with_mcp(json!({"db": {"command": "run-db"}}));
        let m = t.project(&ctx, &p).unwrap();
        t.clear(&ctx, &m).unwrap();

        let cfg: Value = serde_json::from_str(&fs::read_to_string(&ctx.user_config).unwrap()).unwrap();
        assert!(cfg["projects"][&key]["mcpServers"]["keep"].is_object());
        assert!(cfg["projects"][&key]["mcpServers"]["db"].is_null());
    }

    #[test]
    fn foreign_mcp_name_collision_is_protected() {
        let tmp = tempdir().unwrap();
        let ctx = ctx_for(tmp.path());
        let key = ctx.repo_root.to_string_lossy().to_string();
        let pre = json!({"projects": {&key: {"mcpServers": {"db": {"command": "theirs"}}}}});
        fs::write(&ctx.user_config, serde_json::to_string(&pre).unwrap()).unwrap();

        let p = profile_with_mcp(json!({"db": {"command": "ours"}}));
        let err = ClaudeCodeTarget::new(false).project(&ctx, &p).unwrap_err();
        assert!(err.to_string().contains("db"));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test target::claude_code::tests::project_registers`
Expected: FAIL — MCP not handled.

- [ ] **Step 3: Extend project/clear with MCP handling**

In `src/target/claude_code.rs`, add to the end of `project` (before `Ok(manifest)`):

```rust
        if !profile.mcp_servers.is_empty() {
            let mut cfg = slots::read_json(&ctx.user_config)?;
            let servers = mcp_servers_mut(&mut cfg, &ctx.repo_root);
            for (name, def) in &profile.mcp_servers {
                if servers.contains_key(name) && !self.force {
                    return Err(anyhow!(
                        "MCP server '{}' already exists in {} and is not managed by aipm; rerun with --force to overwrite",
                        name, ctx.user_config.display()
                    ));
                }
                servers.insert(name.clone(), def.clone());
                manifest.mcp_servers.push(name.clone());
            }
            slots::write_json(&ctx.user_config, &cfg)?;
        }
```

Add to `clear`, before `Ok(())`:

```rust
        if !manifest.mcp_servers.is_empty() {
            let mut cfg = slots::read_json(&ctx.user_config)?;
            {
                let servers = mcp_servers_mut(&mut cfg, &ctx.repo_root);
                for name in &manifest.mcp_servers {
                    servers.remove(name);
                }
            }
            slots::write_json(&ctx.user_config, &cfg)?;
        }
```

Add this helper at the bottom of the file (module scope):

```rust
fn mcp_servers_mut<'a>(cfg: &'a mut Value, repo_root: &Path) -> &'a mut serde_json::Map<String, Value> {
    let key = repo_root.to_string_lossy().to_string();
    cfg.as_object_mut()
        .unwrap()
        .entry("projects").or_insert_with(|| Value::Object(Default::default()))
        .as_object_mut().unwrap()
        .entry(key).or_insert_with(|| Value::Object(Default::default()))
        .as_object_mut().unwrap()
        .entry("mcpServers").or_insert_with(|| Value::Object(Default::default()))
        .as_object_mut().unwrap()
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test target::claude_code`
Expected: PASS (all target tests, old and new).

- [ ] **Step 5: Commit**

```bash
git add src/target/claude_code.rs
git commit -m "feat: project personal MCP servers into local scope with collision protection"
```

---

### Task 11: ClaudeCodeTarget — agents & skills

**Files:**
- Modify: `src/target/claude_code.rs` (extend `project` for agents/skills + gitignore)

**Interfaces:**
- Consumes: `Profile::agents/skills` (`ProfileFile`), `Context::agents_dir/skills_dir`, `gitignore::ensure_ignored`, `slots::atomic_write`.
- Produces: agent/skill file projection inside the existing `project`/`clear` (cleared via `manifest.files`, already handled by Task 5's `clear`).

- [ ] **Step 1: Write the failing tests**

Add to the test module in `src/target/claude_code.rs`:

```rust
    fn profile_with_agent(rel: &str, body: &str) -> Profile {
        Profile {
            name: "focus".into(),
            settings: None,
            claude_md: None,
            mcp_servers: Default::default(),
            agents: vec![ProfileFile { rel: PathBuf::from(rel), contents: body.as_bytes().to_vec() }],
            skills: vec![],
        }
    }

    #[test]
    fn project_drops_agent_files_and_gitignores_them() {
        let tmp = tempdir().unwrap();
        let ctx = ctx_for(tmp.path());
        let p = profile_with_agent("rev.md", "agent body");
        let m = ClaudeCodeTarget::new(false).project(&ctx, &p).unwrap();

        assert_eq!(fs::read_to_string(ctx.agents_dir().join("rev.md")).unwrap(), "agent body");
        assert!(m.files.contains(&PathBuf::from(".claude/agents/rev.md")));
        let gi = fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        assert!(gi.contains(".claude/agents/rev.md"));
    }

    #[test]
    fn switching_removes_previous_agent_file() {
        let tmp = tempdir().unwrap();
        let ctx = ctx_for(tmp.path());
        let t = ClaudeCodeTarget::new(false);
        let m = t.project(&ctx, &profile_with_agent("rev.md", "body")).unwrap();
        t.clear(&ctx, &m).unwrap();
        assert!(!ctx.agents_dir().join("rev.md").exists());
    }

    #[test]
    fn foreign_agent_file_is_protected() {
        let tmp = tempdir().unwrap();
        let ctx = ctx_for(tmp.path());
        fs::create_dir_all(ctx.agents_dir()).unwrap();
        fs::write(ctx.agents_dir().join("rev.md"), "committed team agent").unwrap();
        let err = ClaudeCodeTarget::new(false)
            .project(&ctx, &profile_with_agent("rev.md", "mine"))
            .unwrap_err();
        assert!(err.to_string().contains("rev.md"));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test target::claude_code::tests::project_drops_agent`
Expected: FAIL — agents not handled.

- [ ] **Step 3: Extend project with agents/skills**

In `src/target/claude_code.rs`, add to `project` (before the MCP block):

```rust
        self.drop_tree(ctx, &profile.agents, &ctx.agents_dir(), &mut manifest)?;
        self.drop_tree(ctx, &profile.skills, &ctx.skills_dir(), &mut manifest)?;
```

Add this method to the `impl ClaudeCodeTarget` block:

```rust
    fn drop_tree(
        &self,
        ctx: &Context,
        files: &[crate::profile::ProfileFile],
        dest_root: &Path,
        manifest: &mut Manifest,
    ) -> Result<()> {
        for f in files {
            let dest = dest_root.join(&f.rel);
            self.guard_foreign(&dest)?;
            slots::atomic_write(&dest, &f.contents)?;
            let rel_path = rel(ctx, &dest);
            let ignore = rel_path.to_string_lossy().replace('\\', "/");
            crate::gitignore::ensure_ignored(&ctx.repo_root, &[ignore.as_str()])?;
            manifest.files.push(rel_path);
        }
        Ok(())
    }
```

(No change to `clear` — agent/skill files are in `manifest.files` and are already removed by the existing loop.)

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test target::claude_code`
Expected: PASS (all target tests).

- [ ] **Step 5: Commit**

```bash
git add src/target/claude_code.rs
git commit -m "feat: project personal agents and skills with cleanup and protection"
```

---

### Task 12: `aipm edit` + `aipm remove`

**Files:**
- Modify: `src/cli.rs` (add `Edit`, `Remove` + handlers)
- Modify: `tests/integration.rs`

**Interfaces:**
- Consumes: `Profile::dir`, `State::load`, `ClaudeCodeTarget::clear`.
- Produces: `cmd_edit`, `cmd_remove`.

- [ ] **Step 1: Write the failing tests**

Append to `tests/integration.rs`:

```rust
#[test]
fn remove_deletes_profile_and_clears_if_active() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    let user = repo.join("user.json");

    aipm(repo, &user).arg("init").assert().success();
    write_profile_settings(repo, "focus", r#"{"model":"opus"}"#);
    aipm(repo, &user).args(["use", "focus"]).assert().success();

    aipm(repo, &user).args(["remove", "focus"]).assert().success();
    assert!(!repo.join(".claude-profiles/focus").exists());
    // it was active, so its projection is gone too
    assert!(!repo.join(".claude/settings.local.json").exists());

    aipm(repo, &user).args(["remove", "ghost"]).assert().failure();
}

#[test]
fn edit_resolves_path_for_existing_profile() {
    let tmp = tempdir().unwrap();
    let repo = tmp.path();
    let user = repo.join("user.json");
    aipm(repo, &user).arg("init").assert().success();
    aipm(repo, &user).args(["new", "focus"]).assert().success();
    aipm(repo, &user)
        .args(["edit", "focus", "--print-path"])
        .assert()
        .success()
        .stdout(predicates::str::contains(".claude-profiles"));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test integration remove_deletes`
Expected: FAIL — no `remove`/`edit` subcommand.

- [ ] **Step 3: Implement `edit` and `remove`**

In `src/cli.rs`, extend the `Command` enum and `run` match:

```rust
    /// Open a profile directory in $EDITOR (or print its path)
    Edit {
        name: String,
        /// Print the profile directory path instead of launching an editor
        #[arg(long)]
        print_path: bool,
    },
    /// Delete a profile (deactivating it first if it is active)
    Remove { name: String },
```

```rust
        Command::Edit { name, print_path } => cmd_edit(&ctx, &name, print_path),
        Command::Remove { name } => cmd_remove(&ctx, &name),
```

Add handlers:

```rust
fn cmd_edit(ctx: &Context, name: &str, print_path: bool) -> Result<()> {
    let dir = Profile::dir(ctx, name);
    if !dir.is_dir() {
        anyhow::bail!("profile '{}' not found", name);
    }
    if print_path {
        println!("{}", dir.display());
        return Ok(());
    }
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    std::process::Command::new(editor).arg(&dir).status()?;
    Ok(())
}

fn cmd_remove(ctx: &Context, name: &str) -> Result<()> {
    let dir = Profile::dir(ctx, name);
    if !dir.is_dir() {
        anyhow::bail!("profile '{}' not found", name);
    }
    let mut state = State::load(ctx)?;
    if state.active.as_deref() == Some(name) {
        ClaudeCodeTarget::new(false).clear(ctx, &state.manifest)?;
        state.active = None;
        state.manifest = Default::default();
        state.save(ctx)?;
    }
    std::fs::remove_dir_all(&dir)?;
    println!("Removed profile '{}'.", name);
    Ok(())
}
```

- [ ] **Step 4: Run the full suite to verify everything passes**

Run: `cargo test`
Expected: PASS (all unit + integration tests).

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs tests/integration.rs
git commit -m "feat: aipm edit and aipm remove"
```

---

### Task 13: Cross-platform CI workflow

**Files:**
- Create: `.github/workflows/ci.yml`

**Interfaces:** none (CI config). Satisfies the spec's cross-platform CI matrix requirement.

- [ ] **Step 1: Write the workflow**

Create `.github/workflows/ci.yml`:

```yaml
name: ci
on:
  push:
    branches: [main]
  pull_request:
jobs:
  test:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
      - run: cargo fmt --all -- --check
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo test --all
```

- [ ] **Step 2: Verify locally before relying on CI**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test --all`
Expected: all three succeed (fmt clean, no clippy warnings, all tests pass).

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: cross-platform test matrix (linux/macos/windows)"
```

---

## Final verification

- [ ] Run `cargo test` — all unit and integration tests pass.
- [ ] Run `cargo clippy -- -D warnings` and fix any lints.
- [ ] Run `cargo fmt`.
- [ ] Manually verify end-to-end in a scratch repo: `aipm init`, create a profile with each artifact type, `aipm use`, inspect `.claude/`, switch, `aipm deactivate`, confirm committed files untouched.
- [ ] Commit any fmt/clippy fixes.

## Notes for the implementer

- **Verify against real Claude Code before relying on it in anger** (these are the spec's open items; they affect runtime behavior, not the tests, which are self-contained): (a) `settings.local.json` overrides `settings.json`; (b) a `@import` to a missing file is skipped — Task 6 writes a placeholder `local.md` so this never bites; (c) `local`-scope MCP servers live in `~/.claude.json` under `projects.<abs-path>.mcpServers`; (d) Claude Code discovers agents/skills under `.claude/agents` and `.claude/skills`.
- The `AIPM_REPO_ROOT` / `AIPM_USER_CONFIG` env overrides exist so every test runs against temp dirs and never touches the real `~/.claude.json`. They are also handy for users who want non-default locations.
- `clear`-before-`project` ordering is load-bearing for foreign protection. Keep it in `cmd_use`.
- `.claude/local.md` is unconditionally tool-owned from `init` onward: `project` always rewrites it (profile markdown or empty), `clear` resets it to empty, and it is never in the manifest nor subject to `guard_foreign`. This is what lets the committed `@.claude/local.md` import always resolve.
- **Partial-projection failure is surfaced, not rolled back.** Per the spec, full rollback is only "where feasible"; this design instead makes `project` perform each artifact's foreign check immediately before its own write and saves `State` only after `project` returns `Ok`. If a write fails mid-way, the half-applied state is reported by `aipm status`'s drift check (owned files present vs. missing). If you want stronger atomicity later, hoist all `guard_foreign` checks into a pre-flight pass before any write.
