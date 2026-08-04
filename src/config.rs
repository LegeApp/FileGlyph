use crate::classify::{classify, make_label};
use crate::model::{is_valid_extension, normalize_extension, Category};
use crate::paths;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub style: IconStyle,
    #[serde(default)]
    pub category_overrides: BTreeMap<String, Category>,
    #[serde(default)]
    pub label_overrides: BTreeMap<String, String>,
    #[serde(default)]
    pub color_overrides: BTreeMap<String, String>,
    #[serde(default)]
    pub excluded_extensions: BTreeSet<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            style: IconStyle::default(),
            category_overrides: BTreeMap::new(),
            label_overrides: BTreeMap::new(),
            color_overrides: BTreeMap::new(),
            excluded_extensions: BTreeSet::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IconStyle {
    #[serde(default = "default_sizes")]
    pub sizes: Vec<u32>,
    #[serde(default = "default_font_paths")]
    pub font_paths: Vec<String>,
    #[serde(default = "default_font_height_ratio")]
    pub font_height_ratio: f32,
    #[serde(default = "default_horizontal_scale")]
    pub horizontal_scale: f32,
    #[serde(default = "default_right_padding_ratio")]
    pub right_padding_ratio: f32,
    #[serde(default = "default_top_padding_ratio")]
    pub top_padding_ratio: f32,
    #[serde(default = "default_max_width_ratio")]
    pub max_width_ratio: f32,
    #[serde(default = "default_halo_radius_ratio")]
    pub halo_radius_ratio: f32,
    #[serde(default = "default_halo_alpha")]
    pub halo_alpha: u8,
    #[serde(default = "default_supersample")]
    pub supersample: u32,
    #[serde(default = "default_max_label_chars")]
    pub max_label_chars: usize,
}

impl Default for IconStyle {
    fn default() -> Self {
        Self {
            sizes: default_sizes(),
            font_paths: default_font_paths(),
            font_height_ratio: default_font_height_ratio(),
            horizontal_scale: default_horizontal_scale(),
            right_padding_ratio: default_right_padding_ratio(),
            top_padding_ratio: default_top_padding_ratio(),
            max_width_ratio: default_max_width_ratio(),
            halo_radius_ratio: default_halo_radius_ratio(),
            halo_alpha: default_halo_alpha(),
            supersample: default_supersample(),
            max_label_chars: default_max_label_chars(),
        }
    }
}

fn default_sizes() -> Vec<u32> {
    vec![16, 20, 24, 32, 40, 48, 64, 96, 128, 256]
}

fn default_font_paths() -> Vec<String> {
    vec![
        "%WINDIR%\\Fonts\\segoeuib.ttf".to_string(),
        "%WINDIR%\\Fonts\\segoeui.ttf".to_string(),
        "%WINDIR%\\Fonts\\bahnschrift.ttf".to_string(),
        "%WINDIR%\\Fonts\\arialbd.ttf".to_string(),
        "%WINDIR%\\Fonts\\arial.ttf".to_string(),
    ]
}

fn default_font_height_ratio() -> f32 {
    0.72
}
fn default_horizontal_scale() -> f32 {
    0.50
}
fn default_right_padding_ratio() -> f32 {
    0.07
}
fn default_top_padding_ratio() -> f32 {
    0.07
}
fn default_max_width_ratio() -> f32 {
    0.88
}
fn default_halo_radius_ratio() -> f32 {
    0.018
}
fn default_halo_alpha() -> u8 {
    220
}
fn default_supersample() -> u32 {
    4
}
fn default_max_label_chars() -> usize {
    4
}

impl Config {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let selected = match path {
            Some(path) => Some(path.to_path_buf()),
            None => {
                let default_path = paths::config_path()?;
                default_path.exists().then_some(default_path)
            }
        };

        let mut config = match selected {
            Some(path) => {
                let bytes = fs::read(&path)
                    .with_context(|| format!("failed to read config {}", path.display()))?;
                serde_json::from_slice::<Config>(&bytes)
                    .with_context(|| format!("failed to parse config {}", path.display()))?
            }
            None => Config::default(),
        };

