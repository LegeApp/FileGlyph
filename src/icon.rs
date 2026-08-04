use crate::config::Config;
use crate::model::Category;
use crate::paths;
use anyhow::{bail, Context, Result};
use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
use fontdue::{Font, FontSettings};
use ico::{IconDir, IconDirEntry, IconImage, ResourceType};
use std::collections::VecDeque;
use std::env;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

pub struct IconRenderer<'a> {
    config: &'a Config,
    font: Font,
    font_path: PathBuf,
}

impl<'a> IconRenderer<'a> {
    pub fn new(config: &'a Config, font_override: Option<&Path>) -> Result<Self> {
        let font_path = find_font(config, font_override)?;
        let bytes = fs::read(&font_path)
            .with_context(|| format!("failed to read font {}", font_path.display()))?;
        let font = Font::from_bytes(bytes, FontSettings::default())
            .map_err(|error| anyhow::anyhow!("failed to parse {}: {error}", font_path.display()))?;
        Ok(Self {
            config,
            font,
            font_path,
        })
    }

    pub fn font_path(&self) -> &Path {
        &self.font_path
    }

    pub fn write_ico(&self, output: &Path, label: &str, category: Category) -> Result<()> {
        paths::ensure_parent(output)?;
        let color = self.config.color_for(category)?;
        let mut directory = IconDir::new(ResourceType::Icon);

        let mut sizes = self.config.style.sizes.clone();
        sizes.sort_unstable();
        sizes.dedup();
        for size in sizes {
            let rgba = self.render_rgba(size, label, color)?;
            let image = IconImage::from_rgba_data(size, size, rgba);
            let entry = if size <= 48 {
                IconDirEntry::encode_as_bmp(&image)
            } else {
                IconDirEntry::encode_as_png(&image)
            }
            .with_context(|| format!("failed to encode {size}x{size} icon entry"))?;
            directory.add_entry(entry);
        }

        let temporary = temporary_path(output);
        directory
            .write(File::create(&temporary).with_context(|| {
                format!("failed to create temporary icon {}", temporary.display())
            })?)
            .with_context(|| format!("failed to write icon {}", temporary.display()))?;
        replace_file(&temporary, output)?;
        Ok(())
    }

    pub fn write_png(
        &self,
        output: &Path,
        size: u32,
        label: &str,
        category: Category,
    ) -> Result<()> {
        paths::ensure_parent(output)?;
        let color = self.config.color_for(category)?;
        let rgba = self.render_rgba(size, label, color)?;
        IconImage::from_rgba_data(size, size, rgba)
            .write_png(File::create(output)?)
            .with_context(|| format!("failed to write PNG {}", output.display()))
    }

