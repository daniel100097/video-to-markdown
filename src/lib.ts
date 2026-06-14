import { createTwoFilesPatch } from "diff";
import { mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import { join, parse } from "node:path";
import { PNG } from "pngjs";

export type Options = {
  video: string;
  fps: string;
  lang: string;
  output: string;
  everyNth: number;
  maxMotion: number;
};

export type OutputDirs = {
  framesDir: string;
  textDir: string;
  diffDir: string;
  markdownDir: string;
};

export type FrameSelectionOptions = {
  everyNth: number;
  maxMotion: number;
};

export type SelectedFrame = {
  file: string;
  motion: number | null;
};

export const usage =
  "Usage: bun run start <video> --fps 1 --lang deu --output result --every-nth 1 --max-motion 100";

const pixelChangeThreshold = 24;

export function parseArgs(args: string[]): Options {
  if (!args[0]) {
    throw new Error(usage);
  }

  const getArg = (name: string, fallback: string) => {
    const index = args.indexOf(name);
    return index >= 0 && args[index + 1] ? args[index + 1] : fallback;
  };

  return {
    video: args[0],
    fps: getArg("--fps", "1"),
    lang: getArg("--lang", "deu"),
    output: getArg("--output", "result"),
    everyNth: parsePositiveInteger(getArg("--every-nth", "1"), "--every-nth"),
    maxMotion: parsePercentage(getArg("--max-motion", "100"), "--max-motion"),
  };
}

function parsePositiveInteger(value: string, name: string): number {
  const parsed = Number(value);

  if (!Number.isInteger(parsed) || parsed < 1) {
    throw new Error(`${name} must be a positive integer`);
  }

  return parsed;
}

function parsePercentage(value: string, name: string): number {
  const parsed = Number(value);

  if (!Number.isFinite(parsed) || parsed < 0 || parsed > 100) {
    throw new Error(`${name} must be a number between 0 and 100`);
  }

  return parsed;
}

export function normalizeText(text: string): string {
  return text
    .split("\n")
    .map((line) => line.trim().replace(/\s+/g, " "))
    .filter(Boolean)
    .join("\n");
}

export function getOutputDirs(output: string): OutputDirs {
  return {
    framesDir: join(output, "frames"),
    textDir: join(output, "text"),
    diffDir: join(output, "diffs"),
    markdownDir: join(output, "markdown"),
  };
}

export function calculateMotionPercent(
  previous: PNG,
  current: PNG,
  threshold = pixelChangeThreshold,
): number {
  if (previous.width !== current.width || previous.height !== current.height) {
    throw new Error("Cannot compare frames with different dimensions");
  }

  let changedPixels = 0;
  const thresholdSquared = threshold * threshold;
  const totalPixels = previous.width * previous.height;

  for (let index = 0; index < previous.data.length; index += 4) {
    const redDelta = previous.data[index] - current.data[index];
    const greenDelta = previous.data[index + 1] - current.data[index + 1];
    const blueDelta = previous.data[index + 2] - current.data[index + 2];
    const distanceSquared =
      redDelta * redDelta + greenDelta * greenDelta + blueDelta * blueDelta;

    if (distanceSquared > thresholdSquared) {
      changedPixels++;
    }
  }

  return (changedPixels / totalPixels) * 100;
}

export async function selectFramesForOcr(
  framesDir: string,
  options: FrameSelectionOptions,
): Promise<SelectedFrame[]> {
  const frames = (await readdir(framesDir))
    .filter((file) => file.endsWith(".png"))
    .sort();

  const selectedFrames: SelectedFrame[] = [];
  let previousImage: PNG | null = null;
  let stableFrameIndex = 0;

  for (const frame of frames) {
    const currentImage = PNG.sync.read(await readFile(join(framesDir, frame)));
    const motion =
      previousImage === null
        ? null
        : calculateMotionPercent(previousImage, currentImage);
    const isStable = motion === null || motion <= options.maxMotion;

    previousImage = currentImage;

    if (!isStable) {
      continue;
    }

    if (stableFrameIndex % options.everyNth === 0) {
      selectedFrames.push({ file: frame, motion });
    }

    stableFrameIndex++;
  }

  return selectedFrames;
}

export async function writeDiffs(textDir: string, diffDir: string) {
  await mkdir(diffDir, { recursive: true });

  const files = (await readdir(textDir))
    .filter((file) => file.endsWith(".txt"))
    .sort();

  for (let i = 1; i < files.length; i++) {
    const previousFile = files[i - 1];
    const currentFile = files[i];

    const previousText = await readFile(join(textDir, previousFile), "utf8");
    const currentText = await readFile(join(textDir, currentFile), "utf8");

    const patch = createTwoFilesPatch(
      previousFile,
      currentFile,
      previousText,
      currentText,
      "",
      "",
      { context: 3 },
    );

    const diffName = `${parse(previousFile).name}_to_${parse(currentFile).name}.diff`;
    await writeFile(join(diffDir, diffName), patch, "utf8");
  }
}

export async function writeMarkdown(
  textDir: string,
  markdownDir: string,
  title: string,
) {
  await mkdir(markdownDir, { recursive: true });

  const files = (await readdir(textDir))
    .filter((file) => file.endsWith(".txt"))
    .sort();
  const sections = [`# ${title}`];

  for (const file of files) {
    const text = await readFile(join(textDir, file), "utf8");
    const frameName = parse(file).name;

    sections.push(`## ${frameName}`, text || "_No text detected._");
  }

  await writeFile(join(markdownDir, "ocr.md"), `${sections.join("\n\n")}\n`, "utf8");
}