        config.normalize_keys();
        config.validate()?;
        Ok(config)
    }

    pub fn write_default(path: Option<PathBuf>, force: bool) -> Result<PathBuf> {
        let path = path.unwrap_or(paths::config_path()?);
        if path.exists() && !force {
            bail!(
                "{} already exists; pass --force to replace it",
                path.display()
            );
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let data = serde_json::to_vec_pretty(&Config::default())?;
        fs::write(&path, data).with_context(|| format!("failed to write {}", path.display()))?;
        Ok(path)
    }

    fn normalize_keys(&mut self) {
        self.category_overrides = self
            .category_overrides
            .iter()
            .map(|(key, value)| (normalize_extension(key), *value))
            .filter(|(key, _)| !key.is_empty())
            .collect();
        self.label_overrides = self
            .label_overrides
            .iter()
            .map(|(key, value)| {
                (
                    normalize_extension(key),
                    sanitize_label(value, self.style.max_label_chars),
                )
            })
            .filter(|(key, value)| !key.is_empty() && !value.is_empty())
            .collect();
        self.excluded_extensions = self
            .excluded_extensions
            .iter()
            .map(|value| normalize_extension(value))
            .filter(|value| !value.is_empty())
            .collect();
        self.color_overrides = self
            .color_overrides
            .iter()
            .map(|(key, value)| (key.trim().to_ascii_lowercase(), value.trim().to_string()))
            .collect();
    }

    fn validate(&self) -> Result<()> {
        if self.style.sizes.is_empty() {
            bail!("style.sizes cannot be empty");
        }
        if self
            .style
            .sizes
            .iter()
            .any(|size| *size == 0 || *size > 256)
        {
            bail!("all ICO sizes must be between 1 and 256");
        }
        if !(0.2..=1.5).contains(&self.style.font_height_ratio) {
            bail!("style.font_height_ratio must be between 0.2 and 1.5");
        }
        if !(0.1..=1.0).contains(&self.style.horizontal_scale) {
            bail!("style.horizontal_scale must be between 0.1 and 1.0");
        }
        if !(0.0..=0.45).contains(&self.style.right_padding_ratio) {
            bail!("style.right_padding_ratio must be between 0.0 and 0.45");
        }
        if !(0.0..=0.45).contains(&self.style.top_padding_ratio) {
            bail!("style.top_padding_ratio must be between 0.0 and 0.45");
        }
        if !(0.2..=1.0).contains(&self.style.max_width_ratio) {
            bail!("style.max_width_ratio must be between 0.2 and 1.0");
        }
        if !(0.0..=0.15).contains(&self.style.halo_radius_ratio) {
            bail!("style.halo_radius_ratio must be between 0.0 and 0.15");
        }
        if self.style.supersample == 0 || self.style.supersample > 8 {
            bail!("style.supersample must be between 1 and 8");
        }
        if self.style.max_label_chars == 0 || self.style.max_label_chars > 8 {
            bail!("style.max_label_chars must be between 1 and 8");
        }
        for extension in self
            .category_overrides
            .keys()
            .chain(self.label_overrides.keys())
            .chain(self.excluded_extensions.iter())
        {
            if !is_valid_extension(extension) {
                bail!("invalid extension in configuration: {extension:?}");
            }
        }
        for (category, color) in &self.color_overrides {
            if !Category::ALL
                .iter()
                .any(|candidate| candidate.as_str() == category)
            {
                bail!("unknown category in color_overrides: {category:?}");
            }
            parse_hex_color(color).with_context(|| {
                format!("invalid color override for category {category}: {color}")
            })?;
        }
        Ok(())
    }

    pub fn category_for(
        &self,
        extension: &str,
        content_type: Option<&str>,
        perceived_type: Option<&str>,
        prog_id: Option<&str>,
        friendly_name: Option<&str>,
    ) -> Category {
        let ext = normalize_extension(extension);
        self.category_overrides
            .get(&ext)
            .copied()
            .unwrap_or_else(|| classify(&ext, content_type, perceived_type, prog_id, friendly_name))
    }

    pub fn label_for(&self, extension: &str) -> String {
        let ext = normalize_extension(extension);
        self.label_overrides
            .get(&ext)
            .cloned()
            .unwrap_or_else(|| make_label(&ext, self.style.max_label_chars))
    }

    pub fn color_for(&self, category: Category) -> Result<[u8; 3]> {
        let value = self
            .color_overrides
            .get(category.as_str())
            .map(String::as_str)
            .unwrap_or_else(|| category.default_color());
        parse_hex_color(value)
    }

    pub fn color_string_for(&self, category: Category) -> String {
        self.color_overrides
            .get(category.as_str())
            .cloned()
            .unwrap_or_else(|| category.default_color().to_string())
    }

    pub fn is_excluded(&self, extension: &str) -> bool {
        self.excluded_extensions
            .contains(&normalize_extension(extension))
    }
}

pub fn parse_hex_color(value: &str) -> Result<[u8; 3]> {
    let value = value.trim().trim_start_matches('#');
    if value.len() != 6 || !value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        bail!("expected #RRGGBB");
    }
    Ok([
        u8::from_str_radix(&value[0..2], 16)?,
        u8::from_str_radix(&value[2..4], 16)?,
        u8::from_str_radix(&value[4..6], 16)?,
    ])
}

fn sanitize_label(value: &str, max_chars: usize) -> String {
    value
        .trim()
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .flat_map(|ch| ch.to_uppercase())
        .take(max_chars.max(1))
        .collect()
}
