# Video to Markdown

Extract frames from a video with ffmpeg, run OCR on each stable frame with Tesseract, and write unified diffs between consecutive OCR snapshots.

## Setup

```sh
bun install
```

## Usage

```sh
bun run start ./video.mp4 --fps 1 --lang deu --output result --every-nth 1 --max-motion 100
```

Options:

- `--fps`: frame extraction rate before filtering
- `--lang`: Tesseract language code
- `--output`: output directory
- `--every-nth`: OCR only every nth stable frame
- `--max-motion`: maximum changed screen area in percent between consecutive frames

Outputs:

- `result/frames`: extracted PNG frames
- `result/text`: normalized OCR text for each frame
- `result/diffs`: unified diffs between consecutive text files
- `result/markdown/ocr.md`: Markdown view of all OCR snapshots

## Fixtures

The Playwright result videos copied from `hosting-manager-v2` live in `tests/fixtures/videos`.

## Development

```sh
bun test
bun run typecheck
```
