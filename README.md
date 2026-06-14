# Video to Markdown

Rust CLI that extracts stable OCR changes from a video with embedded `ffmpeg` and embedded OCR models, then writes change-only Markdown.

## Setup

```sh
cargo build --release
```

The release binary embeds the required ffmpeg binary and OCR model files. No external `ffmpeg`, `tesseract`, or Tesseract language data is required at runtime.

## Usage

```sh
target/release/video-to-markdown ./video.mp4 --fps 1 --lang deu --output result.md --every-nth 1 --max-motion 100
```

Options:

- `--fps`: frame extraction rate before filtering
- `--lang`: accepted for CLI compatibility; the embedded OCR engine does not load external language data
- `--output`: final Markdown file path
- `--every-nth`: OCR only every nth stable frame
- `--max-motion`: maximum changed screen area in percent between consecutive frames

Outputs:

- `/tmp/...`: temporary frames, OCR text, and diff working files
- `result.md`: Markdown containing only OCR changes; unchanged frames are omitted


## Development

```sh
cargo test
cargo build --release
```
