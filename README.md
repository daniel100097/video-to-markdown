# Video to Markdown

Rust CLI that extracts frames from a video with embedded `ffmpeg`, runs OCR with embedded OCR models, and writes text, diffs, and Markdown.

## Setup

```sh
cargo build --release
```

The release binary embeds the required ffmpeg binary and OCR model files. No external `ffmpeg`, `tesseract`, or Tesseract language data is required at runtime.

## Usage

```sh
target/release/video-to-markdown ./video.mp4 --fps 1 --lang deu --output result --every-nth 1 --max-motion 100
```

Options:

- `--fps`: frame extraction rate before filtering
- `--lang`: accepted for CLI compatibility; the embedded OCR engine does not load external language data
- `--output`: output directory
- `--every-nth`: OCR only every nth stable frame
- `--max-motion`: maximum changed screen area in percent between consecutive frames

Outputs:

- `result/frames`: extracted PNG frames
- `result/text`: normalized OCR text for each frame
- `result/diffs`: unified diffs between consecutive text files
- `result/markdown/ocr.md`: Markdown view of all OCR snapshots


## Development

```sh
cargo test
cargo build --release
```
