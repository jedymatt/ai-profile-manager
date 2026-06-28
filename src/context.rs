use anyhow::{anyhow, Result};
use std::env;
use std::path::{Path, PathBuf};

pub struct Context {
    pub repo_root: PathBuf,
    pub user_config: PathBuf,
}

impl Context {
    pub fn new(repo_root: PathBuf, user_config: PathBuf) -> Context {
        Context {
            repo_root,
            user_config,
        }
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

    pub fn claude_dir(&self) -> PathBuf {
        self.repo_root.join(".claude")
    }
    pub fn profiles_dir(&self) -> PathBuf {
        self.repo_root.join(".claude-profiles")
    }
    pub fn state_path(&self) -> PathBuf {
        self.profiles_dir().join(".state.json")
    }
    pub fn settings_local_path(&self) -> PathBuf {
        self.claude_dir().join("settings.local.json")
    }
    pub fn local_md_path(&self) -> PathBuf {
        self.claude_dir().join("local.md")
    }
    pub fn claude_md_path(&self) -> PathBuf {
        self.repo_root.join("CLAUDE.md")
    }
    pub fn agents_dir(&self) -> PathBuf {
        self.claude_dir().join("agents")
    }
    pub fn skills_dir(&self) -> PathBuf {
        self.claude_dir().join("skills")
    }
}

fn find_repo_root(start: &Path) -> Result<PathBuf> {
    let mut cur = Some(start);
    while let Some(dir) = cur {
        if dir.join(".git").exists() {
            return Ok(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    Err(anyhow!(
        "not inside a git repository (no .git found); run from a repo or set AIPM_REPO_ROOT"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_paths_from_repo_root() {
        let ctx = Context::new(PathBuf::from("/repo"), PathBuf::from("/home/.claude.json"));
        assert_eq!(ctx.claude_dir(), PathBuf::from("/repo/.claude"));
        assert_eq!(ctx.profiles_dir(), PathBuf::from("/repo/.claude-profiles"));
        assert_eq!(
            ctx.state_path(),
            PathBuf::from("/repo/.claude-profiles/.state.json")
        );
        assert_eq!(
            ctx.settings_local_path(),
            PathBuf::from("/repo/.claude/settings.local.json")
        );
        assert_eq!(ctx.local_md_path(), PathBuf::from("/repo/.claude/local.md"));
        assert_eq!(ctx.claude_md_path(), PathBuf::from("/repo/CLAUDE.md"));
        assert_eq!(ctx.agents_dir(), PathBuf::from("/repo/.claude/agents"));
        assert_eq!(ctx.skills_dir(), PathBuf::from("/repo/.claude/skills"));
    }
}
