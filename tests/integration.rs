use assert_cmd::Command;
use tempfile::tempdir;
use std::fs;

fn write_profile_settings(repo: &std::path::Path, name: &str, json: &str) {
    let dir = repo.join(".claude-profiles").join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("settings.json"), json).unwrap();
}

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
