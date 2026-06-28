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

        self.drop_tree(ctx, &profile.agents, &ctx.agents_dir(), &mut manifest)?;
        self.drop_tree(ctx, &profile.skills, &ctx.skills_dir(), &mut manifest)?;

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
        Ok(())
    }
}

impl ClaudeCodeTarget {
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
    path.strip_prefix(&ctx.repo_root).unwrap_or(path).to_path_buf()
}

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

    fn profile_with_agent(rel: &str, body: &str) -> Profile {
        Profile {
            name: "focus".into(),
            settings: None,
            claude_md: None,
            mcp_servers: Default::default(),
            agents: vec![crate::profile::ProfileFile { rel: PathBuf::from(rel), contents: body.as_bytes().to_vec() }],
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
}
