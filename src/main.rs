mod app;
mod cli;
mod input;
mod tui;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    let command = cli::Command::parse();
    app::run(command)
}
