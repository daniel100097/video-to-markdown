#!/usr/bin/env bun

import ffmpegPath from "ffmpeg-static";
import { createWorker } from "tesseract.js";
import { spawn } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { basename, join, parse } from "node:path";
import {
  type FrameSelectionOptions,
  getOutputDirs,
  normalizeText,
  parseArgs,
  selectFramesForOcr,
  usage,
  writeDiffs,
  writeMarkdown,
  type Options,
} from "./lib";

export async function extractFrames(video: string, framesDir: string, fps: string) {
  await mkdir(framesDir, { recursive: true });

  if (!ffmpegPath) {
    throw new Error("ffmpeg binary is not available for this platform");
  }

  await runFfmpeg([
    "-hide_banner",
    "-loglevel",
    "error",
    "-y",
    "-i",
    video,
    "-vf",
    `fps=${fps}`,
    join(framesDir, "frame_%06d.png"),
  ]);
}

async function runFfmpeg(args: string[]) {
  if (!ffmpegPath) {
    throw new Error("ffmpeg binary is not available for this platform");
  }

  const process = spawn(ffmpegPath, args, {
    stdio: ["ignore", "ignore", "pipe"],
  });

  let stderr = "";

  process.stderr.setEncoding("utf8");
  process.stderr.on("data", (chunk) => {
    stderr += chunk;
  });

  await new Promise<void>((resolve, reject) => {
    process.on("error", reject);
    process.on("close", (code) => {
      if (code === 0) {
        resolve();
        return;
      }

      reject(new Error(`ffmpeg failed with exit code ${code}: ${stderr.trim()}`));
    });
  });
}

export async function extractTextFromFrames(
  framesDir: string,
  textDir: string,
  lang: string,
  selectionOptions: FrameSelectionOptions,
) {
  await mkdir(textDir, { recursive: true });

  const worker = await createWorker(lang);
  const frames = await selectFramesForOcr(framesDir, selectionOptions);

  console.log(`OCR-Frames: ${frames.length}`);

  try {
    for (const frame of frames) {
      const framePath = join(framesDir, frame.file);
      const result = await worker.recognize(framePath);

      const text = normalizeText(result.data.text);
      const textPath = join(textDir, `${parse(frame.file).name}.txt`);

      await writeFile(textPath, text, "utf8");
    }
  } finally {
    await worker.terminate();
  }
}

export async function run(options: Options) {
  const { framesDir, textDir, diffDir, markdownDir } = getOutputDirs(options.output);

  console.log("Extrahiere Frames mit ffmpeg...");
  await extractFrames(options.video, framesDir, options.fps);

  console.log("Extrahiere Text mit tesseract.js...");
  await extractTextFromFrames(framesDir, textDir, options.lang, {
    everyNth: options.everyNth,
    maxMotion: options.maxMotion,
  });

  console.log("Erzeuge Diffs...");
  await writeDiffs(textDir, diffDir);

  console.log("Erzeuge Markdown...");
  await writeMarkdown(textDir, markdownDir, basename(options.video));

  console.log("Fertig.");
  console.log(`Frames: ${framesDir}`);
  console.log(`Texte:  ${textDir}`);
  console.log(`Diffs:  ${diffDir}`);
  console.log(`Markdown: ${join(markdownDir, "ocr.md")}`);
}

export async function main(args = Bun.argv.slice(2)) {
  let options: Options;

  try {
    options = parseArgs(args);
  } catch (error) {
    console.error(error instanceof Error ? error.message : usage);
    process.exit(1);
  }

  await run(options);
}

if (import.meta.main) {
  main().catch((error) => {
    console.error(error);
    process.exit(1);
  });
}
