use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};

// ─── App State ────────────────────────────────────────────────────────────────

pub struct AppState {
    pub downloads: Arc<Mutex<HashMap<String, Child>>>,
    pub db: Arc<Mutex<rusqlite::Connection>>,
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
    pub downloaded: String,
}

#[derive(serde::Serialize, Clone)]
pub struct CompletePayload { pub id: String, pub path: String }

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

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn ytdlp_cmd() -> std::process::Command {
    let mut cmd = std::process::Command::new("yt-dlp");
    #[cfg(target_os = "macos")]
    {
        let cur = std::env::var("PATH").unwrap_or_default();
        cmd.env("PATH", format!("/opt/homebrew/bin:/usr/local/bin:{}", cur));
    }
    cmd
}

fn parse_progress(line: &str) -> (f64, String, String, String, String) {
    let mut percent = 0.0_f64;
    let mut speed = "--".to_string();
    let mut eta = "--".to_string();
    let mut size = "--".to_string();
    let mut downloaded = "--".to_string();

    if let Some(pct_str) = line.split('%').next() {
        if let Some(num) = pct_str.split_whitespace().last() {
            percent = num.parse().unwrap_or(0.0);
        }
    }
    if let Some(of_part) = line.split(" of ").nth(1) {
        if let Some(s) = of_part.split_whitespace().next() {
            size = s.trim_start_matches('~').trim().to_string();
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
    if let Some(dl_part) = line.split("[download]").nth(1) {
        if let Some(before_of) = dl_part.split(" of ").next() {
            if let Some(tok) = before_of.split_whitespace().next() {
                downloaded = tok.to_string();
            }
        }
    }
    (percent, speed, eta, size, downloaded)
}

fn unix_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

// ─── Commands ─────────────────────────────────────────────────────────────────

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

#[tauri::command]
pub async fn fetch_media_info(url: String) -> Result<MediaInfo, String> {
    let url_c = url.clone();
    let output = tokio::task::spawn_blocking(move || {
        ytdlp_cmd().args(["-j", "--no-warnings", "--no-playlist", &url_c]).output()
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("Failed to run yt-dlp: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let msg = stderr.lines()
            .find(|l| l.contains("ERROR") || l.to_lowercase().contains("error"))
            .unwrap_or(stderr.trim())
            .to_string();
        return Err(if msg.is_empty() { "yt-dlp returned an error".to_string() } else { msg });
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let first_line = raw.lines().next().unwrap_or("{}");
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
) -> Result<(), String> {
    let mut cmd = ytdlp_cmd();
    cmd.arg("--progress").arg("--newline").arg("--no-playlist");

    if audio_only {
        cmd.args(["-x", "--audio-format", "mp3", "--audio-quality", "0"]);
    } else {
        match format_id.as_str() {
            "best"  => { cmd.args(["-f", "bestvideo+bestaudio/best", "--merge-output-format", "mp4"]); }
            "1080p" => { cmd.args(["-f", "bestvideo[height<=1080]+bestaudio/best[height<=1080]", "--merge-output-format", "mp4"]); }
            "720p"  => { cmd.args(["-f", "bestvideo[height<=720]+bestaudio/best[height<=720]",   "--merge-output-format", "mp4"]); }
            "480p"  => { cmd.args(["-f", "bestvideo[height<=480]+bestaudio/best[height<=480]",   "--merge-output-format", "mp4"]); }
            other   => { cmd.args(["-f", other]); }
        }
    }

    cmd.args(["-o", &format!("{}/%(title)s.%(ext)s", output_dir)])
        .arg(&url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| format!("Failed to start yt-dlp: {}", e))?;
    let stdout = child.stdout.take().ok_or("Could not capture stdout")?;
    let stderr = child.stderr.take().ok_or("Could not capture stderr")?;

    { state.downloads.lock().unwrap().insert(id.clone(), child); }

    // ── stdout / progress / complete thread ───────────────────────────────────
    let app_p   = app.clone();
    let id_p    = id.clone();
    let url_p   = url.clone();
    let title_p = title.clone();
    let thumb_p = thumbnail.clone();
    let fmt_p   = format_id.clone();
    let dir_p   = output_dir.clone();
    let dl_arc  = Arc::clone(&state.downloads);
    let db_arc  = Arc::clone(&state.db);

    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut last_dest = dir_p;

        for line in reader.lines().flatten() {
            if line.contains("[download]") && line.contains('%') {
                let (pct, speed, eta, size, dl) = parse_progress(&line);
                let _ = app_p.emit("download:progress", ProgressPayload {
                    id: id_p.clone(), percent: pct, speed, eta, size, downloaded: dl,
                });
            } else if line.contains("Destination:") {
                if let Some(dest) = line.splitn(2, "Destination:").nth(1) {
                    last_dest = dest.trim().to_string();
                }
            }
        }

        let exit_ok = {
            let mut map = dl_arc.lock().unwrap();
            if let Some(mut c) = map.remove(&id_p) {
                c.wait().map(|s| s.success()).unwrap_or(false)
            } else {
                return; // cancelled
            }
        };

        if exit_ok {
            let _ = app_p.emit("download:complete", CompletePayload {
                id: id_p.clone(),
                path: last_dest.clone(),
            });
            let db = db_arc.lock().unwrap();
            let _ = db.execute(
                "INSERT INTO history (url, title, thumbnail, format_id, audio_only, output_path, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'completed', ?7)",
                rusqlite::params![url_p, title_p, thumb_p, fmt_p, audio_only as i32, last_dest, unix_now()],
            );
        }
    });

    // ── stderr / error thread ─────────────────────────────────────────────────
    let app_e   = app.clone();
    let id_e    = id.clone();
    let url_e   = url.clone();
    let title_e = title.clone();
    let thumb_e = thumbnail.clone();
    let fmt_e   = format_id.clone();
    let db_arc2 = Arc::clone(&state.db);

    std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        let errors: Vec<String> = reader.lines().flatten()
            .filter(|l| l.starts_with("ERROR:") || l.contains("ERROR:"))
            .collect();

        if !errors.is_empty() {
            let _ = app_e.emit("download:error", ErrorPayload {
                id: id_e.clone(),
                message: errors.join("\n"),
            });
            let db = db_arc2.lock().unwrap();
            let _ = db.execute(
                "INSERT INTO history (url, title, thumbnail, format_id, audio_only, output_path, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, 'failed', ?6)",
                rusqlite::params![url_e, title_e, thumb_e, fmt_e, audio_only as i32, unix_now()],
            );
        }
    });

    Ok(())
}

#[tauri::command]
pub fn cancel_download(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut map = state.downloads.lock().unwrap();
    if let Some(mut child) = map.remove(&id) { let _ = child.kill(); }
    Ok(())
}

#[tauri::command]
pub async fn open_path(path: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    { std::process::Command::new("open").arg("-R").arg(&path).spawn().map_err(|e| e.to_string())?; }
    #[cfg(target_os = "windows")]
    { std::process::Command::new("explorer").arg(&path).spawn().map_err(|e| e.to_string())?; }
    #[cfg(target_os = "linux")]
    { std::process::Command::new("xdg-open").arg(&path).spawn().map_err(|e| e.to_string())?; }
    Ok(())
}

// ─── History Commands ─────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_history(state: State<'_, AppState>) -> Result<Vec<HistoryItem>, String> {
    let db = state.db.lock().unwrap();
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
    let db = state.db.lock().unwrap();
    db.execute("DELETE FROM history WHERE id = ?1", rusqlite::params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn clear_history(state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().unwrap();
    db.execute("DELETE FROM history", []).map_err(|e| e.to_string())?;
    Ok(())
}
