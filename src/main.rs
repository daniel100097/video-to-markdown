use anyhow::{Context, Result, bail};
use clap::Parser;
use flate2::read::GzDecoder;
use ocrs::{ImageSource, OcrEngine, OcrEngineParams};
use rten::Model;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::copy;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::{TempDir, tempdir};
use video_to_markdown::{
    FrameSelectionOptions, Options, get_output_dirs, normalize_text, select_frames_for_ocr,
    write_diffs, write_markdown,
};

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const FFMPEG_GZ_BYTES: &[u8] = include_bytes!("../assets/ffmpeg-linux-x64.gz");
const TEXT_DETECTION_MODEL_BYTES: &[u8] = include_bytes!("../assets/text-detection.rten");
const TEXT_RECOGNITION_MODEL_BYTES: &[u8] = include_bytes!("../assets/text-recognition.rten");

struct EmbeddedExecutable {
    path: PathBuf,
    _temp_dir: TempDir,
}

fn main() {
    if let Err(error) = run(Options::parse()) {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}

fn run(options: Options) -> Result<()> {
    let output_dirs = get_output_dirs(&options.output);

    println!("Extrahiere Frames mit ffmpeg...");
    extract_frames(&options.video, &output_dirs.frames_dir, &options.fps)?;

    println!("Extrahiere Text mit eingebetteter OCR...");
    extract_text_from_frames(
        &output_dirs.frames_dir,
        &output_dirs.text_dir,
        &options.lang,
        FrameSelectionOptions {
            every_nth: options.every_nth,
            max_motion: options.max_motion,
        },
    )?;

    println!("Erzeuge Diffs...");
    write_diffs(&output_dirs.text_dir, &output_dirs.diff_dir)?;

    println!("Erzeuge Markdown...");
    let markdown_path = write_markdown(
        &output_dirs.text_dir,
        &output_dirs.markdown_dir,
        &options
            .video
            .file_name()
            .unwrap_or_default()
            .to_string_lossy(),
    )?;

    println!("Fertig.");
    println!("Frames: {}", output_dirs.frames_dir.display());
    println!("Texte:  {}", output_dirs.text_dir.display());
    println!("Diffs:  {}", output_dirs.diff_dir.display());
    println!("Markdown: {}", markdown_path.display());

    Ok(())
}

fn extract_frames(video: &Path, frames_dir: &Path, fps: &str) -> Result<()> {
    fs::create_dir_all(frames_dir)?;

    let ffmpeg = materialize_embedded_ffmpeg()?;
    let output_pattern = frames_dir.join("frame_%06d.png");
    let output = Command::new(&ffmpeg.path)
        .args([
            OsString::from("-hide_banner"),
            OsString::from("-loglevel"),
            OsString::from("error"),
            OsString::from("-y"),
            OsString::from("-i"),
            video.as_os_str().to_os_string(),
            OsString::from("-vf"),
            OsString::from(format!("fps={fps}")),
            output_pattern.as_os_str().to_os_string(),
        ])
        .output()
        .with_context(|| format!("Failed to run {}", ffmpeg.path.display()))?;

    if !output.status.success() {
        bail!(
            "ffmpeg failed with exit code {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(())
}

fn extract_text_from_frames(
    frames_dir: &Path,
    text_dir: &Path,
    _lang: &str,
    selection_options: FrameSelectionOptions,
) -> Result<()> {
    fs::create_dir_all(text_dir)?;

    let engine = embedded_ocr_engine()?;
    let frames = select_frames_for_ocr(frames_dir, selection_options)?;

    println!("OCR-Frames: {}", frames.len());

    for frame in frames {
        let frame_path = frames_dir.join(&frame.file);
        let text = normalize_text(&recognize_frame_text(&engine, &frame_path)?);
        let text_path = text_dir.join(format!(
            "{}.txt",
            Path::new(&frame.file)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
        ));

        fs::write(text_path, text)?;
    }

    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn materialize_embedded_ffmpeg() -> Result<EmbeddedExecutable> {
    let temp_dir = tempdir()?;
    let path = temp_dir.path().join("ffmpeg");
    let mut decoder = GzDecoder::new(FFMPEG_GZ_BYTES);
    let mut file = File::create(&path)?;

    copy(&mut decoder, &mut file)?;

    #[cfg(unix)]
    {
        let mut permissions = file.metadata()?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions)?;
    }

    Ok(EmbeddedExecutable {
        path,
        _temp_dir: temp_dir,
    })
}

#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn materialize_embedded_ffmpeg() -> Result<EmbeddedExecutable> {
    bail!("embedded ffmpeg is currently bundled only for linux x86_64")
}

fn embedded_ocr_engine() -> Result<OcrEngine> {
    let detection_model = Model::load_static_slice(TEXT_DETECTION_MODEL_BYTES)?;
    let recognition_model = Model::load_static_slice(TEXT_RECOGNITION_MODEL_BYTES)?;

    OcrEngine::new(OcrEngineParams {
        detection_model: Some(detection_model),
        recognition_model: Some(recognition_model),
        ..Default::default()
    })
}

fn recognize_frame_text(engine: &OcrEngine, frame_path: &Path) -> Result<String> {
    let image = image::open(frame_path)
        .with_context(|| format!("Failed to open {}", frame_path.display()))?
        .into_rgb8();
    let image_source = ImageSource::from_bytes(image.as_raw(), image.dimensions())?;
    let input = engine.prepare_input(image_source)?;

    engine.get_text(&input)
}
