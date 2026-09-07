# yt-grab

A desktop GUI for [yt-dlp](https://github.com/yt-dlp/yt-dlp), with yt-dlp bundled
in. Paste a link, pick a quality, get a file.

Built with Tauri 2, React and TypeScript. macOS, Windows and Linux.

---

## What it does

**Nothing to install first.** yt-dlp ships inside the app. A macOS build is
~41 MB, of which yt-dlp is ~35 MB.

**Works past the first failure.** On sites other than YouTube, failing on the
first attempt is normal — and nearly every failure has a known fix. Errors are
classified and the fix is offered as a button rather than dumped as stderr:

| What happened | What you're offered |
| --- | --- |
| Site wants a login | Use browser cookies · Import cookies.txt |
| Rate limited | Slow down · Try again |
| yt-dlp too old for the site | Try again (see [self-update](#known-gaps)) |
| Quality not available here | Use best available |
| ffmpeg missing | How to install it |
| Couldn't write the file | Pick another folder |
| Geo-blocked, deleted, unsupported | Nothing — said plainly, no false hope |

**Remembers per site.** Accepting a fix writes a site rule, so you answer once.
Rules can also override the save folder, quality and audio-only per site — a
Bandcamp album and a YouTube video want different treatment.

**Doesn't assume YouTube.** Audio keeps the source codec by default instead of
transcoding everything to MP3; MP4 remuxing is opt-in rather than forced;
subtitle language is configurable; a source with no video track hides the
resolution picker entirely.

**Queues instead of stampeding.** A concurrency limit (default 3) caps how many
yt-dlp processes run at once, so a batch doesn't fork one downloader and one
ffmpeg merge per item.

**Accepts links from outside.** A `ytgrab://` URL scheme lets a browser
extension or bookmarklet hand links to the app.

---

## Requirements

**ffmpeg**, for combining video with audio and for converting audio:

```bash
brew install ffmpeg
```

The app detects it and shows a banner if it's absent. yt-dlp itself is bundled.

---

## Development

```bash
npm install
npm run tauri dev
```

`npm run tauri` fetches the yt-dlp binary for your platform first.

## Building

```bash
npm run tauri build
```

Bundles land in `src-tauri/target/release/bundle/`.

## Tests

```bash
cd src-tauri && cargo test
```

130 tests, covering yt-dlp command construction and output parsing,
progress-line recognition, final-path detection across the merge/extract/move
stages, failure classification and error-line selection, site-key extraction,
rule merging and storage, queue and cancellation behaviour, settings migration,
and deep-link parsing.

---

## The bundled yt-dlp

The binary is **fetched at build time, not committed** — it is 17–39 MB
depending on platform, and that does not belong in git history.
`src-tauri/binaries/` is gitignored.

```bash
npm run fetch-ytdlp              # latest, for this platform
npm run fetch-ytdlp -- --force   # re-download
YTDLP_VERSION=2026.08.19 npm run fetch-ytdlp
```

It writes `src-tauri/binaries/yt-dlp-<target-triple>`, which is what Tauri's
`externalBin` expects. The macOS asset is universal2, so one download covers
both Apple architectures.

At runtime yt-dlp is resolved in this order:

1. a copy in the app's data directory — reserved for a future self-updater
2. the bundled binary, next to the app executable
3. `yt-dlp` on `PATH`

On macOS and Linux the app also searches `/opt/homebrew/bin`, `/usr/local/bin`
and `~/.local/bin`, because a GUI app doesn't inherit your shell's `PATH`.

---

## Architecture

The UI talks to the backend through a single interface, so the planned UI
redesign can be done without losing functionality.

- **`src/api.ts`** — the only place the frontend calls `invoke`. Every operation
  the app supports is listed there.
- **`src-tauri/src/commands.rs`** — the matching Tauri command surface. Thin
  wrappers; the logic lives in a module.

The division of labour is **the UI says what, the backend decides how**. A grab
request names a URL and a format; the backend resolves the folder, cookies,
subtitle languages and container from settings plus the site rule. That is what
keeps a new UI from having to re-implement policy.

| Module | Responsibility |
| --- | --- |
| `tools.rs` | Finding yt-dlp and ffmpeg, versions, staleness |
| `ytdlp.rs` | Building yt-dlp command lines, parsing its output |
| `download.rs` | The queue, concurrency limit, process lifecycle |
| `diagnose.rs` | Classifying failures into kinds and remedies |
| `rules.rs` | Per-site rules, merged over global defaults |
| `store.rs` | All SQL — history, settings, site rules |
| `settings.rs` | Global defaults and validation |

### Commands

```
tool_status  resolve_url  diagnose_error
enqueue_grab  cancel_grab  queue_status  apply_remedy
list_site_rules  set_site_rule  delete_site_rule  site_for_url
get_history  completed_urls  delete_history_item  clear_history  purge_old_history
get_settings  save_settings  open_path
```

### Events

`download:started` · `download:progress` · `download:complete` ·
`download:error` · `deeplink:add`

---

## The `ytgrab://` scheme

```
ytgrab://add?url=<percent-encoded-url>
```

Opening one focuses the app, fills the bar and resolves the link.

It deliberately does **not** start a download. Any web page can trigger a deep
link, so the user still presses Grab; auto-starting would let a page cause
network fetches and disk writes with one click. Only `http(s)` targets are
accepted, so a link can't point yt-dlp at `file:///`.

Test it with:

```bash
open "ytgrab://add?url=https%3A%2F%2Fvimeo.com%2F76979871"
```

---

## Known gaps

- **No yt-dlp self-update yet.** The bundled copy sits inside a code-signed
  bundle, so rewriting it would break the signature, and the app-data path that
  would hold a downloaded replacement has no downloader. `tools::can_self_update()`
  returns `false` and gates the "Update yt-dlp" button in one place. Staleness
  *detection* is live — the header and a banner flag a build over 60 days old,
  which is the usual cause of a setup that worked yesterday and doesn't today.
- **ffmpeg is not bundled.** A static build is another 40–80 MB, which would put
  every platform over a 60 MB budget.
- **No playlist or multi-URL intake.** The scheduler that bulk downloading needs
  is in place; the intake and batch review UI are not. `--no-playlist` is still
  passed unconditionally.
- **Live streams** report no total size, so the progress bar can't fill. The
  media panel warns when a link is live.
- **The UI is an MVP.** The current three-tab layout is deliberate scaffolding
  over the interface above; the intended design is a single persistent list
  where history and queue are the same surface.
