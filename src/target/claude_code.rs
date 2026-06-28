use super::Target;
use crate::context::Context;
use crate::manifest::Manifest;
use crate::profile::Profile;
use crate::slots;
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub struct ClaudeCodeTarget {
    pub force: bool,
}

impl ClaudeCodeTarget {
    pub fn new(force: bool) -> ClaudeCodeTarget {
        ClaudeCodeTarget { force }
    }
}

impl Target for ClaudeCodeTarget {
    fn project(&self, ctx: &Context, profile: &Profile) -> Result<Manifest> {
        let mut manifest = Manifest::default();
        match self.project_inner(ctx, profile, &mut manifest) {
            Ok(()) => Ok(manifest),
            Err(e) => {
                // Best-effort rollback: undo whatever was written before the failure.
                // `manifest` precisely tracks files and MCP entries committed so far.
                match self.rollback_partial(ctx, &manifest) {
                    Ok(()) => Err(e),
                    Err(rb_err) => Err(anyhow!(
                        "projection failed: {e:#}\n\
                         Rollback also failed ({rb_err:#}); partial state left on disk:\n  \
                         files: [{}]\n  MCP servers: [{}]",
                        manifest
                            .files
                            .iter()
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", "),
                        manifest.mcp_servers.join(", "),
                    )),
                }
            }
        }
    }

    fn clear(&self, ctx: &Context, manifest: &Manifest) -> Result<()> {
        for rel_path in &manifest.files {
            slots::remove_if_exists(&ctx.repo_root.join(rel_path))?;
        }
        // reset the tool-owned import target instead of deleting it
        if ctx.local_md_path().exists() {
            slots::atomic_write(&ctx.local_md_path(), b"")?;
        }
        if !manifest.mcp_servers.is_empty() {
            let mut cfg = slots::read_json(&ctx.user_config)?;
            {
                let servers = mcp_servers_mut(&mut cfg, &ctx.repo_root)?;
                for name in &manifest.mcp_servers {
                    servers.remove(name);
                }
            }
            slots::write_json(&ctx.user_config, &cfg)?;
        }
        Ok(())
    }
}

impl ClaudeCodeTarget {
    /// Inner body of `project`: writes artifacts and accumulates `manifest` as it
    /// goes. Callers must call `rollback_partial` on the accumulated manifest when
    /// this returns `Err`, so that no orphan files are left behind.
    fn project_inner(
        &self,
        ctx: &Context,
        profile: &Profile,
        manifest: &mut Manifest,
    ) -> Result<()> {
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

        self.drop_tree(ctx, &profile.agents, &ctx.agents_dir(), manifest)?;
        self.drop_tree(ctx, &profile.skills, &ctx.skills_dir(), manifest)?;

        if !profile.mcp_servers.is_empty() {
            let mut cfg = slots::read_json(&ctx.user_config)?;
            let servers = mcp_servers_mut(&mut cfg, &ctx.repo_root)?;
            for (name, def) in &profile.mcp_servers {
                if servers.contains_key(name) {
                    if !self.force {
                        return Err(anyhow!(
                            "MCP server '{}' already exists in {} and is not managed by aipm; rerun with --force to back it up and overwrite",
                            name, ctx.user_config.display()
                        ));
                    }
                    // --force: preserve the foreign definition before replacing it.
                    let old = servers.get(name).cloned().unwrap_or(Value::Null);
                    backup_foreign_mcp(ctx, name, &old)?;
                }
                servers.insert(name.clone(), def.clone());
                manifest.mcp_servers.push(name.clone());
            }
            slots::write_json(&ctx.user_config, &cfg)?;
        }

        Ok(())
    }

    /// Undo the partial writes recorded in `partial` after a failed `project_inner`.
    /// Mirrors what `clear` does, but operates only on what was written so far.
    fn rollback_partial(&self, ctx: &Context, partial: &Manifest) -> Result<()> {
        for rel_path in &partial.files {
            slots::remove_if_exists(&ctx.repo_root.join(rel_path))?;
        }
        // local.md is always written unconditionally by project_inner; reset it.
        if ctx.local_md_path().exists() {
            slots::atomic_write(&ctx.local_md_path(), b"")?;
        }
        if !partial.mcp_servers.is_empty() {
            let mut cfg = slots::read_json(&ctx.user_config)?;
            {
                let servers = mcp_servers_mut(&mut cfg, &ctx.repo_root)?;
                for name in &partial.mcp_servers {
                    servers.remove(name);
                }
            }
            slots::write_json(&ctx.user_config, &cfg)?;
        }
        Ok(())
    }

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
    path.strip_prefix(&ctx.repo_root)
        .unwrap_or(path)
        .to_path_buf()
}

