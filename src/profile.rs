use crate::context::Context;
use anyhow::{Context as _, Result};
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub struct ProfileFile {
    pub rel: PathBuf,
    pub contents: Vec<u8>,
}

pub struct Profile {
    pub name: String,
    pub settings: Option<Value>,
    pub claude_md: Option<String>,
    pub mcp_servers: Map<String, Value>,
    pub agents: Vec<ProfileFile>,
    pub skills: Vec<ProfileFile>,
}

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
            Some(_) => anyhow::bail!(
                "{}/mcp.json must be a JSON object of server definitions",
                name
            ),
            None => Map::new(),
        };

        let agents = read_tree(&dir.join("agents"))?;
        let skills = read_tree(&dir.join("skills"))?;

        Ok(Profile {
            name: name.to_string(),
            settings,
            claude_md,
            mcp_servers,
            agents,
            skills,
        })
    }
}

fn read_string_opt(path: &Path) -> Result<Option<String>> {
    if path.is_file() {
        Ok(Some(
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?,
        ))
    } else {
        Ok(None)
    }
}

fn read_json_opt(path: &Path) -> Result<Option<Value>> {
    match read_string_opt(path)? {
        Some(s) => Ok(Some(
            serde_json::from_str(&s).with_context(|| format!("parsing {}", path.display()))?,
        )),
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
            out.push(ProfileFile {
                rel,
                contents: fs::read(&path)?,
            });
        }
    }
    Ok(())
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