    pub fn write_category_preview(&self, output: &Path, cell_size: u32) -> Result<()> {
        if cell_size < 32 || cell_size > 512 {
            bail!("preview cell size must be between 32 and 512");
        }
        paths::ensure_parent(output)?;

        let columns = 4_u32;
        let rows = (Category::ALL.len() as u32).div_ceil(columns);
        let gutter = (cell_size / 8).max(8);
        let width = columns * cell_size + (columns + 1) * gutter;
        let height = rows * cell_size + (rows + 1) * gutter;
        let mut canvas = vec![0_u8; (width * height * 4) as usize];

        // A neutral checker-like solid field makes transparency and halo behavior visible.
        for pixel in canvas.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[238, 240, 243, 255]);
        }

        for (index, category) in Category::ALL.iter().copied().enumerate() {
            let column = index as u32 % columns;
            let row = index as u32 / columns;
            let x = gutter + column * (cell_size + gutter);
            let y = gutter + row * (cell_size + gutter);
            let label: String = preview_label(category)
                .chars()
                .take(self.config.style.max_label_chars)
                .collect();
            let rgba = self.render_rgba(cell_size, &label, self.config.color_for(category)?)?;
            alpha_blit(
                &mut canvas,
                width,
                height,
                &rgba,
                cell_size,
                cell_size,
                x,
                y,
            );
        }

        IconImage::from_rgba_data(width, height, canvas)
            .write_png(File::create(output)?)
            .with_context(|| format!("failed to write preview {}", output.display()))
    }

    pub fn render_rgba(&self, size: u32, label: &str, color: [u8; 3]) -> Result<Vec<u8>> {
        if size == 0 || size > 4096 {
            bail!("invalid render size {size}");
        }
        let label = label.trim();
        if label.is_empty() {
            bail!("icon label cannot be empty");
        }

        let supersample = self.config.style.supersample.max(1);
        let work_size = size
            .checked_mul(supersample)
            .ok_or_else(|| anyhow::anyhow!("render size overflow"))?;
        let font_px = work_size as f32 * self.config.style.font_height_ratio;
        let source = self.rasterize_label(label, font_px)?;

        let fixed_scaled_width =
            ((source.width as f32 * self.config.style.horizontal_scale).round() as u32).max(1);
        let max_width =
            ((work_size as f32 * self.config.style.max_width_ratio).round() as u32).max(1);
        let target_width = fixed_scaled_width.min(max_width);
        let text = resize_gray_horizontal(&source, target_width);

        let mut alpha = vec![0_u8; (work_size * work_size) as usize];
        let right_padding =
            (work_size as f32 * self.config.style.right_padding_ratio).round() as i32;
        let top_padding = (work_size as f32 * self.config.style.top_padding_ratio).round() as i32;
        let x = (work_size as i32 - right_padding - text.width as i32).max(0);
        let y = top_padding.max(0);
        blit_gray_max(&mut alpha, work_size, work_size, &text, x as u32, y as u32);

        let radius =
            ((work_size as f32 * self.config.style.halo_radius_ratio).round() as usize).max(1);
        let halo = max_filter_2d(&alpha, work_size as usize, work_size as usize, radius);
        let outline_color = contrasting_halo(color);
        let work_rgba = composite_text_and_halo(
            &alpha,
            &halo,
            color,
            outline_color,
            self.config.style.halo_alpha,
        );

        Ok(downsample_rgba(
            &work_rgba,
            work_size,
            work_size,
            supersample,
        ))
    }

    fn rasterize_label(&self, label: &str, px: f32) -> Result<GrayImage> {
        let mut layout: Layout<()> = Layout::new(CoordinateSystem::PositiveYDown);
        layout.reset(&LayoutSettings {
            x: 0.0,
            y: 0.0,
            max_width: None,
            max_height: None,
            ..LayoutSettings::default()
        });
        layout.append(
            std::slice::from_ref(&self.font),
            &TextStyle::new(label, px, 0),
        );

        let glyphs = layout.glyphs();
        let mut min_x = i32::MAX;
        let mut min_y = i32::MAX;
        let mut max_x = i32::MIN;
        let mut max_y = i32::MIN;
        for glyph in glyphs {
            if glyph.width == 0 || glyph.height == 0 {
                continue;
            }
            let x = glyph.x.floor() as i32;
            let y = glyph.y.floor() as i32;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x + glyph.width as i32);
            max_y = max_y.max(y + glyph.height as i32);
        }

        if min_x == i32::MAX || max_x <= min_x || max_y <= min_y {
            bail!("font produced no visible glyphs for label {label:?}");
        }

        let width = (max_x - min_x) as u32;
        let height = (max_y - min_y) as u32;
        let mut data = vec![0_u8; (width * height) as usize];
        for glyph in glyphs {
            if glyph.width == 0 || glyph.height == 0 {
                continue;
            }
            let (_, bitmap) = self.font.rasterize_config(glyph.key);
            let origin_x = glyph.x.floor() as i32 - min_x;
            let origin_y = glyph.y.floor() as i32 - min_y;
            for glyph_y in 0..glyph.height {
                for glyph_x in 0..glyph.width {
                    let destination_x = origin_x + glyph_x as i32;
                    let destination_y = origin_y + glyph_y as i32;
                    if destination_x < 0
                        || destination_y < 0
                        || destination_x >= width as i32
                        || destination_y >= height as i32
                    {
                        continue;
                    }
                    let source_index = glyph_y * glyph.width + glyph_x;
                    let destination_index =
                        destination_y as usize * width as usize + destination_x as usize;
                    data[destination_index] = data[destination_index].max(bitmap[source_index]);
                }
            }
        }

        Ok(GrayImage {
            width,
            height,
            data,
        })
    }
}

fn preview_label(category: Category) -> &'static str {
    match category {
        Category::Text => "TXT",
        Category::Document => "PDF",
        Category::Spreadsheet => "XLSX",
        Category::Presentation => "PPTX",
        Category::Image => "JP2",
        Category::Video => "MKV",
        Category::Audio => "FLAC",
        Category::Archive => "7Z",
        Category::Database => "SQLT",
        Category::Code => "RS",
        Category::System => "SYS",
        Category::Model3d => "GLTF",
        Category::Font => "OTF",
        Category::Internal => "ASD",
    }
}

#[derive(Debug, Clone)]
struct GrayImage {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

fn find_font(config: &Config, override_path: Option<&Path>) -> Result<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = override_path {
        candidates.push(path.to_path_buf());
    }
    candidates.extend(
        config
            .style
            .font_paths
            .iter()
            .map(|value| PathBuf::from(expand_environment(value))),
    );

