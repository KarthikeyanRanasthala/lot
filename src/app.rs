use crate::{cli::Command, input::LoadedInput, tui};
use anyhow::{Context, Result, bail};

pub fn run(command: Command) -> Result<()> {
    if command.headless {
        bail!(
            "headless frame output is not available yet: this build validates inputs and provides the metadata TUI, but does not include a renderer"
        );
    }

    let loaded = if let Some(path) = command.local_path() {
        LoadedInput::from_path(&path)
            .with_context(|| format!("could not load {}", path.display()))?
    } else {
        eprintln!("Downloading {}", command.input);
        LoadedInput::from_url(&command.input, |downloaded, total| match total {
            Some(total) => eprint!("\rDownloaded {downloaded}/{total} bytes"),
            None => eprint!("\rDownloaded {downloaded} bytes"),
        })
        .with_context(|| format!("could not download {}", command.input))?
    };

    if command.local_path().is_none() {
        eprintln!();
    }

    tui::run(loaded)
}
