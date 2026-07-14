use crate::input::LoadedInput;
use crate::terminal::kitty::{
    BufferingStrategy, ImageId, PlacementId, Presenter, PreviewArea, RgbaFrame,
};
use anyhow::{Context, Result, anyhow};
use dotlottie_rs::{ColorSpace, Player};
use std::{env, ffi::CString, io::Write, time::Duration};

pub mod headless;

/// CPU-backed Lottie playback that exposes each rendered frame as straight RGBA bytes.
pub struct AnimationRenderer {
    player: Player,
    software_buffer: Vec<u32>,
    rgba_buffer: Vec<u8>,
    width: u32,
    height: u32,
}

impl AnimationRenderer {
    pub fn new(
        input: &LoadedInput,
        animation_index: usize,
        theme_id: Option<&str>,
        width: u32,
        height: u32,
    ) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(anyhow!("render dimensions must be greater than zero"));
        }

        let pixels = usize::try_from(width)
            .ok()
            .and_then(|width| width.checked_mul(usize::try_from(height).ok()?))
            .ok_or_else(|| anyhow!("render dimensions are too large"))?;
        let mut software_buffer = vec![0_u32; pixels];
        let mut player = Player::new();
        player
            .set_sw_target(&mut software_buffer, width, height, ColorSpace::ABGR8888S)
            .map_err(|error| anyhow!("could not configure software renderer: {error:?}"))?;

        if input.is_dotlottie() {
            player
                .load_dotlottie_data(input.data())
                .map_err(|error| anyhow!("could not load dotLottie animation: {error:?}"))?;
            if let Some(animation_id) = input.animation_id(animation_index) {
                let animation_id = CString::new(animation_id)
                    .context("animation ID contains an unsupported NUL byte")?;
                player
                    .load_animation(&animation_id)
                    .map_err(|error| anyhow!("could not select animation: {error:?}"))?;
            }
        } else {
            let json = std::str::from_utf8(input.data()).context("Lottie JSON is not UTF-8")?;
            let json =
                CString::new(json).context("Lottie JSON contains an unsupported NUL byte")?;
            player
                .load_animation_data(&json)
                .map_err(|error| anyhow!("could not load Lottie animation: {error:?}"))?;
        }

        if let Some(theme_id) = theme_id {
            let theme_id =
                CString::new(theme_id).context("theme ID contains an unsupported NUL byte")?;
            player
                .set_theme(&theme_id)
                .map_err(|error| anyhow!("could not apply dotLottie theme: {error:?}"))?;
        }

        // Callers that need a finite export disable looping after construction.
        player.set_loop(true);
        player
            .play()
            .map_err(|error| anyhow!("could not start animation playback: {error:?}"))?;

        let mut renderer = Self {
            player,
            software_buffer,
            rgba_buffer: vec![0_u8; pixels * 4],
            width,
            height,
        };
        renderer.copy_rgba();
        Ok(renderer)
    }

    pub fn frame(&self) -> (&[u8], u32, u32) {
        (&self.rgba_buffer, self.width, self.height)
    }

    /// Advances dotlottie-rs using its millisecond clock and returns whether a frame changed.
    pub fn advance(&mut self, elapsed: Duration) -> Result<bool> {
        let changed = self
            .player
            .tick(elapsed.as_secs_f32() * 1_000.0)
            .map_err(|error| anyhow!("could not render animation frame: {error:?}"))?;
        if changed {
            self.copy_rgba();
        }
        Ok(changed)
    }

    pub fn set_looping(&mut self, looping: bool) {
        self.player.set_loop(looping);
    }

    fn copy_rgba(&mut self) {
        // ABGR8888S encodes a straight-alpha pixel as 0xAABBGGRR. Converting the numeric value
        // to little-endian bytes yields the protocol's required RGBA byte order on every host.
        for (pixel, rgba) in self
            .software_buffer
            .iter()
            .zip(self.rgba_buffer.chunks_exact_mut(4))
        {
            rgba.copy_from_slice(&pixel.to_le_bytes());
        }
    }
}

/// A dotLottie playback loop paired with the custom Kitty image presenter.
pub struct KittyPlayback {
    animation: AnimationRenderer,
    presenter: Presenter,
    animation_index: usize,
    theme_id: Option<String>,
    area: PreviewArea,
}