    for path in &candidates {
        if path.is_file() {
            return Ok(path.clone());
        }
    }

    let rendered = candidates
        .iter()
        .map(|path| format!("  - {}", path.display()))
        .collect::<Vec<_>>()
        .join("\n");
    bail!("no usable font was found. Pass --font PATH or edit style.font_paths. Tried:\n{rendered}")
}

fn expand_environment(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut result = String::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '%' {
            if let Some(relative_end) = chars[index + 1..].iter().position(|ch| *ch == '%') {
                let end = index + 1 + relative_end;
                let name: String = chars[index + 1..end].iter().collect();
                if let Some(replacement) = env::var_os(&name) {
                    result.push_str(&replacement.to_string_lossy());
                    index = end + 1;
                    continue;
                }
            }
        }
        result.push(chars[index]);
        index += 1;
    }
    result
}

fn resize_gray_horizontal(source: &GrayImage, target_width: u32) -> GrayImage {
    if source.width == target_width {
        return source.clone();
    }
    let target_width = target_width.max(1);
    let mut data = vec![0_u8; (target_width * source.height) as usize];
    let scale = source.width as f32 / target_width as f32;

    for y in 0..source.height {
        for target_x in 0..target_width {
            let source_x = (target_x as f32 + 0.5) * scale - 0.5;
            let left = source_x.floor().max(0.0) as u32;
            let right = (left + 1).min(source.width - 1);
            let fraction = (source_x - left as f32).clamp(0.0, 1.0);
            let left_value = source.data[(y * source.width + left) as usize] as f32;
            let right_value = source.data[(y * source.width + right) as usize] as f32;
            data[(y * target_width + target_x) as usize] =
                (left_value * (1.0 - fraction) + right_value * fraction).round() as u8;
        }
    }

    GrayImage {
        width: target_width,
        height: source.height,
        data,
    }
}

fn blit_gray_max(
    destination: &mut [u8],
    destination_width: u32,
    destination_height: u32,
    source: &GrayImage,
    x: u32,
    y: u32,
) {
    for source_y in 0..source.height {
        let destination_y = y + source_y;
        if destination_y >= destination_height {
            break;
        }
        for source_x in 0..source.width {
            let destination_x = x + source_x;
            if destination_x >= destination_width {
                break;
            }
            let source_index = (source_y * source.width + source_x) as usize;
            let destination_index = (destination_y * destination_width + destination_x) as usize;
            destination[destination_index] =
                destination[destination_index].max(source.data[source_index]);
        }
    }
}

fn max_filter_2d(source: &[u8], width: usize, height: usize, radius: usize) -> Vec<u8> {
    let mut horizontal = vec![0_u8; source.len()];
    for y in 0..height {
        sliding_max(
            &source[y * width..(y + 1) * width],
            &mut horizontal[y * width..(y + 1) * width],
            radius,
        );
    }

    let mut vertical = vec![0_u8; source.len()];
    let mut column = vec![0_u8; height];
    let mut filtered = vec![0_u8; height];
    for x in 0..width {
        for y in 0..height {
            column[y] = horizontal[y * width + x];
        }
        sliding_max(&column, &mut filtered, radius);
        for y in 0..height {
            vertical[y * width + x] = filtered[y];
        }
    }
    vertical
}

fn sliding_max(source: &[u8], destination: &mut [u8], radius: usize) {
    debug_assert_eq!(source.len(), destination.len());
    if source.is_empty() {
        return;
    }
    let mut queue: VecDeque<usize> = VecDeque::new();
    let mut next = 0_usize;

    for center in 0..source.len() {
        let window_end = (center + radius).min(source.len() - 1);
        while next <= window_end {
            while let Some(&back) = queue.back() {
                if source[back] > source[next] {
                    break;
                }
                queue.pop_back();
            }
            queue.push_back(next);
            next += 1;
        }

        let window_start = center.saturating_sub(radius);
        while queue.front().is_some_and(|index| *index < window_start) {
            queue.pop_front();
        }
        destination[center] = source[*queue.front().expect("non-empty max-filter window")];
    }
}

fn contrasting_halo(color: [u8; 3]) -> [u8; 3] {
    let luminance = 0.2126 * color[0] as f32 + 0.7152 * color[1] as f32 + 0.0722 * color[2] as f32;
    if luminance < 175.0 {
        [248, 249, 250]
    } else {
        [24, 28, 33]
    }
}

