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
        let mut json = serde_json::to_vec_pretty(self)?;
        json.push(b'\n');
        crate::slots::atomic_write(&path, &json)
    }
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
