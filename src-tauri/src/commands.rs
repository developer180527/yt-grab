use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use tauri::{AppHandle, Emitter, State};

// ─── App State ────────────────────────────────────────────────────────────────

pub struct AppState {
    pub downloads: Arc<Mutex<HashMap<String, Child>>>,
    /// Ids the user explicitly cancelled. Lets the reaper thread tell a kill
    /// apart from a genuine failure so cancelled items aren't logged as errors.
    pub cancelled: Arc<Mutex<HashSet<String>>>,
    pub db: Arc<Mutex<rusqlite::Connection>>,
}

/// `Mutex::lock` only fails if another thread panicked while holding the guard.
/// The data we keep behind these mutexes stays consistent in that case, so
/// recover instead of poisoning every later command with a panic.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct YtFormat {
    pub format_id: String,
    pub ext: String,
    pub resolution: String,
    pub filesize: Option<u64>,
    pub vcodec: String,
    pub acodec: String,
    pub format_note: String,
    pub tbr: Option<f64>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct MediaInfo {
    pub id: String,
    pub title: String,
    pub thumbnail: Option<String>,
    pub duration: Option<f64>,
    pub uploader: Option<String>,
    pub formats: Vec<YtFormat>,
    pub webpage_url: String,
}

#[derive(serde::Serialize, Clone)]
pub struct ProgressPayload {
    pub id: String,
    pub percent: f64,
    pub speed: String,
    pub eta: String,
    pub size: String,
    /// "video" | "audio" | "merging" | "processing" — explains why the bar
    /// restarts at 0% when yt-dlp fetches video and audio as separate streams.
    pub stage: String,
}

#[derive(serde::Serialize, Clone)]
pub struct CompletePayload {
    pub id: String,
    pub path: String,
    /// Size of the finished file on disk. The last progress line only reports
    /// the size of the final *stream*, which understates a merged video+audio
    /// download by however much the video track weighed.
    pub size: Option<u64>,
}

#[derive(serde::Serialize, Clone)]
pub struct ErrorPayload { pub id: String, pub message: String }

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct HistoryItem {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub thumbnail: Option<String>,
    pub format_id: String,
    pub audio_only: bool,
    pub output_path: Option<String>,
    pub status: String,
    pub created_at: String,
}

/// `#[serde(default)]` makes the stored JSON forward- and backward-compatible:
/// a settings blob written by a different build can gain or lose fields without
/// the whole thing failing to parse and silently resetting to defaults.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub default_save_folder: String,
    pub default_format: String,
    pub embed_thumbnail: bool,
    pub embed_subtitles: bool,
    pub speed_limit: String,          // "" = no limit, "2M", "500K", etc.
    pub cookies_browser: String,      // "" = off, "chrome", "firefox", "safari", "brave", "edge"
    pub auto_open_folder: bool,
    pub auto_delete_history_days: u32, // 0 = never
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_save_folder: String::new(),
            default_format: "best".to_string(),
            embed_thumbnail: false,
            embed_subtitles: false,
            speed_limit: String::new(),
            cookies_browser: String::new(),
            auto_open_folder: false,
            auto_delete_history_days: 0,
        }
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Builds a `Command` for an external tool, patched so it works when the app is
/// launched from the desktop rather than a shell.
fn tool_cmd(program: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);

    // GUI apps inherit a minimal PATH from the launcher (Finder, GNOME, ...),
    // which usually misses Homebrew and pip's --user bin dirs.
    #[cfg(unix)]
    {
        let cur = std::env::var("PATH").unwrap_or_default();
        let mut extra = vec![
            "/opt/homebrew/bin".to_string(),
            "/usr/local/bin".to_string(),
            "/usr/bin".to_string(),
            "/bin".to_string(),
        ];
        if let Ok(home) = std::env::var("HOME") {
            extra.push(format!("{}/.local/bin", home));
        }
        extra.push(cur);
        cmd.env("PATH", extra.join(":"));
    }

    // Without this every yt-dlp invocation flashes a console window on Windows.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    cmd
}

