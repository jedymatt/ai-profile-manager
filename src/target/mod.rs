use anyhow::Result;
use crate::context::Context;
use crate::manifest::Manifest;
use crate::profile::Profile;

pub mod claude_code;
pub use claude_code::ClaudeCodeTarget;

pub trait Target {
    fn project(&self, ctx: &Context, profile: &Profile) -> Result<Manifest>;
    fn clear(&self, ctx: &Context, manifest: &Manifest) -> Result<()>;
}
