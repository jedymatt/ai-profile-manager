mod cli;
mod context;
mod gitignore;
mod manifest;
mod profile;
mod slots;
mod state;
mod target;

use clap::Parser;

fn main() {
    if let Err(e) = cli::run(cli::Cli::parse()) {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
