use crate::{cli::Command, input::LoadedInput, render::headless, tui};
use anyhow::{Context, Result, anyhow};
use std::io;

pub fn run(command: Command) -> Result<()> {
    let loaded = load_input(&command)?;

    if command.headless {
        return run_headless(&command, &loaded);
    }

    tui::run(loaded)
}

fn load_input(command: &Command) -> Result<LoadedInput> {
    if let Some(path) = command.local_path() {
        LoadedInput::from_path(&path).with_context(|| format!("could not load {}", path.display()))
    } else {
        eprintln!("Downloading {}", command.input);
        LoadedInput::from_url(&command.input, |downloaded, total| match total {
            Some(total) => eprint!("\rDownloaded {downloaded}/{total} bytes"),
            None => eprint!("\rDownloaded {downloaded} bytes"),
        })
        .with_context(|| format!("could not download {}", command.input))
    }
}

fn run_headless(command: &Command, loaded: &LoadedInput) -> Result<()> {
    if command.local_path().is_none() {
        eprintln!();
    }

    let animation_index = loaded.animation_index(command.animation_id.as_deref())?;
    let theme_id = loaded.theme_id(animation_index, command.theme.as_deref())?;
    let width = command
        .width
        .ok_or_else(|| anyhow!("--width is required with --headless"))?;
    let height = command
        .height
        .ok_or_else(|| anyhow!("--height is required with --headless"))?;
    let fps = command
        .fps
        .ok_or_else(|| anyhow!("--fps is required with --headless"))?;

    headless::write_rgba_frames(
        loaded,
        headless::Options {
            animation_index,
            theme_id,
            width,
            height,
            fps,
        },
        &mut io::stdout().lock(),
    )
}
