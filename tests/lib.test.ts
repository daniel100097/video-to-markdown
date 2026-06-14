import { afterEach, describe, expect, test } from "bun:test";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { PNG } from "pngjs";
import {
  calculateMotionPercent,
  getOutputDirs,
  normalizeText,
  parseArgs,
  selectFramesForOcr,
  usage,
  writeDiffs,
  writeMarkdown,
} from "../src/lib";

let tempDirs: string[] = [];

async function makeTempDir() {
  const dir = await mkdtemp(join(tmpdir(), "video-to-markdown-"));
  tempDirs.push(dir);
  return dir;
}

function makePng(width: number, height: number, color: [number, number, number]) {
  const png = new PNG({ width, height });

  for (let index = 0; index < png.data.length; index += 4) {
    png.data[index] = color[0];
    png.data[index + 1] = color[1];
    png.data[index + 2] = color[2];
    png.data[index + 3] = 255;
  }

  return png;
}

function setPixel(png: PNG, x: number, y: number, color: [number, number, number]) {
  const index = (png.width * y + x) << 2;

  png.data[index] = color[0];
  png.data[index + 1] = color[1];
  png.data[index + 2] = color[2];
  png.data[index + 3] = 255;
}

async function writePng(
  filePath: string,
  width: number,
  height: number,
  color: [number, number, number],
) {
  await writeFile(filePath, PNG.sync.write(makePng(width, height, color)));
}

afterEach(async () => {
  await Promise.all(tempDirs.map((dir) => rm(dir, { recursive: true, force: true })));
  tempDirs = [];
});

describe("parseArgs", () => {
  test("uses defaults when optional flags are omitted", () => {
    expect(parseArgs(["input.mp4"])).toEqual({
      video: "input.mp4",
      fps: "1",
      lang: "deu",
      output: "result",
      everyNth: 1,
      maxMotion: 100,
    });
  });

  test("reads optional flags", () => {
    expect(
      parseArgs([
        "input.mp4",
        "--fps",
        "0.5",
        "--lang",
        "eng",
        "--output",
        "out",
        "--every-nth",
        "3",
        "--max-motion",
        "12.5",
      ]),
    ).toEqual({
      video: "input.mp4",
      fps: "0.5",
      lang: "eng",
      output: "out",
      everyNth: 3,
      maxMotion: 12.5,
    });
  });

  test("throws usage text when the video argument is missing", () => {
    expect(() => parseArgs([])).toThrow(usage);
  });

  test("rejects invalid frame selection values", () => {
    expect(() => parseArgs(["input.mp4", "--every-nth", "0"])).toThrow(
      "--every-nth must be a positive integer",
    );
    expect(() => parseArgs(["input.mp4", "--max-motion", "101"])).toThrow(
      "--max-motion must be a number between 0 and 100",
    );
  });
});

describe("normalizeText", () => {
  test("trims lines, collapses whitespace, and removes empty lines", () => {
    expect(normalizeText("  Hello    world  \n\n second\t\tline \n")).toBe(
      "Hello world\nsecond line",
    );
  });
});

describe("getOutputDirs", () => {
  test("returns stable output subdirectories", () => {
    expect(getOutputDirs("result")).toEqual({
      framesDir: join("result", "frames"),
      textDir: join("result", "text"),
      diffDir: join("result", "diffs"),
      markdownDir: join("result", "markdown"),
    });
  });
});

describe("calculateMotionPercent", () => {
  test("returns the percentage of changed pixels", () => {
    const previous = makePng(2, 2, [0, 0, 0]);
    const current = makePng(2, 2, [0, 0, 0]);

    setPixel(current, 1, 1, [255, 255, 255]);

    expect(calculateMotionPercent(previous, current, 0)).toBe(25);
  });

  test("rejects frames with different dimensions", () => {
    expect(() =>
      calculateMotionPercent(makePng(2, 2, [0, 0, 0]), makePng(3, 2, [0, 0, 0])),
    ).toThrow("Cannot compare frames with different dimensions");
  });
});

describe("selectFramesForOcr", () => {
  test("keeps every nth stable frame and skips high-motion frames", async () => {
    const root = await makeTempDir();
    const framesDir = join(root, "frames");

    await mkdir(framesDir);
    await writePng(join(framesDir, "frame_000001.png"), 2, 2, [0, 0, 0]);
    await writePng(join(framesDir, "frame_000002.png"), 2, 2, [0, 0, 0]);
    await writePng(join(framesDir, "frame_000003.png"), 2, 2, [255, 255, 255]);
    await writePng(join(framesDir, "frame_000004.png"), 2, 2, [255, 255, 255]);
    await writePng(join(framesDir, "frame_000005.png"), 2, 2, [255, 255, 255]);

    const selected = await selectFramesForOcr(framesDir, {
      everyNth: 2,
      maxMotion: 10,
    });

    expect(selected).toEqual([
      { file: "frame_000001.png", motion: null },
      { file: "frame_000004.png", motion: 0 },
    ]);
  });
});

describe("writeDiffs", () => {
  test("writes unified diffs between consecutive text files", async () => {
    const root = await makeTempDir();
    const textDir = join(root, "text");
    const diffDir = join(root, "diffs");

    await mkdir(textDir);
    await writeFile(join(textDir, "frame_000001.txt"), "Title\nA\n", "utf8");
    await writeFile(join(textDir, "frame_000002.txt"), "Title\nA\nB\n", "utf8");

    await writeDiffs(textDir, diffDir);

    const diff = await readFile(
      join(diffDir, "frame_000001_to_frame_000002.diff"),
      "utf8",
    );

    expect(diff).toContain("--- frame_000001.txt");
    expect(diff).toContain("+++ frame_000002.txt");
    expect(diff).toContain("+B");
  });

  test("does not create diffs when there is only one text file", async () => {
    const root = await makeTempDir();
    const textDir = join(root, "text");
    const diffDir = join(root, "diffs");

    await mkdir(textDir);
    await writeFile(join(textDir, "frame_000001.txt"), "Only frame\n", "utf8");

    await writeDiffs(textDir, diffDir);

    expect(await Array.fromAsync(new Bun.Glob("*.diff").scan(diffDir))).toEqual([]);
  });
});

describe("writeMarkdown", () => {
  test("writes one markdown file with a section per OCR text file", async () => {
    const root = await makeTempDir();
    const textDir = join(root, "text");
    const markdownDir = join(root, "markdown");

    await mkdir(textDir);
    await writeFile(join(textDir, "frame_000001.txt"), "Login\nE-Mail", "utf8");
    await writeFile(join(textDir, "frame_000002.txt"), "", "utf8");

    await writeMarkdown(textDir, markdownDir, "video.webm");

    const markdown = await readFile(join(markdownDir, "ocr.md"), "utf8");

    expect(markdown).toContain("# video.webm");
    expect(markdown).toContain("## frame_000001\n\nLogin\nE-Mail");
    expect(markdown).toContain("## frame_000002\n\n_No text detected._");
  });
});
