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
