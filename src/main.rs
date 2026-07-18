use anyhow::Result;
use clap::Parser;
use lot::app;
use lot::cli::Command;

fn main() -> Result<()> {
    let command = Command::parse();
    app::run(command)
}
