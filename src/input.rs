use anyhow::{Context, Result, anyhow, bail};
use dotlottie_rs::{DotLottieManager, Manifest};
use serde_json::Value;
use std::{fs, io::Read, path::Path, sync::Arc};

#[derive(Clone, Debug, PartialEq)]
pub struct AnimationInfo {
    pub id: String,
    pub name: Option<String>,
    pub initial_theme_id: Option<String>,
    pub width: Option<u64>,
    pub height: Option<u64>,
    pub fps: Option<f64>,
    pub duration_seconds: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThemeInfo {
    pub id: String,
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LoadedInput {
    Json {
        data: Arc<[u8]>,
        animation: AnimationInfo,
    },
    DotLottie {
        data: Arc<[u8]>,
        default_animation_id: String,
        animations: Vec<AnimationInfo>,
        themes: Vec<ThemeInfo>,
    },
}

impl LoadedInput {
    pub fn from_path(path: &Path) -> Result<Self> {
        let data = fs::read(path)?;
        Self::from_bytes(&data)
    }

    pub fn from_url<F>(url: &str, mut progress: F) -> Result<Self>
    where
        F: FnMut(u64, Option<u64>),
    {
        let response = ureq::get(url)
            .call()
            .map_err(|error| anyhow!("request failed: {error}"))?;
        let total = response
            .headers()
            .get("content-length")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        let mut reader = response.into_body().into_reader();
        let mut data = Vec::new();
        let mut chunk = [0_u8; 16 * 1024];
        let mut downloaded = 0_u64;

        loop {
            let read = reader.read(&mut chunk)?;
            if read == 0 {
                break;
            }
            data.extend_from_slice(&chunk[..read]);
            downloaded += read as u64;
            progress(downloaded, total);
        }

        Self::from_bytes(&data)
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.starts_with(b"PK") {
            return Self::dotlottie(data);
        }

        Self::json(data)
    }

    fn json(data: &[u8]) -> Result<Self> {
        let value: Value = serde_json::from_slice(data).context("invalid Lottie JSON")?;
        let animation = animation_from_json("animation", None, None, &value)?;
        Ok(Self::Json {
            data: Arc::from(data),
            animation,
        })
    }

    fn dotlottie(data: &[u8]) -> Result<Self> {
        // DotLottieManager checks the ZIP and manifest formats. Reading every listed animation
        // also verifies that its packaged Lottie payload is present and valid JSON.
        let manager = DotLottieManager::new(data)
            .map_err(|error| anyhow!("invalid dotLottie container: {error:?}"))?;
        let manifest = manager.manifest();
        let animations = animations_from_manifest(&manager, manifest)?;
        let default_animation_id = manager.active_animation_id();
        let themes = manifest
            .themes
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|theme| ThemeInfo {
                id: theme.id.clone(),
                name: theme.name.clone(),
            })
            .collect();

        Ok(Self::DotLottie {
            data: Arc::from(data),
            default_animation_id,
            animations,
            themes,
        })
    }

    pub fn selected_animation(&self, index: usize) -> &AnimationInfo {
        match self {
            Self::Json { animation, .. } => animation,
            Self::DotLottie { animations, .. } => &animations[index.min(animations.len() - 1)],
        }
    }

    pub fn data(&self) -> &[u8] {
        match self {
            Self::Json { data, .. } | Self::DotLottie { data, .. } => data,
        }
    }

    pub fn animation_id(&self, index: usize) -> Option<&str> {
        match self {
            Self::Json { .. } => None,
            Self::DotLottie { animations, .. } => animations
                .get(index.min(animations.len() - 1))
                .map(|animation| animation.id.as_str()),
        }
    }

    pub fn default_animation_index(&self) -> usize {
        match self {
            Self::Json { .. } => 0,
            Self::DotLottie {
                default_animation_id,
                animations,
                ..
            } => animations
                .iter()
                .position(|animation| animation.id == *default_animation_id)
                .unwrap_or(0),
        }
    }

    pub fn initial_theme_index(&self, animation_index: usize) -> Option<usize> {
        let initial_theme_id = self
            .selected_animation(animation_index)
            .initial_theme_id
            .as_deref()?;
        self.themes()
            .iter()
            .position(|theme| theme.id == initial_theme_id)
    }

    pub fn is_dotlottie(&self) -> bool {
        matches!(self, Self::DotLottie { .. })
    }

    pub fn animations(&self) -> &[AnimationInfo] {
        match self {
            Self::Json { animation, .. } => std::slice::from_ref(animation),
            Self::DotLottie { animations, .. } => animations,
        }
    }

    pub fn themes(&self) -> &[ThemeInfo] {
        match self {
            Self::Json { .. } => &[],
            Self::DotLottie { themes, .. } => themes,
        }
    }
}

fn animations_from_manifest(
    manager: &DotLottieManager,
    manifest: &Manifest,
) -> Result<Vec<AnimationInfo>> {
    if manifest.animations.is_empty() {
        bail!("dotLottie container does not include any animations");
    }

    manifest
        .animations
        .iter()
        .map(|entry| {
            let document = manager
                .get_animation(&entry.id)
                .map_err(|error| anyhow!("animation `{}` is invalid: {error:?}", entry.id))?;
            let value: Value = serde_json::from_str(&document)
                .with_context(|| format!("animation `{}` is not valid Lottie JSON", entry.id))?;
            animation_from_json(
                &entry.id,
                entry.name.clone(),
                entry.initial_theme.clone(),
                &value,
            )
        })
        .collect()
}

fn animation_from_json(
    id: &str,
    name: Option<String>,
    initial_theme_id: Option<String>,
    value: &Value,
) -> Result<AnimationInfo> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("Lottie animation must be a JSON object"))?;
    let width = object.get("w").and_then(Value::as_u64);
    let height = object.get("h").and_then(Value::as_u64);
    let fps = object.get("fr").and_then(Value::as_f64);
    let in_point = object.get("ip").and_then(Value::as_f64);
    let out_point = object.get("op").and_then(Value::as_f64);
    let duration_seconds = match (fps, in_point, out_point) {
        (Some(rate), Some(start), Some(end)) if rate > 0.0 && end >= start => {
            Some((end - start) / rate)
        }
        _ => None,
    };

    Ok(AnimationInfo {
        id: id.to_owned(),
        name,
        initial_theme_id,
        width,
        height,
        fps,
        duration_seconds,
    })
}

#[cfg(test)]
mod tests {
    use super::LoadedInput;

    #[test]
    fn loads_json_animation_metadata() {
        let input =
            LoadedInput::from_bytes(br#"{"w":320,"h":180,"fr":30,"ip":0,"op":60}"#).unwrap();

        assert_eq!(input.selected_animation(0).duration_seconds, Some(2.0));
        assert!(!input.is_dotlottie());
    }

    #[test]
    fn rejects_non_object_json() {
        assert!(LoadedInput::from_bytes(br#"[]"#).is_err());
    }
}
