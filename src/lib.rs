use anyhow::{Context, Result, bail};
use clap::Parser;
use image::ImageReader;
use similar::TextDiff;
use std::fs;
use std::path::{Path, PathBuf};

const PIXEL_CHANGE_THRESHOLD: u8 = 24;

#[derive(Debug, Clone, Parser, PartialEq)]
#[command(
    name = "video-to-markdown",
    about = "Extract stable OCR changes from a video and write change-only Markdown."
)]
pub struct Options {
    pub video: PathBuf,

    #[arg(long, default_value = "1")]
    pub fps: String,

    #[arg(long, default_value = "deu")]
    pub lang: String,

    #[arg(long, default_value = "result.md")]
    pub output: PathBuf,

    #[arg(long = "every-nth", default_value_t = 1, value_parser = parse_positive_usize)]
    pub every_nth: usize,

    #[arg(long = "max-motion", default_value_t = 100.0, value_parser = parse_percentage)]
    pub max_motion: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct FrameSelectionOptions {
    pub every_nth: usize,
    pub max_motion: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelectedFrame {
    pub file: String,
    pub motion: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbFrame {
    width: u32,
    height: u32,
    pixels: Vec<[u8; 3]>,
}

impl RgbFrame {
    pub fn new(width: u32, height: u32, pixels: Vec<[u8; 3]>) -> Result<Self> {
        let expected_pixels = width as usize * height as usize;

        if expected_pixels == 0 {
            bail!("Frame dimensions must be greater than zero");
        }

        if pixels.len() != expected_pixels {
            bail!(
                "Expected {expected_pixels} pixels for {width}x{height}, got {}",
                pixels.len()
            );
        }

        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    pub fn solid(width: u32, height: u32, color: [u8; 3]) -> Result<Self> {
        Self::new(width, height, vec![color; width as usize * height as usize])
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, color: [u8; 3]) -> Result<()> {
        if x >= self.width || y >= self.height {
            bail!("Pixel coordinate is outside the frame");
        }

        let index = (self.width * y + x) as usize;
        self.pixels[index] = color;
        Ok(())
    }
}

pub fn parse_percentage(value: &str) -> std::result::Result<f64, String> {
    let parsed = value
        .parse::<f64>()
        .map_err(|_| "--max-motion must be a number between 0 and 100".to_string())?;

    if !(0.0..=100.0).contains(&parsed) {
        return Err("--max-motion must be a number between 0 and 100".to_string());
    }

    Ok(parsed)
}

pub fn parse_positive_usize(value: &str) -> std::result::Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| "--every-nth must be a positive integer".to_string())?;

    if parsed == 0 {
        return Err("--every-nth must be a positive integer".to_string());
    }

    Ok(parsed)
}

pub fn normalize_text(text: &str) -> String {
    text.lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn get_markdown_path(output: impl AsRef<Path>) -> PathBuf {
    output.as_ref().to_path_buf()
}

pub fn read_png_frame(path: impl AsRef<Path>) -> Result<RgbFrame> {
    let image = ImageReader::open(path.as_ref())
        .with_context(|| format!("Failed to open {}", path.as_ref().display()))?
        .decode()
        .with_context(|| format!("Failed to decode {}", path.as_ref().display()))?
        .to_rgb8();
    let (width, height) = image.dimensions();
    let pixels = image.pixels().map(|pixel| pixel.0).collect();

    RgbFrame::new(width, height, pixels)
}

pub fn calculate_motion_percent(previous: &RgbFrame, current: &RgbFrame) -> Result<f64> {
    if previous.width != current.width || previous.height != current.height {
        bail!("Cannot compare frames with different dimensions");
    }

    let threshold_squared = i32::from(PIXEL_CHANGE_THRESHOLD).pow(2);
    let changed_pixels = previous
        .pixels
        .iter()
        .zip(&current.pixels)
        .filter(|(previous_pixel, current_pixel)| {
            let red_delta = i32::from(previous_pixel[0]) - i32::from(current_pixel[0]);
            let green_delta = i32::from(previous_pixel[1]) - i32::from(current_pixel[1]);
            let blue_delta = i32::from(previous_pixel[2]) - i32::from(current_pixel[2]);
            let distance_squared =
                red_delta * red_delta + green_delta * green_delta + blue_delta * blue_delta;

            distance_squared > threshold_squared
        })
        .count();

    Ok((changed_pixels as f64 / previous.pixels.len() as f64) * 100.0)
}

pub fn select_frames_for_ocr(
    frames_dir: impl AsRef<Path>,
    options: FrameSelectionOptions,
) -> Result<Vec<SelectedFrame>> {
    let mut frames = png_files(frames_dir.as_ref())?;
    let mut selected_frames = Vec::new();
    let mut previous_image: Option<RgbFrame> = None;
    let mut stable_frame_index = 0usize;

    frames.sort();

    for frame_path in frames {
        let current_image = read_png_frame(&frame_path)?;
        let motion = previous_image
            .as_ref()
            .map(|previous| calculate_motion_percent(previous, &current_image))
            .transpose()?;
        let is_stable = motion.is_none_or(|motion| motion <= options.max_motion);

        previous_image = Some(current_image);

        if !is_stable {
            continue;
        }

        if stable_frame_index % options.every_nth == 0 {
            let file = frame_path
                .file_name()
                .context("Frame path has no file name")?
                .to_string_lossy()
                .into_owned();

            selected_frames.push(SelectedFrame { file, motion });
        }

        stable_frame_index += 1;
    }

    Ok(selected_frames)
}

pub fn write_diffs(text_dir: impl AsRef<Path>, diff_dir: impl AsRef<Path>) -> Result<()> {
    fs::create_dir_all(diff_dir.as_ref())?;

    let mut files = text_files(text_dir.as_ref())?;
    files.sort();

    for pair in files.windows(2) {
        let previous_path = &pair[0];
        let current_path = &pair[1];
        let previous_file = file_name(previous_path)?;
        let current_file = file_name(current_path)?;
        let previous_text = fs::read_to_string(previous_path)
            .with_context(|| format!("Failed to read {}", previous_path.display()))?;
        let current_text = fs::read_to_string(current_path)
            .with_context(|| format!("Failed to read {}", current_path.display()))?;
        let patch = TextDiff::from_lines(&previous_text, &current_text)
            .unified_diff()
            .context_radius(3)
            .header(&previous_file, &current_file)
            .to_string();
        let previous_stem = file_stem(previous_path)?;
        let current_stem = file_stem(current_path)?;
        let diff_name = format!("{previous_stem}_to_{current_stem}.diff");

        fs::write(diff_dir.as_ref().join(diff_name), patch)?;
    }

    Ok(())
}

pub fn write_change_markdown(
    text_dir: impl AsRef<Path>,
    markdown_path: impl AsRef<Path>,
    title: &str,
) -> Result<PathBuf> {
    let markdown_path = markdown_path.as_ref();

    if let Some(parent) = markdown_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }

    let mut files = text_files(text_dir.as_ref())?;
    files.sort();

    let mut sections = vec![
        format!("# {title}"),
        "Only OCR changes are shown. Unchanged frames are omitted.".to_string(),
    ];
    let mut previous_file = "empty.txt".to_string();
    let mut previous_text = String::new();

    for file in files {
        let current_text = fs::read_to_string(&file)
            .with_context(|| format!("Failed to read {}", file.display()))?;
        let current_file = file_name(&file)?;

        if current_text == previous_text {
            previous_file = current_file;
            continue;
        }

        let patch = TextDiff::from_lines(&previous_text, &current_text)
            .unified_diff()
            .context_radius(3)
            .header(&previous_file, &current_file)
            .to_string();

        if !patch.trim().is_empty() {
            let frame_name = file_stem(&file)?;
            sections.push(format!(
                "## {frame_name}\n\n```diff\n{}\n```",
                patch.trim_end()
            ));
        }

        previous_file = current_file;
        previous_text = current_text;
    }

    if sections.len() == 2 {
        sections.push("_No OCR changes detected._".to_string());
    }

    fs::write(markdown_path, format!("{}\n", sections.join("\n\n")))?;

    Ok(markdown_path.to_path_buf())
}

fn png_files(dir: &Path) -> Result<Vec<PathBuf>> {
    files_with_extension(dir, "png")
}

fn text_files(dir: &Path) -> Result<Vec<PathBuf>> {
    files_with_extension(dir, "txt")
}

fn files_with_extension(dir: &Path, extension: &str) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for entry in fs::read_dir(dir).with_context(|| format!("Failed to read {}", dir.display()))? {
        let path = entry?.path();

        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|actual| actual.eq_ignore_ascii_case(extension))
        {
            files.push(path);
        }
    }

