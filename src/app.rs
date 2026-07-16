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
        LoadedInput::from_url(&command.input, |downloaded, total| {
            eprint!("{}", download_progress_line(downloaded, total));
        })
        .with_context(|| format!("could not download {}", command.input))
    }
}

fn format_bytes(bytes: u64) -> String {
    let (value, unit) = format_byte_parts(bytes);
    format!("{value} {unit}")
}

fn format_download_progress(downloaded: u64, total: u64) -> String {
    let (downloaded_value, downloaded_unit) = format_byte_parts(downloaded);
    let (total_value, total_unit) = format_byte_parts(total);

    if downloaded_unit == total_unit {
        format!("{downloaded_value} / {total_value} {total_unit}")
    } else {
        format!("{downloaded_value} {downloaded_unit} / {total_value} {total_unit}")
    }
}

fn download_progress_line(downloaded: u64, total: Option<u64>) -> String {
    let progress = total.map_or_else(
        || format!("Downloading {}", format_bytes(downloaded)),
        |total| format_download_bar(downloaded, total),
    );
    format!("\r{progress}\x1b[K")
}

fn format_download_bar(downloaded: u64, total: u64) -> String {
    const BAR_WIDTH: usize = 20;

    let ratio = if total == 0 {
        0.0
    } else {
        downloaded as f64 / total as f64
    }
    .clamp(0.0, 1.0);
    let filled = (ratio * BAR_WIDTH as f64).round() as usize;
    let bar = format!("{}{}", "█".repeat(filled), "░".repeat(BAR_WIDTH - filled));
    let percent = (ratio * 100.0).round() as u8;

    format!(
        "Downloading [{bar}] {percent:>3}%  {}",
        format_download_progress(downloaded, total)
    )
}

fn format_byte_parts(bytes: u64) -> (String, &'static str) {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];

    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        (bytes.to_string(), UNITS[unit])
    } else {
        (format!("{value:.1}"), UNITS[unit])
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

#[cfg(test)]
mod tests {
    use super::{
        download_progress_line, format_bytes, format_download_bar, format_download_progress,
    };

    #[test]
    fn formats_download_sizes_with_adaptive_units() {
        assert_eq!(format_bytes(999), "999 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1_572_864), "1.5 MB");
    }

    #[test]
    fn download_progress_shares_a_repeated_unit() {
        assert_eq!(
            format_download_progress(1_572_864, 1_572_864),
            "1.5 / 1.5 MB"
        );
        assert_eq!(
            format_download_progress(16 * 1024, 1_572_864),
            "16.0 KB / 1.5 MB"
        );
    }

    #[test]
    fn download_progress_bar_includes_percentage_and_sizes() {
        assert_eq!(
            format_download_bar(1_048_576, 1_572_864),
            "Downloading [█████████████░░░░░░░]  67%  1.0 / 1.5 MB"
        );
    }

    #[test]
    fn download_progress_clears_stale_terminal_text() {
        assert_eq!(
            download_progress_line(1_048_576, Some(1_572_864)),
            "\rDownloading [█████████████░░░░░░░]  67%  1.0 / 1.5 MB\x1b[K"
        );
    }
}
