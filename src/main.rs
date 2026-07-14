mod app;
mod cli;
mod input;
mod render;
mod tui;

pub mod terminal;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    let command = cli::Command::parse();
    app::run(command)
}