    Ok(files)
}

fn file_name(path: &Path) -> Result<String> {
    Ok(path
        .file_name()
        .context("Path has no file name")?
        .to_string_lossy()
        .into_owned())
}

fn file_stem(path: &Path) -> Result<String> {
    Ok(path
        .file_stem()
        .context("Path has no file stem")?
        .to_string_lossy()
        .into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use image::{Rgb, RgbImage};
    use tempfile::tempdir;

    #[test]
    fn parses_defaults() {
        let options = Options::parse_from(["video-to-markdown", "input.mp4"]);

        assert_eq!(options.video, PathBuf::from("input.mp4"));
        assert_eq!(options.fps, "1");
        assert_eq!(options.lang, "deu");
        assert_eq!(options.output, PathBuf::from("result.md"));
        assert_eq!(options.every_nth, 1);
        assert_eq!(options.max_motion, 100.0);
    }

    #[test]
    fn parses_optional_flags() {
        let options = Options::parse_from([
            "video-to-markdown",
            "input.mp4",
            "--fps",
            "0.5",
            "--lang",
            "eng",
            "--output",
            "out.md",
            "--every-nth",
            "3",
            "--max-motion",
            "12.5",
        ]);

        assert_eq!(options.video, PathBuf::from("input.mp4"));
        assert_eq!(options.fps, "0.5");
        assert_eq!(options.lang, "eng");
        assert_eq!(options.output, PathBuf::from("out.md"));
        assert_eq!(options.every_nth, 3);
        assert_eq!(options.max_motion, 12.5);
    }

    #[test]
    fn rejects_invalid_frame_selection_values() {
        assert!(
            Options::try_parse_from(["video-to-markdown", "input.mp4", "--every-nth", "0"])
                .is_err()
        );
        assert!(
            Options::try_parse_from(["video-to-markdown", "input.mp4", "--max-motion", "101"])
                .is_err()
        );
    }

    #[test]
    fn normalizes_text() {
        assert_eq!(
            normalize_text("  Hello    world  \n\n second\t\tline \n"),
            "Hello world\nsecond line"
        );
    }

    #[test]
    fn returns_markdown_path() {
        assert_eq!(get_markdown_path("result.md"), PathBuf::from("result.md"));
    }

    #[test]
    fn calculates_motion_percent() -> Result<()> {
        let previous = RgbFrame::solid(2, 2, [0, 0, 0])?;
        let mut current = RgbFrame::solid(2, 2, [0, 0, 0])?;

        current.set_pixel(1, 1, [255, 255, 255])?;

        assert_eq!(calculate_motion_percent(&previous, &current)?, 25.0);
        Ok(())
    }

    #[test]
    fn rejects_frames_with_different_dimensions() -> Result<()> {
        let previous = RgbFrame::solid(2, 2, [0, 0, 0])?;
        let current = RgbFrame::solid(3, 2, [0, 0, 0])?;

        assert!(calculate_motion_percent(&previous, &current).is_err());
        Ok(())
    }

    #[test]
    fn selects_every_nth_stable_frame() -> Result<()> {
        let temp = tempdir()?;
        let frames_dir = temp.path().join("frames");

        fs::create_dir(&frames_dir)?;
        write_png(frames_dir.join("frame_000001.png"), [0, 0, 0])?;
        write_png(frames_dir.join("frame_000002.png"), [0, 0, 0])?;
        write_png(frames_dir.join("frame_000003.png"), [255, 255, 255])?;
        write_png(frames_dir.join("frame_000004.png"), [255, 255, 255])?;
        write_png(frames_dir.join("frame_000005.png"), [255, 255, 255])?;

        let selected = select_frames_for_ocr(
            &frames_dir,
            FrameSelectionOptions {
                every_nth: 2,
                max_motion: 10.0,
            },
        )?;

        assert_eq!(
            selected,
            vec![
                SelectedFrame {
                    file: "frame_000001.png".to_string(),
                    motion: None,
                },
                SelectedFrame {
                    file: "frame_000004.png".to_string(),
                    motion: Some(0.0),
                },
            ]
        );
        Ok(())
    }

    #[test]
    fn writes_unified_diffs() -> Result<()> {
        let temp = tempdir()?;
        let text_dir = temp.path().join("text");
        let diff_dir = temp.path().join("diffs");

        fs::create_dir(&text_dir)?;
        fs::write(text_dir.join("frame_000001.txt"), "Title\nA\n")?;
        fs::write(text_dir.join("frame_000002.txt"), "Title\nA\nB\n")?;

        write_diffs(&text_dir, &diff_dir)?;

        let diff = fs::read_to_string(diff_dir.join("frame_000001_to_frame_000002.diff"))?;

        assert!(diff.contains("--- frame_000001.txt"));
        assert!(diff.contains("+++ frame_000002.txt"));
        assert!(diff.contains("+B"));
        Ok(())
    }

    #[test]
    fn writes_no_diffs_for_one_text_file() -> Result<()> {
        let temp = tempdir()?;
        let text_dir = temp.path().join("text");
        let diff_dir = temp.path().join("diffs");

        fs::create_dir(&text_dir)?;
        fs::write(text_dir.join("frame_000001.txt"), "Only frame\n")?;

        write_diffs(&text_dir, &diff_dir)?;

        assert_eq!(fs::read_dir(diff_dir)?.count(), 0);
        Ok(())
    }

    #[test]
    fn writes_change_only_markdown() -> Result<()> {
        let temp = tempdir()?;
        let text_dir = temp.path().join("text");
        let markdown_path = temp.path().join("markdown").join("ocr.md");

        fs::create_dir(&text_dir)?;
        fs::write(text_dir.join("frame_000001.txt"), "")?;
        fs::write(text_dir.join("frame_000002.txt"), "Login\nE-Mail\n")?;
        fs::write(text_dir.join("frame_000003.txt"), "Login\nE-Mail\n")?;
        fs::write(text_dir.join("frame_000004.txt"), "Login\nPassword\n")?;

        let written_path = write_change_markdown(&text_dir, &markdown_path, "video.webm")?;
        let markdown = fs::read_to_string(written_path)?;

        assert!(markdown.contains("# video.webm"));
        assert!(markdown.contains("Only OCR changes are shown"));
        assert!(!markdown.contains("## frame_000001"));
        assert!(markdown.contains("## frame_000002"));
        assert!(!markdown.contains("## frame_000003"));
        assert!(markdown.contains("## frame_000004"));
        assert!(markdown.contains("+Login"));
        assert!(markdown.contains("-E-Mail"));
        assert!(markdown.contains("+Password"));
        Ok(())
    }

    fn write_png(path: impl AsRef<Path>, color: [u8; 3]) -> Result<()> {
        let image = RgbImage::from_pixel(2, 2, Rgb(color));
        image.save(path)?;
        Ok(())
    }
}
