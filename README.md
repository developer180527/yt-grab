# yt-grab

A desktop GUI for [yt-dlp](https://github.com/yt-dlp/yt-dlp). Paste a URL, pick a
quality, get a file. Built with Tauri 2, React and TypeScript.

- Video downloads with quality presets (best / 1080p / 720p / 480p) or any exact
  format yt-dlp reports
- Audio-only MP3 extraction
- Live progress with speed, ETA and the current stage (video → audio → merging)
- Download history in SQLite, with optional auto-expiry
- Optional cover-art embedding, subtitle embedding, rate limiting and
  cookies-from-browser for age-restricted content

## Requirements

Both must be on `PATH`:

- **yt-dlp** — the downloader itself
- **ffmpeg** — required to merge video with audio and to write MP3s

```bash
brew install yt-dlp ffmpeg
```

The app checks for both at launch and shows a banner if either is missing. On
macOS and Linux it also searches `/opt/homebrew/bin`, `/usr/local/bin` and
`~/.local/bin`, since a GUI app doesn't inherit your shell's `PATH`.

## Development

```bash
npm install
npm run tauri dev
```

## Building

```bash
npm run tauri build
```

Bundles are written to `src-tauri/target/release/bundle/`.

## Icons

The master artwork lives in `Assets/`. To regenerate every platform's icon set
(macOS `.icns`, Windows `.ico`, Linux PNGs, plus iOS/Android):

```bash
npm run tauri icon Assets/yt_grab_#3.icns
```

The generated files land in `src-tauri/icons/` and **must be committed** —
`tauri build` reads them from there, and the list in `tauri.conf.json` under
`bundle.icon` is what actually gets bundled.

## Tests

```bash
cd src-tauri && cargo test
```

Covers yt-dlp output parsing: progress lines, final-path detection across the
merge/extract/move stages, and rate-limit validation.
