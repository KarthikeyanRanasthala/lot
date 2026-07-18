mod app;
mod cli;
mod input;
mod playlist;
mod render;
mod tui;
mod tui_playlist;

pub mod terminal;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    let command = cli::Command::parse();
    app::run(command)
}