impl KittyPlayback {
    pub fn new(
        input: &LoadedInput,
        animation_index: usize,
        theme_id: Option<&str>,
        area: PreviewArea,
        width: u32,
        height: u32,
        strategy: BufferingStrategy,
    ) -> Result<Self> {
        let process_id = std::process::id().max(1);
        let first_image_id = ImageId::new(process_id)?;
        let second_image_id = ImageId::new(process_id.checked_add(1).unwrap_or(1))?;
        let placement_id = PlacementId::new(process_id.checked_add(2).unwrap_or(2))?;
        let presenter = match strategy {
            BufferingStrategy::Single => Presenter::single(first_image_id, placement_id, area),
            BufferingStrategy::Double => {
                Presenter::double(first_image_id, second_image_id, placement_id, area)?
            }
        };

        let theme_id = theme_id.map(str::to_owned);
        Ok(Self {
            animation: AnimationRenderer::new(
                input,
                animation_index,
                theme_id.as_deref(),
                width,
                height,
            )?,
            presenter,
            animation_index,
            theme_id,
            area,
        })
    }

    pub fn matches(
        &self,
        animation_index: usize,
        theme_id: Option<&str>,
        area: PreviewArea,
    ) -> bool {
        self.animation_index == animation_index
            && self.theme_id.as_deref() == theme_id
            && self.area == area
    }

    pub fn present<W: Write>(&mut self, writer: &mut W) -> Result<()> {
        let (pixels, width, height) = self.animation.frame();
        self.presenter
            .present(writer, RgbaFrame::new(width, height, pixels)?)?;
        Ok(())
    }

    /// Advances the source timeline without forcing an image upload. The caller may pace uploads
    /// separately and present only the latest frame that fits its terminal transport budget.
    pub fn advance(&mut self, elapsed: Duration) -> Result<bool> {
        self.animation.advance(elapsed)
    }

    pub fn clear<W: Write>(&mut self, writer: &mut W) -> Result<()> {
        self.presenter.clear(writer)?;
        Ok(())
    }
}

/// Uses only terminals with a known static-image update strategy. WezTerm's Kitty support is
/// detected directly and uses the conservative double-buffer path.
pub fn kitty_strategy_from_environment() -> Option<BufferingStrategy> {
    terminal_strategy(
        env::var("TERM").ok().as_deref(),
        env::var("TERM_PROGRAM").ok().as_deref(),
    )
}

fn terminal_strategy(
    term: Option<&str>,
    terminal_program: Option<&str>,
) -> Option<BufferingStrategy> {
    if term == Some("xterm-kitty") || terminal_program == Some("WezTerm") {
        return Some(BufferingStrategy::Double);
    }

    if term == Some("xterm-ghostty") {
        return Some(BufferingStrategy::Single);
    }

    match terminal_program {
        Some("Ghostty" | "ghostty" | "Warp" | "WarpTerminal") => Some(BufferingStrategy::Single),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{AnimationRenderer, BufferingStrategy, terminal_strategy};
    use crate::input::LoadedInput;
    use std::time::Duration;

    #[test]
    fn renders_a_json_animation_to_rgba() {
        let input = LoadedInput::from_bytes(
            br#"{"v":"5.5.2","fr":30,"ip":0,"op":30,"w":16,"h":16,"layers":[]}"#,
        )
        .unwrap();
        let mut renderer = AnimationRenderer::new(&input, 0, None, 8, 8).unwrap();

        let (frame, width, height) = renderer.frame();
        assert_eq!((width, height), (8, 8));
        assert_eq!(frame.len(), 8 * 8 * 4);
        renderer.advance(Duration::from_millis(34)).unwrap();
    }

    #[test]
    fn chooses_the_documented_buffering_strategy_per_terminal() {
        assert_eq!(
            terminal_strategy(Some("xterm-kitty"), None),
            Some(BufferingStrategy::Double)
        );
        assert_eq!(
            terminal_strategy(Some("xterm-ghostty"), None),
            Some(BufferingStrategy::Single)
        );
        assert_eq!(
            terminal_strategy(None, Some("WarpTerminal")),
            Some(BufferingStrategy::Single)
        );
        assert_eq!(
            terminal_strategy(Some("xterm-256color"), Some("WezTerm")),
            Some(BufferingStrategy::Double)
        );
        assert_eq!(terminal_strategy(Some("screen"), None), None);
    }
}