fn ytdlp_cmd() -> std::process::Command {
    tool_cmd("yt-dlp")
}

/// Parses one `[download] ... %` progress line.
///
/// Shapes handled:
///   `[download]   4.2% of   10.00MiB at    1.05MiB/s ETA 00:09`
///   `[download]  50.0% of ~ 10.00MiB at Unknown B/s ETA Unknown (frag 3/10)`
fn parse_progress(line: &str) -> (f64, String, String, String) {
    let mut percent = 0.0_f64;
    let mut speed = "--".to_string();
    let mut eta = "--".to_string();
    let mut size = "--".to_string();

    if let Some(pct_str) = line.split('%').next() {
        if let Some(num) = pct_str.split_whitespace().last() {
            percent = num.parse().unwrap_or(0.0);
        }
    }
    percent = percent.clamp(0.0, 100.0);

    if let Some(of_part) = line.split(" of ").nth(1) {
        // The `~` marking an estimated size is a separate whitespace-delimited
        // token, so skip tokens that are only a tilde before taking the value.
        if let Some(s) = of_part
            .split_whitespace()
            .map(|t| t.trim_start_matches('~'))
            .find(|t| !t.is_empty())
        {
            size = s.to_string();
        }
    }
    if let Some(at_part) = line.split(" at ").nth(1) {
        speed = at_part.split_whitespace().next().unwrap_or("--").to_string();
    }
    if let Some(eta_part) = line.split("ETA ").nth(1) {
        let mut e = eta_part.split_whitespace().next().unwrap_or("--").to_string();
        if let Some(p) = e.find('(') { e = e[..p].trim().to_string(); }
        eta = e;
    }
    (percent, speed, eta, size)
}

/// Side-car files (cover art, subtitles, metadata) also produce `Destination:`
/// lines. They must not be mistaken for the file the user asked for.
fn is_sidecar(path: &str) -> bool {
    const SKIP: [&str; 12] = [
        ".jpg", ".jpeg", ".png", ".webp", ".gif",
        ".vtt", ".srt", ".ass", ".lrc", ".ttml",
        ".info.json", ".description",
    ];
    let lower = path.to_ascii_lowercase();
    SKIP.iter().any(|e| lower.ends_with(e))
}

fn quoted(line: &str, marker: &str) -> Option<String> {
    let rest = line.split(marker).nth(1)?.trim();
    let inner = rest.strip_prefix('"')?;
    let end = inner.rfind('"')?;
    Some(inner[..end].to_string())
}

/// Extracts the path yt-dlp is writing, from whichever stage printed the line.
/// Later stages (merge, audio extraction, move) overwrite earlier ones, which
/// matches yt-dlp's output order — so "last match wins" yields the final file.
fn parse_destination(line: &str) -> Option<String> {
    let candidate = if line.contains("Merging formats into ") {
        quoted(line, "Merging formats into ")?
    } else if line.contains("[MoveFiles] Moving file ") {
        // `Moving file "src" to "dst"` — we want the destination.
        let rest = line.split("\" to \"").nth(1)?;
        rest.trim_end().trim_end_matches('"').to_string()
    } else if let Some(rest) = line.split("Destination: ").nth(1) {
        rest.trim().to_string()
    } else if line.contains(" has already been downloaded") {
        let rest = line.split("[download] ").nth(1)?;
        rest.split(" has already been downloaded").next()?.trim().to_string()
    } else {
        return None;
    };

    if candidate.is_empty() || is_sidecar(&candidate) {
        None
    } else {
        Some(candidate)
    }
}

/// Picks the most useful line out of yt-dlp's stderr for display.
fn best_error(stderr: &[String]) -> Option<String> {
    stderr
        .iter()
        .find(|l| l.contains("ERROR:"))
        .or_else(|| stderr.iter().find(|l| l.to_lowercase().contains("error")))
        .or_else(|| stderr.iter().rev().find(|l| !l.trim().is_empty()))
        .map(|l| l.trim().to_string())
}

