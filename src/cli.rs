use anyhow::Result;
use clap::{Parser, Subcommand};
use crate::context::Context;
use crate::state::State;
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
