use std::fs;
use std::path::Path;
use anyhow::{Context as _, Result};

pub fn ensure_ignored(repo_root: &Path, entries: &[&str]) -> Result<()> {
    let path = repo_root.join(".gitignore");
    let mut body = if path.is_file() {
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?
    } else {
        String::new()
    };
    let existing: Vec<String> = body.lines().map(|l| l.trim().to_owned()).collect();
    let mut changed = false;
    for entry in entries {
        if !existing.iter().any(|l| l.as_str() == *entry) {
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
