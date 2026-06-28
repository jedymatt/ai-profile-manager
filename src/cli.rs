use anyhow::Result;
use clap::{Parser, Subcommand};
use crate::context::Context;
use crate::profile::Profile;
use crate::state::State;
use crate::target::{ClaudeCodeTarget, Target};
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
    /// Create a new empty profile
    New { name: String },
    /// List profiles (marks the active one)
    List,
    /// Activate a profile (switching from any current one)
    Use {
        name: String,
        /// Back up and overwrite foreign (hand-made) local slot files
        #[arg(long)]
        force: bool,
    },
    /// Remove the active personal overlay, leaving only committed team config
    Deactivate,
    /// Show the active profile and what is projected (with a drift check)
    Status,
    /// Open a profile directory in $EDITOR (or print its path)
    Edit {
        name: String,
        /// Print the profile directory path instead of launching an editor
        #[arg(long)]
        print_path: bool,
    },
    /// Delete a profile (deactivating it first if it is active)
    Remove { name: String },
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
        Command::New { name } => cmd_new(&ctx, &name),
        Command::List => cmd_list(&ctx),
        Command::Use { name, force } => cmd_use(&ctx, &name, force),
        Command::Deactivate => cmd_deactivate(&ctx),
        Command::Status => cmd_status(&ctx),
        Command::Edit { name, print_path } => cmd_edit(&ctx, &name, print_path),
        Command::Remove { name } => cmd_remove(&ctx, &name),
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