fn mcp_servers_mut<'a>(
    cfg: &'a mut Value,
    repo_root: &Path,
) -> Result<&'a mut serde_json::Map<String, Value>> {
    let key = repo_root.to_string_lossy().to_string();
    let root = cfg
        .as_object_mut()
        .ok_or_else(|| anyhow!("user config root is not a JSON object"))?;
    let projects = root
        .entry("projects")
        .or_insert_with(|| Value::Object(Default::default()))
        .as_object_mut()
        .ok_or_else(|| anyhow!("user config `projects` is not a JSON object"))?;
    let project = projects
        .entry(key)
        .or_insert_with(|| Value::Object(Default::default()))
        .as_object_mut()
        .ok_or_else(|| anyhow!("user config project entry is not a JSON object"))?;
    let servers = project
        .entry("mcpServers")
        .or_insert_with(|| Value::Object(Default::default()))
        .as_object_mut()
        .ok_or_else(|| anyhow!("user config `mcpServers` is not a JSON object"))?;
    Ok(servers)
}

/// Append a displaced foreign MCP server definition to a gitignored backup file so
/// `--force` never destroys unowned config. Keyed by server name -> array of displaced
/// definitions (newest appended), so repeated overwrites never lose an earlier backup.
fn backup_foreign_mcp(ctx: &Context, name: &str, old: &Value) -> Result<()> {
    let path = ctx.profiles_dir().join(".mcp-backups.json");
    let mut backups = slots::read_json(&path)?; // {} if missing
    let obj = backups
        .as_object_mut()
        .ok_or_else(|| anyhow!("{} is not a JSON object", path.display()))?;
    let entry = obj
        .entry(name.to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let arr = entry
        .as_array_mut()
        .ok_or_else(|| anyhow!("{} entry for '{}' is not an array", path.display(), name))?;
    arr.push(old.clone());
    slots::write_json(&path, &backups)
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

        assert!(fs::read_to_string(ctx.settings_local_path())
            .unwrap()
            .contains("opus"));
        assert_eq!(fs::read_to_string(ctx.local_md_path()).unwrap(), "be terse");
        assert!(m
            .files
            .contains(&PathBuf::from(".claude/settings.local.json")));
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

        let cfg: Value =
            serde_json::from_str(&fs::read_to_string(&ctx.user_config).unwrap()).unwrap();
        let key = ctx.repo_root.to_string_lossy().to_string();
        assert_eq!(
            cfg["projects"][&key]["mcpServers"]["db"]["command"],
            "run-db"
        );
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

        let cfg: Value =
            serde_json::from_str(&fs::read_to_string(&ctx.user_config).unwrap()).unwrap();
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

    #[test]
    fn force_backs_up_foreign_mcp_server() {
        let tmp = tempdir().unwrap();
        let ctx = ctx_for(tmp.path());
        let key = ctx.repo_root.to_string_lossy().to_string();
        let pre = json!({"projects": {&key: {"mcpServers": {"db": {"command": "theirs"}}}}});
        fs::write(&ctx.user_config, serde_json::to_string(&pre).unwrap()).unwrap();

        let p = profile_with_mcp(json!({"db": {"command": "ours"}}));
        ClaudeCodeTarget::new(true).project(&ctx, &p).unwrap();

        // live config now has our value
        let cfg: Value =
            serde_json::from_str(&fs::read_to_string(&ctx.user_config).unwrap()).unwrap();
        assert_eq!(cfg["projects"][&key]["mcpServers"]["db"]["command"], "ours");

        // the displaced foreign definition is preserved in the backup file
        let backups: Value = serde_json::from_str(
            &fs::read_to_string(ctx.profiles_dir().join(".mcp-backups.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(backups["db"][0]["command"], "theirs");
    }

    #[test]
    fn malformed_user_config_structure_errors_cleanly() {
        let tmp = tempdir().unwrap();
        let ctx = ctx_for(tmp.path());
        // valid JSON, but the wrong shape (array, not object)
        fs::write(&ctx.user_config, "[]").unwrap();

        let p = profile_with_mcp(json!({"db": {"command": "run-db"}}));
        let err = ClaudeCodeTarget::new(false).project(&ctx, &p).unwrap_err();
        assert!(err.to_string().contains("not a JSON object"));
        // file must be left untouched (not corrupted)
        assert_eq!(fs::read_to_string(&ctx.user_config).unwrap(), "[]");
    }

    fn profile_with_agent(rel: &str, body: &str) -> Profile {
        Profile {
            name: "focus".into(),
            settings: None,
            claude_md: None,
            mcp_servers: Default::default(),
            agents: vec![crate::profile::ProfileFile {
                rel: PathBuf::from(rel),
                contents: body.as_bytes().to_vec(),
            }],
            skills: vec![],
        }
    }

    #[test]
    fn project_drops_agent_files_and_gitignores_them() {
        let tmp = tempdir().unwrap();
        let ctx = ctx_for(tmp.path());
        let p = profile_with_agent("rev.md", "agent body");
        let m = ClaudeCodeTarget::new(false).project(&ctx, &p).unwrap();

        assert_eq!(
            fs::read_to_string(ctx.agents_dir().join("rev.md")).unwrap(),
            "agent body"
        );
        assert!(m.files.contains(&PathBuf::from(".claude/agents/rev.md")));
        let gi = fs::read_to_string(tmp.path().join(".gitignore")).unwrap();
        assert!(gi.contains(".claude/agents/rev.md"));
    }

    #[test]
    fn switching_removes_previous_agent_file() {
        let tmp = tempdir().unwrap();
        let ctx = ctx_for(tmp.path());
        let t = ClaudeCodeTarget::new(false);
        let m = t
            .project(&ctx, &profile_with_agent("rev.md", "body"))
            .unwrap();
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

    // --- rollback regression tests ---

    /// If project() fails mid-way (e.g. foreign MCP collision after writing settings),
    /// the earlier partial writes must be rolled back so that a subsequent non-force
    /// `use` does not trip on orphan files.
    #[test]
    fn partial_project_is_rolled_back_on_failure() {
        let tmp = tempdir().unwrap();
        let ctx = ctx_for(tmp.path());
        let key = ctx.repo_root.to_string_lossy().to_string();

        // Pre-existing foreign MCP entry that will cause project() to fail after it
        // has already written settings.local.json and local.md.
        let pre = json!({"projects": {&key: {"mcpServers": {"db": {"command": "theirs"}}}}});
        fs::write(&ctx.user_config, serde_json::to_string(&pre).unwrap()).unwrap();

        // A profile that has settings *and* an MCP server whose name collides.
        let profile = Profile {
            name: "focus".into(),
            settings: Some(json!({"model": "opus"})),
            claude_md: Some("hello".into()),
            mcp_servers: json!({"db": {"command": "ours"}})
                .as_object()
                .unwrap()
                .clone(),
            agents: vec![],
            skills: vec![],
        };

        let err = ClaudeCodeTarget::new(false)
            .project(&ctx, &profile)
            .unwrap_err();
        // The original collision error is surfaced unchanged.
        assert!(err.to_string().contains("db"));

        // settings.local.json written before the failure must have been removed.
        assert!(
            !ctx.settings_local_path().exists(),
            "settings.local.json should be rolled back"
        );
        // local.md must be reset to empty (not left with profile content).
        let local_md = fs::read_to_string(ctx.local_md_path()).unwrap_or_default();
        assert_eq!(
            local_md, "",
            "local.md should be reset to empty after rollback"
        );
        // The foreign MCP entry must still be intact.
        let cfg: Value =
            serde_json::from_str(&fs::read_to_string(&ctx.user_config).unwrap()).unwrap();
        assert_eq!(
            cfg["projects"][&key]["mcpServers"]["db"]["command"], "theirs",
            "foreign MCP entry must not be touched by rollback"
        );
    }

    /// After a rolled-back failure, a second `use` (without force) must succeed
    /// because no orphan files remain on disk.
    #[test]
    fn use_succeeds_after_rolled_back_failure() {
        let tmp = tempdir().unwrap();
        let ctx = ctx_for(tmp.path());
        let key = ctx.repo_root.to_string_lossy().to_string();

        // Trigger a failure: foreign MCP collision.
        let pre = json!({"projects": {&key: {"mcpServers": {"db": {"command": "theirs"}}}}});
        fs::write(&ctx.user_config, serde_json::to_string(&pre).unwrap()).unwrap();

        let failing = Profile {
            name: "focus".into(),
            settings: Some(json!({"model": "opus"})),
            claude_md: None,
            mcp_servers: json!({"db": {"command": "ours"}})
                .as_object()
                .unwrap()
                .clone(),
            agents: vec![],
            skills: vec![],
        };
        assert!(
            ClaudeCodeTarget::new(false)
                .project(&ctx, &failing)
                .is_err(),
            "first project should fail"
        );

        // Now try a different profile that does NOT use the colliding MCP name.
        let ok_profile = Profile {
            name: "clean".into(),
            settings: Some(json!({"model": "sonnet"})),
            claude_md: None,
            mcp_servers: Default::default(),
            agents: vec![],
            skills: vec![],
        };
        let manifest = ClaudeCodeTarget::new(false)
            .project(&ctx, &ok_profile)
            .expect("second project must succeed — no orphan files from the rollback");
        assert!(manifest
            .files
            .contains(&PathBuf::from(".claude/settings.local.json")));
    }
}
