mod context;
mod profile;
mod manifest;
mod state;
mod slots;
mod gitignore;
mod target;
mod cli;

use clap::Parser;

fn main() {
    if let Err(e) = cli::run(cli::Cli::parse()) {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
