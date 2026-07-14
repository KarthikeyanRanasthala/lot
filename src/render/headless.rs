use super::AnimationRenderer;
use crate::input::LoadedInput;
use anyhow::{Context, Result, anyhow, bail};
use std::{io::Write, time::Duration};

pub struct Options<'a> {
    pub animation_index: usize,
    pub theme_id: Option<&'a str>,
    pub width: u32,
    pub height: u32,
    pub fps: f32,
}

/// Writes a single playback of the selected animation as tightly-packed RGBA frames.
///
/// The caller owns the stream framing: each frame is exactly `width * height * 4` bytes and
/// stdout contains no metadata, progress, or terminal escape sequences.
pub fn write_rgba_frames<W: Write>(
    input: &LoadedInput,
    options: Options<'_>,
    writer: &mut W,
) -> Result<()> {
    if !options.fps.is_finite() || options.fps <= 0.0 {
        bail!("--fps must be a positive, finite number");
    }

    let duration = input
        .selected_animation(options.animation_index)
        .duration_seconds
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .ok_or_else(|| {
            anyhow!("animation must declare a positive frame rate and duration for headless output")
        })?;
    let frame_count = frame_count(duration, options.fps)?;
    let frame_interval = Duration::from_secs_f64(1.0 / f64::from(options.fps));

    let mut renderer = AnimationRenderer::new(
        input,
        options.animation_index,
        options.theme_id,
        options.width,
        options.height,
    )?;
    renderer.set_looping(false);

    for frame_number in 0..frame_count {
        let (frame, _, _) = renderer.frame();
        writer
            .write_all(frame)
            .context("could not write RGBA frame to standard output")?;

        if frame_number + 1 < frame_count {
            renderer.advance(frame_interval)?;
        }
    }
    writer
        .flush()
        .context("could not flush RGBA frame output")?;
    Ok(())
}

fn frame_count(duration: f64, fps: f32) -> Result<usize> {
    let frames = (duration * f64::from(fps)).ceil();
    if !frames.is_finite() || frames < 1.0 || frames > usize::MAX as f64 {
        bail!("requested animation output contains an unsupported number of frames");
    }
    Ok(frames as usize)
}

#[cfg(test)]
mod tests {
    use super::{Options, write_rgba_frames};
    use crate::input::LoadedInput;

    #[test]
    fn writes_one_frame_per_requested_output_tick() {
        let input = LoadedInput::from_bytes(
            br#"{"v":"5.5.2","fr":10,"ip":0,"op":10,"w":16,"h":16,"layers":[]}"#,
        )
        .unwrap();
        let mut output = Vec::new();

        write_rgba_frames(
            &input,
            Options {
                animation_index: 0,
                theme_id: None,
                width: 4,
                height: 3,
                fps: 5.0,
            },
            &mut output,
        )
        .unwrap();

        assert_eq!(output.len(), 5 * 4 * 3 * 4);
    }

    #[test]
    fn rejects_an_invalid_output_rate() {
        let input = LoadedInput::from_bytes(
            br#"{"v":"5.5.2","fr":10,"ip":0,"op":10,"w":16,"h":16,"layers":[]}"#,
        )
        .unwrap();

        assert!(
            write_rgba_frames(
                &input,
                Options {
                    animation_index: 0,
                    theme_id: None,
                    width: 4,
                    height: 3,
                    fps: 0.0,
                },
                &mut Vec::new(),
            )
            .is_err()
        );
    }
}