fn unix_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

/// `--rate-limit` rejects anything that isn't a number with an optional unit,
/// and a rejected value aborts the whole download with an opaque usage error.
fn valid_rate_limit(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() { return false; }
    let (num, unit) = match s.find(|c: char| c.is_ascii_alphabetic()) {
        Some(i) => (&s[..i], &s[i..]),
        None => (s, ""),
    };
    if num.is_empty() || num.parse::<f64>().is_err() { return false; }
    matches!(unit.to_ascii_uppercase().as_str(), "" | "K" | "M" | "G" | "KB" | "MB" | "GB")
}

fn insert_history(
    db: &Mutex<rusqlite::Connection>,
    url: &str,
    title: &str,
    thumbnail: &Option<String>,
    format_id: &str,
    audio_only: bool,
    output_path: Option<&str>,
    status: &str,
) {
    let db = lock(db);
    let _ = db.execute(
        "INSERT INTO history (url, title, thumbnail, format_id, audio_only, output_path, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![url, title, thumbnail, format_id, audio_only as i32, output_path, status, unix_now()],
    );
}

// ─── Core Commands ────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn check_ytdlp() -> Result<String, String> {
    let output = tokio::task::spawn_blocking(|| ytdlp_cmd().arg("--version").output())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|_| "yt-dlp not found.\nInstall: brew install yt-dlp  or  pip install yt-dlp".to_string())?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err("yt-dlp returned non-zero exit code.".to_string())
    }
}

/// ffmpeg is required for every merged (video+audio) download and for MP3
/// extraction. Without it those downloads fail deep inside yt-dlp with an error
/// that doesn't obviously point at a missing dependency.
#[tauri::command]
pub async fn check_ffmpeg() -> Result<bool, String> {
    let output = tokio::task::spawn_blocking(|| tool_cmd("ffmpeg").arg("-version").output())
        .await
        .map_err(|e| e.to_string())?;
    Ok(matches!(output, Ok(o) if o.status.success()))
}

