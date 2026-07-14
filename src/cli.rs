use clap::Parser;
use std::path::PathBuf;

/// Inspect a Lottie JSON animation or a dotLottie container in the terminal.
#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Command {
    /// A local .json/.lottie file or an http(s) URL.
    pub input: String,

    /// Request raw frame output. Rendering is intentionally not available yet.
    #[arg(long, requires_all = ["width", "height", "fps"])]
    pub headless: bool,

    /// Output frame width in headless mode.
    #[arg(long, value_name = "PIXELS")]
    pub width: Option<u32>,

    /// Output frame height in headless mode.
    #[arg(long, value_name = "PIXELS")]
    pub height: Option<u32>,

    /// Output frames per second in headless mode.
    #[arg(long, value_name = "FPS")]
    pub fps: Option<f32>,

    /// dotLottie animation ID to select in headless mode.
    #[arg(long)]
    pub animation_id: Option<String>,

    /// dotLottie theme ID to select in headless mode.
    #[arg(long)]
    pub theme: Option<String>,
}

impl Command {
    pub fn local_path(&self) -> Option<PathBuf> {
        (!self.input.starts_with("http://") && !self.input.starts_with("https://"))
            .then(|| PathBuf::from(&self.input))
    }
}