fn composite_text_and_halo(
    text: &[u8],
    halo: &[u8],
    text_color: [u8; 3],
    halo_color: [u8; 3],
    halo_alpha: u8,
) -> Vec<u8> {
    let mut output = vec![0_u8; text.len() * 4];
    for index in 0..text.len() {
        let text_alpha = text[index] as f32 / 255.0;
        let halo_coverage = halo[index].saturating_sub(text[index]) as f32 / 255.0;
        let outline_alpha = halo_coverage * (halo_alpha as f32 / 255.0);
        let combined_alpha = text_alpha + outline_alpha * (1.0 - text_alpha);
        let pixel = &mut output[index * 4..index * 4 + 4];
        if combined_alpha <= f32::EPSILON {
            pixel.copy_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        for channel in 0..3 {
            let premultiplied = text_color[channel] as f32 * text_alpha
                + halo_color[channel] as f32 * outline_alpha * (1.0 - text_alpha);
            pixel[channel] = (premultiplied / combined_alpha).round().clamp(0.0, 255.0) as u8;
        }
        pixel[3] = (combined_alpha * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    output
}

fn downsample_rgba(source: &[u8], width: u32, height: u32, factor: u32) -> Vec<u8> {
    if factor <= 1 {
        return source.to_vec();
    }
    let target_width = width / factor;
    let target_height = height / factor;
    let samples = (factor * factor) as u64;
    let mut output = vec![0_u8; (target_width * target_height * 4) as usize];

    for target_y in 0..target_height {
        for target_x in 0..target_width {
            let mut alpha_sum = 0_u64;
            let mut premultiplied = [0_u64; 3];
            for offset_y in 0..factor {
                for offset_x in 0..factor {
                    let source_x = target_x * factor + offset_x;
                    let source_y = target_y * factor + offset_y;
                    let source_index = ((source_y * width + source_x) * 4) as usize;
                    let alpha = source[source_index + 3] as u64;
                    alpha_sum += alpha;
                    for channel in 0..3 {
                        premultiplied[channel] += source[source_index + channel] as u64 * alpha;
                    }
                }
            }

            let destination_index = ((target_y * target_width + target_x) * 4) as usize;
            let averaged_alpha = (alpha_sum / samples) as u8;
            if alpha_sum > 0 {
                for channel in 0..3 {
                    output[destination_index + channel] =
                        (premultiplied[channel] / alpha_sum).min(255) as u8;
                }
            }
            output[destination_index + 3] = averaged_alpha;
        }
    }
    output
}

#[allow(clippy::too_many_arguments)]
fn alpha_blit(
    destination: &mut [u8],
    destination_width: u32,
    destination_height: u32,
    source: &[u8],
    source_width: u32,
    source_height: u32,
    x: u32,
    y: u32,
) {
    for source_y in 0..source_height {
        if y + source_y >= destination_height {
            break;
        }
        for source_x in 0..source_width {
            if x + source_x >= destination_width {
                break;
            }
            let source_index = ((source_y * source_width + source_x) * 4) as usize;
            let destination_index =
                (((y + source_y) * destination_width + x + source_x) * 4) as usize;
            let source_alpha = source[source_index + 3] as f32 / 255.0;
            let destination_alpha = destination[destination_index + 3] as f32 / 255.0;
            let combined_alpha = source_alpha + destination_alpha * (1.0 - source_alpha);
            if combined_alpha <= f32::EPSILON {
                continue;
            }
            for channel in 0..3 {
                let value = (source[source_index + channel] as f32 * source_alpha
                    + destination[destination_index + channel] as f32
                        * destination_alpha
                        * (1.0 - source_alpha))
                    / combined_alpha;
                destination[destination_index + channel] = value.round() as u8;
            }
            destination[destination_index + 3] = (combined_alpha * 255.0).round() as u8;
        }
    }
}

fn temporary_path(output: &Path) -> PathBuf {
    let mut file_name = output
        .file_name()
        .map(|value| value.to_os_string())
        .unwrap_or_else(|| "fileglyph.ico".into());
    file_name.push(".tmp");
    output.with_file_name(file_name)
}

fn replace_file(temporary: &Path, output: &Path) -> Result<()> {
    if output.exists() {
        fs::remove_file(output)
            .with_context(|| format!("failed to replace existing {}", output.display()))?;
    }
    fs::rename(temporary, output).with_context(|| {
        format!(
            "failed to move temporary file {} to {}",
            temporary.display(),
            output.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_filter_expands_signal() {
        let source = [0, 0, 10, 0, 0];
        let mut destination = [0; 5];
        sliding_max(&source, &mut destination, 1);
        assert_eq!(destination, [0, 10, 10, 10, 0]);
    }

    #[test]
    fn environment_expansion_keeps_unknown_variables() {
        assert_eq!(
            expand_environment("%FILEGLYPH_UNSET_TEST%\\x"),
            "%FILEGLYPH_UNSET_TEST%\\x"
        );
    }
}
