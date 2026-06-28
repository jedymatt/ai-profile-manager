use anyhow::{anyhow, Context as _, Result};
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::Path;
use tempfile::NamedTempFile;

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("no parent for {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let mut tmp = NamedTempFile::new_in(parent)?;
    tmp.write_all(bytes)?;
    tmp.flush()?;
    tmp.persist(path)
        .map_err(|e| anyhow!("writing {}: {}", path.display(), e))?;
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

/// Render a path in canonical forward-slash form so tool output (status
/// listings, `.gitignore` entries) is identical across platforms. On Windows
/// `Path::display` emits `\`; we normalize to `/`, which Windows also accepts.
pub fn forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

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

    #[test]
    fn forward_slashes_normalizes_separators() {
        // Windows-native backslashes render as forward slashes so tool output
        // (status listings, gitignore entries) is identical across platforms.
        assert_eq!(
            forward_slashes(Path::new(".claude\\settings.local.json")),
            ".claude/settings.local.json"
        );
        // A path that is already forward-slash (the Unix-native form) is unchanged.
        assert_eq!(
            forward_slashes(Path::new(".claude/agents/rev.md")),
            ".claude/agents/rev.md"
        );
    }
}
