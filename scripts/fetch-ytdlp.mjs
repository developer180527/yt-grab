// Downloads the yt-dlp build for this platform into src-tauri/binaries/, named
// the way Tauri's `externalBin` expects (<name>-<target-triple>).
//
// The binary is fetched at build time rather than committed: it is 17–39 MB
// depending on platform, and a file that size in git history is permanent.
//
// Usage:  node scripts/fetch-ytdlp.mjs [--force]
//         YTDLP_VERSION=2026.08.19 node scripts/fetch-ytdlp.mjs

import { createWriteStream } from "node:fs";
import { chmod, mkdir, copyFile, stat } from "node:fs/promises";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const outDir = join(root, "src-tauri", "binaries");

// asset: the release file to download.
// triples: every Rust target triple that file is valid for. The macOS build is
// universal2, so one download serves both Apple architectures — which is what
// lets `tauri build --target universal-apple-darwin` work.
const PLATFORMS = {
  "darwin:arm64": { asset: "yt-dlp_macos", triples: ["aarch64-apple-darwin", "x86_64-apple-darwin"] },
  "darwin:x64":   { asset: "yt-dlp_macos", triples: ["aarch64-apple-darwin", "x86_64-apple-darwin"] },
  "win32:x64":    { asset: "yt-dlp.exe",         triples: ["x86_64-pc-windows-msvc"], ext: ".exe" },
  "win32:arm64":  { asset: "yt-dlp_arm64.exe",   triples: ["aarch64-pc-windows-msvc"], ext: ".exe" },
  "win32:ia32":   { asset: "yt-dlp_x86.exe",     triples: ["i686-pc-windows-msvc"], ext: ".exe" },
  "linux:x64":    { asset: "yt-dlp_linux",          triples: ["x86_64-unknown-linux-gnu"] },
  "linux:arm64":  { asset: "yt-dlp_linux_aarch64",  triples: ["aarch64-unknown-linux-gnu"] },
  "linux:arm":    { asset: "yt-dlp_linux_armv7l",   triples: ["armv7-unknown-linux-gnueabihf"] },
};

const key = `${process.platform}:${process.arch}`;
const target = PLATFORMS[key];
if (!target) {
  console.error(`fetch-ytdlp: no yt-dlp build for ${key}.`);
  console.error("Install yt-dlp yourself and the app will fall back to it on PATH.");
  process.exit(1);
}

const ext = target.ext ?? "";
const paths = target.triples.map((t) => join(outDir, `yt-dlp-${t}${ext}`));
const force = process.argv.includes("--force");

async function exists(p) {
  try {
    const s = await stat(p);
    return s.size > 0;
  } catch {
    return false;
  }
}

if (!force && (await Promise.all(paths.map(exists))).every(Boolean)) {
  console.log("fetch-ytdlp: already present, skipping (use --force to refresh).");
  process.exit(0);
}

const version = process.env.YTDLP_VERSION;
const base = version
  ? `https://github.com/yt-dlp/yt-dlp/releases/download/${version}`
  : "https://github.com/yt-dlp/yt-dlp/releases/latest/download";
const url = `${base}/${target.asset}`;

console.log(`fetch-ytdlp: downloading ${target.asset} (${version ?? "latest"})…`);

const res = await fetch(url, { redirect: "follow" });
if (!res.ok || !res.body) {
  console.error(`fetch-ytdlp: ${res.status} ${res.statusText} for ${url}`);
  process.exit(1);
}

await mkdir(outDir, { recursive: true });
const [first, ...rest] = paths;
await pipeline(Readable.fromWeb(res.body), createWriteStream(first));
await chmod(first, 0o755);

// The remaining triples share the same universal binary.
for (const p of rest) {
  await copyFile(first, p);
  await chmod(p, 0o755);
}

const mb = ((await stat(first)).size / 1048576).toFixed(1);
for (const p of paths) console.log(`fetch-ytdlp: wrote ${p} (${mb} MB)`);