#[tauri::command]
pub async fn fetch_media_info(url: String, cookies_browser: String) -> Result<MediaInfo, String> {
    let url_c = url.clone();
    let cookies = cookies_browser.trim().to_string();

    let output = tokio::task::spawn_blocking(move || {
        let mut cmd = ytdlp_cmd();
        cmd.args([
            "-j",
            "--no-warnings",
            "--no-playlist",
            // Without these a dead host leaves the UI stuck on "Fetching…"
            // indefinitely, with no way to abort.
            "--socket-timeout", "15",
            "--retries", "2",
        ]);
        // Age-restricted and members-only pages fail at the metadata step too,
        // not just at download time, so the cookie setting has to apply here.
        if !cookies.is_empty() {
            cmd.args(["--cookies-from-browser", &cookies]);
        }
        cmd.arg(&url_c).output()
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("Failed to run yt-dlp: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let lines: Vec<String> = stderr.lines().map(String::from).collect();
        return Err(best_error(&lines).unwrap_or_else(|| "yt-dlp returned an error".to_string()));
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let first_line = raw.lines().find(|l| l.trim_start().starts_with('{')).unwrap_or("{}");
    let v: serde_json::Value = serde_json::from_str(first_line)
        .map_err(|e| format!("JSON parse error: {}", e))?;

    let formats: Vec<YtFormat> = v["formats"].as_array().map(|arr| {
        arr.iter().filter_map(|f| {
            let format_id   = f["format_id"].as_str()?.to_string();
            let ext         = f["ext"].as_str().unwrap_or("?").to_string();
            let vcodec      = f["vcodec"].as_str().unwrap_or("none").to_string();
            let acodec      = f["acodec"].as_str().unwrap_or("none").to_string();
            let format_note = f["format_note"].as_str().unwrap_or("").to_string();
            let tbr         = f["tbr"].as_f64();
            let resolution  = if vcodec != "none" {
                match (f["width"].as_u64(), f["height"].as_u64()) {
                    (Some(w), Some(h)) => format!("{}×{}", w, h),
                    _ => "video".to_string(),
                }
            } else if acodec != "none" {
                f["abr"].as_f64().map(|a| format!("{:.0}kbps", a)).unwrap_or_else(|| "audio".to_string())
            } else { "n/a".to_string() };
            let filesize = f["filesize"].as_u64().or_else(|| f["filesize_approx"].as_u64());
            Some(YtFormat { format_id, ext, resolution, filesize, vcodec, acodec, format_note, tbr })
        }).collect()
    }).unwrap_or_default();

    Ok(MediaInfo {
        id:          v["id"].as_str().unwrap_or("").to_string(),
        title:       v["title"].as_str().unwrap_or("Untitled").to_string(),
        thumbnail:   v["thumbnail"].as_str().map(String::from),
        duration:    v["duration"].as_f64(),
        uploader:    v["uploader"].as_str().map(String::from),
        formats,
        webpage_url: v["webpage_url"].as_str().unwrap_or(&url).to_string(),
    })
}

#[tauri::command]
pub async fn start_download(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    url: String,
    title: String,
    thumbnail: Option<String>,
    format_id: String,
    output_dir: String,
    audio_only: bool,
    // True when the chosen custom format carries no audio track, so yt-dlp has
    // to pull a separate audio stream and merge — otherwise the resulting file
    // would be silent.
    merge_audio: bool,
    // settings flags
    embed_thumbnail: bool,
    embed_subtitles: bool,
    speed_limit: String,
    cookies_browser: String,
) -> Result<(), String> {
    if output_dir.trim().is_empty() {
        return Err("No output folder selected.".to_string());
    }

    let mut cmd = ytdlp_cmd();
    cmd.arg("--progress").arg("--newline").arg("--no-playlist");

    // ── Format ───────────────────────────────────────────────────────────────
    if audio_only {
        cmd.args(["-x", "--audio-format", "mp3", "--audio-quality", "0"]);
        if embed_thumbnail {
            cmd.arg("--embed-thumbnail");
        }
    } else {
        match format_id.as_str() {
            "best"  => { cmd.args(["-f", "bestvideo+bestaudio/best", "--merge-output-format", "mp4"]); }
            "1080p" => { cmd.args(["-f", "bestvideo[height<=1080]+bestaudio/best[height<=1080]", "--merge-output-format", "mp4"]); }
            "720p"  => { cmd.args(["-f", "bestvideo[height<=720]+bestaudio/best[height<=720]",   "--merge-output-format", "mp4"]); }
            "480p"  => { cmd.args(["-f", "bestvideo[height<=480]+bestaudio/best[height<=480]",   "--merge-output-format", "mp4"]); }
            other   => {
                if merge_audio {
                    cmd.args(["-f", &format!("{}+bestaudio/{}", other, other)]);
                    cmd.args(["--merge-output-format", "mp4"]);
                } else {
                    cmd.args(["-f", other]);
                }
            }
        }
        if embed_thumbnail { cmd.arg("--embed-thumbnail"); }
        if embed_subtitles { cmd.args(["--write-auto-subs", "--embed-subs", "--sub-langs", "en"]); }
    }

    // ── Speed limit ──────────────────────────────────────────────────────────
    let speed_limit = speed_limit.trim();
    if !speed_limit.is_empty() {
        if !valid_rate_limit(speed_limit) {
            return Err(format!(
                "Invalid speed limit \"{}\". Use a number with an optional K/M/G suffix, e.g. 500K or 2M.",
                speed_limit
            ));
        }
        cmd.args(["--rate-limit", speed_limit]);
    }

    // ── Cookies ──────────────────────────────────────────────────────────────
    if !cookies_browser.trim().is_empty() {
        cmd.args(["--cookies-from-browser", cookies_browser.trim()]);
    }

    // `join` keeps the separator correct on Windows and collapses a trailing
    // slash the user may have picked up from the folder dialog.
    let out_template = Path::new(output_dir.trim()).join("%(title)s.%(ext)s");
    cmd.arg("-o")
        .arg(&out_template)
        .arg(&url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| format!("Failed to start yt-dlp: {}", e))?;
    let stdout = child.stdout.take().ok_or("Could not capture stdout")?;
    let stderr = child.stderr.take().ok_or("Could not capture stderr")?;

    lock(&state.downloads).insert(id.clone(), child);

    // ── stderr collector ─────────────────────────────────────────────────────
    // Drained on its own thread so a chatty stderr can never fill the pipe
    // buffer and deadlock yt-dlp. The lines are handed back to the reaper below
    // so that exactly one thread decides the outcome.
    let stderr_thread = std::thread::spawn(move || {
        BufReader::new(stderr).lines().map_while(Result::ok).collect::<Vec<String>>()
    });

    // ── stdout / progress / finalisation thread ──────────────────────────────
    let app_p   = app.clone();
    let id_p    = id.clone();
    let url_p   = url.clone();
    let title_p = title.clone();
    let thumb_p = thumbnail.clone();
    let fmt_p   = format_id.clone();
    let dl_arc  = Arc::clone(&state.downloads);
    let cancel_arc = Arc::clone(&state.cancelled);
    let db_arc  = Arc::clone(&state.db);

    std::thread::spawn(move || {
        let mut final_path: Option<String> = None;
        let mut stage = if audio_only { "audio" } else { "video" }.to_string();
        let mut stream_idx = 0usize;

        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if line.contains("Merging formats into ") {
                stage = "merging".to_string();
            } else if line.contains("[ExtractAudio]") {
                stage = "processing".to_string();
            }

            if let Some(dest) = parse_destination(&line) {
                // yt-dlp writes the selected streams in selector order: video
                // first, then audio. Counting them is more reliable than
                // sniffing the container, since both tracks can be .webm.
                if line.contains("Destination: ") && !line.contains("[ExtractAudio]") {
                    stream_idx += 1;
                    if !audio_only {
                        stage = if stream_idx == 1 { "video" } else { "audio" }.to_string();
                    }
                }
                final_path = Some(dest);
            }
            if line.contains("[download]") && line.contains('%') {
                let (pct, speed, eta, size) = parse_progress(&line);
                let _ = app_p.emit("download:progress", ProgressPayload {
                    id: id_p.clone(), percent: pct, speed, eta, size, stage: stage.clone(),
                });
            }
        }

        // Take the child out of the map *before* waiting: holding the lock
        // across `wait()` would block every other download's cancel button for
        // as long as this process runs.
        let child = lock(&dl_arc).remove(&id_p);
        let was_cancelled = lock(&cancel_arc).remove(&id_p);

        let exit_ok = match child {
            Some(mut c) => c.wait().map(|s| s.success()).unwrap_or(false),
            // Already reaped by `cancel_download`.
            None => false,
        };

        let stderr_lines = stderr_thread.join().unwrap_or_default();

        if was_cancelled {
            // The frontend already moved the item to "cancelled"; a kill is not
            // a failure and must not land in history.
            return;
        }

        if exit_ok {
            let path = final_path.unwrap_or_else(|| output_dir.clone());
            let size = std::fs::metadata(&path).ok().filter(|m| m.is_file()).map(|m| m.len());
            let _ = app_p.emit("download:complete", CompletePayload {
                id: id_p.clone(), path: path.clone(), size,
            });
            insert_history(&db_arc, &url_p, &title_p, &thumb_p, &fmt_p, audio_only, Some(&path), "completed");
        } else {
            // Previously an item whose yt-dlp exited non-zero without printing a
            // line containing "ERROR:" stayed stuck on "downloading" forever.
            let message = best_error(&stderr_lines)
                .unwrap_or_else(|| "yt-dlp exited with an error.".to_string());
            let _ = app_p.emit("download:error", ErrorPayload {
                id: id_p.clone(), message,
            });
            insert_history(&db_arc, &url_p, &title_p, &thumb_p, &fmt_p, audio_only, None, "failed");
        }
    });

    Ok(())
}

/// Stops yt-dlp, giving it a chance to exit cleanly first.
///
/// A bare SIGKILL orphans whatever yt-dlp had spawned — most importantly the
/// ffmpeg process doing a merge, which would then keep running unsupervised and
/// writing to a file nothing is tracking any more. SIGTERM lets yt-dlp tear its
/// own children down and leave a resumable `.part` behind.
fn terminate(mut child: Child) {
    #[cfg(unix)]
    {
        unsafe { libc::kill(child.id() as i32, libc::SIGTERM); }
        for _ in 0..20 {
            if matches!(child.try_wait(), Ok(Some(_))) { return; }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    let _ = child.kill();
    let _ = child.wait(); // reap, so the child doesn't linger as a zombie
}

#[tauri::command]
pub fn cancel_download(state: State<'_, AppState>, id: String) -> Result<(), String> {
    // Mark first: the reaper thread may wake up the instant the child dies.
    lock(&state.cancelled).insert(id.clone());
    if let Some(child) = lock(&state.downloads).remove(&id) {
        // Off-thread: the grace period must not block the UI.
        std::thread::spawn(move || terminate(child));
    }
    Ok(())
}

/// Reveals `path` in the platform file manager, selecting the file itself where
/// the OS supports it.
#[tauri::command]
pub async fn open_path(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    let parent = p.parent().map(Path::to_path_buf);

    #[cfg(target_os = "macos")]
    {
        tool_cmd("open").arg("-R").arg(p).spawn().map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        // Plain `explorer <file>` *opens* the file in the default player.
        // `/select,` is what actually reveals it in a folder window.
        if p.is_file() {
            tool_cmd("explorer").arg(format!("/select,{}", p.display()))
                .spawn().map_err(|e| e.to_string())?;
        } else {
            tool_cmd("explorer").arg(p).spawn().map_err(|e| e.to_string())?;
        }
    }
    #[cfg(target_os = "linux")]
    {
        // xdg-open on a file launches the media player, so open the containing
        // folder instead. Try the freedesktop "show item" API first, since it
        // highlights the file the way Finder/Explorer do.
        let shown = std::process::Command::new("dbus-send")
            .args([
                "--session",
                "--dest=org.freedesktop.FileManager1",
                "--type=method_call",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
                &format!("array:string:file://{}", p.display()),
                "string:",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if !shown {
            let target = if p.is_dir() { p.to_path_buf() } else { parent.clone().unwrap_or_else(|| p.to_path_buf()) };
            tool_cmd("xdg-open").arg(target).spawn().map_err(|e| e.to_string())?;
        }
    }

    let _ = parent; // only read on some platforms
    Ok(())
}

// ─── History Commands ─────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_history(state: State<'_, AppState>) -> Result<Vec<HistoryItem>, String> {
    let db = lock(&state.db);
    let mut stmt = db.prepare(
        "SELECT id, url, title, thumbnail, format_id, audio_only, output_path, status, created_at
         FROM history ORDER BY id DESC LIMIT 200"
    ).map_err(|e| e.to_string())?;

    let items = stmt.query_map([], |row| {
        Ok(HistoryItem {
            id:          row.get(0)?,
            url:         row.get(1)?,
            title:       row.get(2)?,
            thumbnail:   row.get(3)?,
            format_id:   row.get(4)?,
            audio_only:  row.get::<_, i32>(5)? != 0,
            output_path: row.get(6)?,
            status:      row.get(7)?,
            created_at:  row.get(8)?,
        })
    }).map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();

    Ok(items)
}

#[tauri::command]
pub fn delete_history_item(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let db = lock(&state.db);
    db.execute("DELETE FROM history WHERE id = ?1", rusqlite::params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn clear_history(state: State<'_, AppState>) -> Result<(), String> {
    let db = lock(&state.db);
    db.execute("DELETE FROM history", []).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn purge_old_history(state: State<'_, AppState>, days: u32) -> Result<(), String> {
    if days == 0 { return Ok(()); }
    use std::time::{SystemTime, UNIX_EPOCH};
    let cutoff = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .saturating_sub(days as u64 * 86400)
        .to_string();
    let db = lock(&state.db);
    db.execute(
        "DELETE FROM history WHERE CAST(created_at AS INTEGER) < ?1",
        rusqlite::params![cutoff],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

// ─── Settings Commands ────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    let db = lock(&state.db);
    let result: rusqlite::Result<String> = db.query_row(
        "SELECT value FROM settings WHERE key = 'settings'",
        [],
        |row| row.get(0),
    );
    match result {
        // A settings row written by an older build can be missing fields the
        // current `Settings` expects; fall back rather than leaving the app
        // permanently unable to read its own settings.
        Ok(json) => Ok(serde_json::from_str(&json).unwrap_or_default()),
        Err(_)   => Ok(Settings::default()),
    }
}

#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, settings: Settings) -> Result<(), String> {
    if !settings.speed_limit.trim().is_empty() && !valid_rate_limit(&settings.speed_limit) {
        return Err(format!(
            "Invalid speed limit \"{}\". Use a number with an optional K/M/G suffix, e.g. 500K or 2M.",
            settings.speed_limit.trim()
        ));
    }
    let json = serde_json::to_string(&settings).map_err(|e| e.to_string())?;
    let db = lock(&state.db);
    db.execute(
        "INSERT INTO settings (key, value) VALUES ('settings', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![json],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_plain_progress_line() {
        let (p, speed, eta, size) =
            parse_progress("[download]   4.2% of   10.00MiB at    1.05MiB/s ETA 00:09");
        assert_eq!(p, 4.2);
        assert_eq!(speed, "1.05MiB/s");
        assert_eq!(eta, "00:09");
        assert_eq!(size, "10.00MiB");
    }

    #[test]
    fn estimated_size_tilde_is_not_returned_as_the_size() {
        let (_, _, _, size) =
            parse_progress("[download]  50.0% of ~  10.00MiB at Unknown B/s ETA Unknown (frag 3/10)");
        assert_eq!(size, "10.00MiB");
    }

    #[test]
    fn merge_line_wins_over_the_fragment_destinations() {
        assert_eq!(
            parse_destination("[download] Destination: /out/Clip.f137.mp4").as_deref(),
            Some("/out/Clip.f137.mp4")
        );
        assert_eq!(
            parse_destination("[Merger] Merging formats into \"/out/Clip.mp4\"").as_deref(),
            Some("/out/Clip.mp4")
        );
    }

    #[test]
    fn cover_art_and_subtitles_are_not_treated_as_the_output_file() {
        assert!(parse_destination("[download] Destination: /out/Clip.webp").is_none());
        assert!(parse_destination("[download] Destination: /out/Clip.en.vtt").is_none());
    }

    #[test]
    fn already_downloaded_still_yields_a_path() {
        assert_eq!(
            parse_destination("[download] /out/Clip.mp4 has already been downloaded").as_deref(),
            Some("/out/Clip.mp4")
        );
    }

    #[test]
    fn rate_limits_are_validated() {
        for ok in ["2M", "500K", "1.5M", "1024", "2GB"] {
            assert!(valid_rate_limit(ok), "{ok} should be accepted");
        }
        for bad in ["", "fast", "2Mbps", "M", "-1x"] {
            assert!(!valid_rate_limit(bad), "{bad} should be rejected");
        }
    }
}
