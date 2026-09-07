//! The Tauri command surface — the interface the UI calls.
//!
//! These are deliberately thin. Everything they do is delegated to a module, so
//! that a future UI redesign changes only the frontend: the operations a UI
//! needs (resolve, grab, cancel, remedy, rules, history, settings) stay put
//! even when the screens around them change shape.
//!
//! The division of labour is: **the UI says what, the backend decides how.** A
//! grab request names a URL and a format; the backend resolves the folder,
//! cookies, subtitle languages and container from settings plus the site rule.

use crate::diagnose::remedy;
use crate::download::{lock, AppState, DownloadJob};
use crate::rules::{self, SiteRule};
use crate::settings::{clamp_concurrency, valid_rate_limit, Settings};
use crate::store::{self, HistoryItem};
use crate::tools::{self, ToolInfo};
use crate::ytdlp::{self, MediaInfo};
use std::path::Path;
use tauri::{AppHandle, State};

// ─── Environment ──────────────────────────────────────────────────────────────

#[derive(serde::Serialize, Clone)]
pub struct ToolStatus {
    pub ytdlp: ToolInfo,
    pub ffmpeg: ToolInfo,
}

/// What the app has to work with. Replaces the old separate yt-dlp/ffmpeg
/// checks, and adds where yt-dlp came from and how old it is — stale builds are
/// the usual cause of a setup that worked yesterday and doesn't today.
#[tauri::command]
pub async fn tool_status() -> Result<ToolStatus, String> {
    tokio::task::spawn_blocking(|| ToolStatus {
        ytdlp: tools::ytdlp_info(),
        ffmpeg: tools::ffmpeg_info(),
    })
    .await
    .map_err(|e| e.to_string())
}

// ─── Resolving ────────────────────────────────────────────────────────────────

fn effective_for(state: &AppState, url: &str) -> rules::EffectiveConfig {
    let db = lock(&state.db);
    let settings = store::load_settings(&db);
    let rule = rules::site_key(url).and_then(|d| store::get_rule(&db, &d));
    drop(db);
    rules::effective(&settings, rule.as_ref(), url)
}

/// Looks up what is at a URL, applying the site's cookie rule so that
/// members-only and age-gated pages resolve rather than failing at step one.
#[tauri::command]
pub async fn resolve_url(state: State<'_, AppState>, url: String) -> Result<MediaInfo, String> {
    let cfg = effective_for(&state, &url);
    let url_c = url.clone();

    let output = tokio::task::spawn_blocking(move || {
        ytdlp::info_command(&url_c, &cfg, false).output()
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("Failed to run yt-dlp: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let lines: Vec<String> = stderr.lines().map(String::from).collect();
        return Err(ytdlp::best_error(&lines)
            .unwrap_or_else(|| "yt-dlp returned an error".to_string()));
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let line = raw.lines().find(|l| l.trim_start().starts_with('{')).unwrap_or("{}");
    let v: serde_json::Value =
        serde_json::from_str(line).map_err(|e| format!("JSON parse error: {e}"))?;

    Ok(ytdlp::media_from_json(&v, &url))
}

/// The choices a site's rule pre-selects, fetched right after a resolve.
///
/// A rule's format and audio-only are *defaults the UI starts from*, not
/// settings applied behind the user's back: a download uses whatever the panel
/// shows when Grab is pressed. Without this command those two rule fields would
/// have no consumer at all — written by a remedy, then silently ignored.
#[derive(serde::Serialize, Debug, PartialEq)]
pub struct SiteDefaults {
    pub domain: Option<String>,
    pub has_rule: bool,
    pub format_id: String,
    pub audio_only: bool,
    /// Present only when a rule names a folder, so applying these defaults
    /// can't replace one the user just picked by hand.
    pub output_dir: Option<String>,
}

impl SiteDefaults {
    /// The pure half, so it can be tested without a database.
    fn build(settings: &Settings, rule: Option<&SiteRule>, url: &str) -> Self {
        let cfg = rules::effective(settings, rule, url);
        Self {
            domain: rules::site_key(url),
            has_rule: rule.is_some(),
            format_id: cfg.format_id,
            audio_only: cfg.audio_only,
            output_dir: rule.and_then(|r| r.output_dir.clone()),
        }
    }
}

#[tauri::command]
pub fn site_defaults(state: State<'_, AppState>, url: String) -> Result<SiteDefaults, String> {
    let db = lock(&state.db);
    let settings = store::load_settings(&db);
    let rule = rules::site_key(&url).and_then(|d| store::get_rule(&db, &d));
    drop(db);
    Ok(SiteDefaults::build(&settings, rule.as_ref(), &url))
}

/// Classifies a failure the UI already has the text for — used when a resolve
/// fails, so the same remedy buttons appear as on a failed download.
#[tauri::command]
pub fn diagnose_error(state: State<'_, AppState>, url: String, message: String) -> Result<crate::diagnose::Failure, String> {
    let cfg = effective_for(&state, &url);
    let browser = Some(cfg.cookies_browser.as_str()).filter(|b| !b.is_empty());
    Ok(crate::diagnose::diagnose(&message, browser, tools::can_self_update()))
}

// ─── Grabbing ─────────────────────────────────────────────────────────────────

/// What the UI asks for. Everything absent here is policy the backend resolves.
#[derive(serde::Deserialize, Debug)]
pub struct GrabRequest {
    pub id: String,
    pub url: String,
    pub title: String,
    pub thumbnail: Option<String>,
    /// Preset id ("best", "1080p", …) or an exact yt-dlp format id.
    pub format_id: String,
    pub audio_only: bool,
    /// True when the chosen exact format has no audio track, so a separate
    /// audio stream has to be merged in or the file would be silent.
    pub merge_audio: bool,
    /// Overrides the folder for this grab only; `None` uses settings/rules.
    pub output_dir: Option<String>,
}

#[tauri::command]
pub fn enqueue_grab(
    app: AppHandle,
    state: State<'_, AppState>,
    request: GrabRequest,
) -> Result<(), String> {
    let mut cfg = effective_for(&state, &request.url);
    if let Some(dir) = request.output_dir.as_deref().filter(|d| !d.trim().is_empty()) {
        cfg.output_dir = dir.to_string();
    }
    cfg.audio_only = request.audio_only;

    if cfg.output_dir.trim().is_empty() {
        return Err("No output folder selected.".to_string());
    }
    if !cfg.speed_limit.trim().is_empty() && !valid_rate_limit(&cfg.speed_limit) {
        return Err(format!(
            "Invalid speed limit \"{}\". Use a number with an optional K/M/G suffix, e.g. 500K or 2M.",
            cfg.speed_limit.trim()
        ));
    }

    state.enqueue(DownloadJob {
        id: request.id,
        url: request.url,
        title: request.title,
        thumbnail: request.thumbnail,
        // A preset selector already pulls in audio, so merging only ever
        // applies to an exact format id.
        merge_audio: request.merge_audio && !ytdlp::is_preset(&request.format_id),
        format_id: request.format_id,
        cfg,
    })?;
    state.dispatch(&app);
    Ok(())
}

#[tauri::command]
pub fn cancel_grab(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.cancel(&id);
    Ok(())
}

/// Running and waiting counts, so the queue can distinguish a download in
/// flight from one holding for a free slot.
#[tauri::command]
pub fn queue_status(state: State<'_, AppState>) -> Result<(usize, usize), String> {
    Ok(state.counts())
}

// ─── Remedies ─────────────────────────────────────────────────────────────────

/// Records one cookie source on a rule, clearing the other.
///
/// The two are mutually exclusive because `ytdlp` prefers the file: leaving a
/// stale browser setting behind would make the rules list advertise something
/// that never takes effect.
fn set_cookie_source(rule: &mut SiteRule, remedy_id: &str, value: String) {
    if remedy_id == remedy::COOKIES_FROM_BROWSER {
        rule.cookies_browser = Some(value);
        rule.cookies_file = None;
    } else {
        rule.cookies_file = Some(value);
        rule.cookies_browser = None;
    }
}

/// Applies the persistent part of a remedy and reports what changed.
///
/// Remedies that need no stored state (plain retry, retry at best available)
/// are handled entirely in the UI by re-issuing the grab; those are absent here
/// on purpose rather than by omission.
#[tauri::command]
pub fn apply_remedy(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
    remedy_id: String,
    value: Option<String>,
) -> Result<Option<SiteRule>, String> {
    match remedy_id.as_str() {
        remedy::COOKIES_FROM_BROWSER | remedy::IMPORT_COOKIES_FILE => {
            let domain = rules::site_key(&url)
                .ok_or_else(|| "Couldn't work out the site for that link.".to_string())?;
            let value = value.ok_or_else(|| "Nothing chosen.".to_string())?;
            let db = lock(&state.db);
            let rule = store::patch_rule(&db, &domain, |r| {
                set_cookie_source(r, &remedy_id, value)
            })
            .map_err(|e| e.to_string())?;
            Ok(Some(rule))
        }
        remedy::LOWER_CONCURRENCY => {
            let db = lock(&state.db);
            let mut s = store::load_settings(&db);
            s.max_concurrent = clamp_concurrency(s.max_concurrent.saturating_sub(1));
            store::save_settings(&db, &s).map_err(|e| e.to_string())?;
            drop(db);
            state.set_concurrency(s.max_concurrent);
            state.dispatch(&app);
            Ok(None)
        }
        other => Err(format!("\"{other}\" isn't applied through the backend.")),
    }
}

// ─── Site rules ───────────────────────────────────────────────────────────────

#[tauri::command]
pub fn list_site_rules(state: State<'_, AppState>) -> Result<Vec<SiteRule>, String> {
    store::list_rules(&lock(&state.db)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_site_rule(state: State<'_, AppState>, rule: SiteRule) -> Result<(), String> {
    if rule.domain.trim().is_empty() {
        return Err("A rule needs a site.".to_string());
    }
    store::upsert_rule(&lock(&state.db), &rule).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_site_rule(state: State<'_, AppState>, domain: String) -> Result<(), String> {
    store::delete_rule(&lock(&state.db), &domain).map_err(|e| e.to_string())
}

/// The rule key for a URL, so the UI can say which site a change applies to.
#[tauri::command]
pub fn site_for_url(url: String) -> Result<Option<String>, String> {
    Ok(rules::site_key(&url))
}

// ─── Files ────────────────────────────────────────────────────────────────────

/// Reveals `path` in the platform file manager, selecting the file itself where
/// the OS supports it.
#[tauri::command]
pub async fn open_path(path: String) -> Result<(), String> {
    let p = Path::new(&path);

    #[cfg(target_os = "macos")]
    {
        tools::tool_cmd(Path::new("open")).arg("-R").arg(p).spawn().map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        // Plain `explorer <file>` *opens* the file in the default player;
        // `/select,` is what reveals it in a folder window.
        let mut cmd = tools::tool_cmd(Path::new("explorer"));
        if p.is_file() {
            cmd.arg(format!("/select,{}", p.display()));
        } else {
            cmd.arg(p);
        }
        cmd.spawn().map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        // xdg-open on a file launches the media player, so open the containing
        // folder instead. Try the freedesktop "show item" API first, since it
        // highlights the file the way Finder and Explorer do.
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
            let target = if p.is_dir() {
                p.to_path_buf()
            } else {
                p.parent().map(Path::to_path_buf).unwrap_or_else(|| p.to_path_buf())
            };
            tools::tool_cmd(Path::new("xdg-open")).arg(target).spawn().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

// ─── History ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_history(state: State<'_, AppState>) -> Result<Vec<HistoryItem>, String> {
    store::get_history(&lock(&state.db)).map_err(|e| e.to_string())
}

/// URLs already grabbed successfully, so the UI can flag a repeat before it
/// costs bandwidth.
#[tauri::command]
pub fn completed_urls(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    store::completed_urls(&lock(&state.db)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_history_item(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    store::delete_history_item(&lock(&state.db), id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_history(state: State<'_, AppState>) -> Result<(), String> {
    store::clear_history(&lock(&state.db)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn purge_old_history(state: State<'_, AppState>, days: u32) -> Result<(), String> {
    store::purge_old_history(&lock(&state.db), days).map_err(|e| e.to_string())
}

// ─── Settings ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<Settings, String> {
    Ok(store::load_settings(&lock(&state.db)))
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<(), String> {
    if !settings.speed_limit.trim().is_empty() && !valid_rate_limit(&settings.speed_limit) {
        return Err(format!(
            "Invalid speed limit \"{}\". Use a number with an optional K/M/G suffix, e.g. 500K or 2M.",
            settings.speed_limit.trim()
        ));
    }
    store::save_settings(&lock(&state.db), &settings).map_err(|e| e.to_string())?;

    // Apply the new concurrency immediately. Raising it should start waiting
    // jobs now rather than at the next completion; lowering it lets the running
    // ones finish and simply stops new ones starting.
    state.set_concurrency(settings.max_concurrent);
    state.dispatch(&app);
    Ok(())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::SiteRule;

    fn settings() -> Settings {
        Settings {
            default_save_folder: "/downloads".into(),
            default_format: "1080p".into(),
            ..Default::default()
        }
    }

    #[test]
    fn with_no_rule_the_defaults_are_the_global_ones() {
        let d = SiteDefaults::build(&settings(), None, "https://vimeo.com/1");
        assert_eq!(d.domain.as_deref(), Some("vimeo.com"));
        assert!(!d.has_rule);
        assert_eq!(d.format_id, "1080p");
        assert!(!d.audio_only);
        assert_eq!(d.output_dir, None, "no rule folder means the UI keeps its own");
    }

    #[test]
    fn a_rule_supplies_the_choices_the_panel_should_open_with() {
        // This is the only consumer of a rule's format/audio_only; without it
        // those fields would be written by a remedy and never read.
        let rule = SiteRule {
            domain: "bandcamp.com".into(),
            audio_only: Some(true),
            format_id: Some("best".into()),
            output_dir: Some("/music".into()),
            ..Default::default()
        };
        let d = SiteDefaults::build(&settings(), Some(&rule), "https://x.bandcamp.com/album/y");

        assert!(d.has_rule);
        assert_eq!(d.format_id, "best", "rule beats the global default");
        assert!(d.audio_only);
        assert_eq!(d.output_dir.as_deref(), Some("/music"));
        assert_eq!(d.domain.as_deref(), Some("bandcamp.com"));
    }

    #[test]
    fn a_cookies_only_rule_does_not_move_the_users_folder() {
        // The common case: a remedy saved cookies for a site and nothing else.
        // That must not start redirecting where files land.
        let rule = SiteRule {
            domain: "instagram.com".into(),
            cookies_browser: Some("chrome".into()),
            ..Default::default()
        };
        let d = SiteDefaults::build(&settings(), Some(&rule), "https://instagram.com/reel/x");

        assert!(d.has_rule);
        assert_eq!(d.output_dir, None);
        assert_eq!(d.format_id, "1080p", "untouched by the rule");
        assert!(!d.audio_only);
    }

    #[test]
    fn a_link_with_no_recognisable_site_still_yields_usable_defaults() {
        let d = SiteDefaults::build(&settings(), None, "not a url");
        assert_eq!(d.domain, None);
        assert_eq!(d.format_id, "1080p");
    }

    // ── Cookie sources ────────────────────────────────────────────────────

    #[test]
    fn choosing_a_browser_clears_a_previously_imported_file() {
        let mut r = SiteRule {
            domain: "x.com".into(),
            cookies_file: Some("/old/cookies.txt".into()),
            ..Default::default()
        };
        set_cookie_source(&mut r, remedy::COOKIES_FROM_BROWSER, "firefox".into());

        assert_eq!(r.cookies_browser.as_deref(), Some("firefox"));
        assert_eq!(r.cookies_file, None, "the file would otherwise still win");
    }

    #[test]
    fn importing_a_file_clears_a_previously_chosen_browser() {
        let mut r = SiteRule {
            domain: "x.com".into(),
            cookies_browser: Some("chrome".into()),
            ..Default::default()
        };
        set_cookie_source(&mut r, remedy::IMPORT_COOKIES_FILE, "/new/cookies.txt".into());

        assert_eq!(r.cookies_file.as_deref(), Some("/new/cookies.txt"));
        assert_eq!(r.cookies_browser, None);
    }

    #[test]
    fn setting_a_cookie_source_leaves_the_rest_of_the_rule_alone() {
        let mut r = SiteRule {
            domain: "x.com".into(),
            output_dir: Some("/keep".into()),
            audio_only: Some(true),
            ..Default::default()
        };
        set_cookie_source(&mut r, remedy::COOKIES_FROM_BROWSER, "safari".into());

        assert_eq!(r.output_dir.as_deref(), Some("/keep"));
        assert_eq!(r.audio_only, Some(true));
        assert_eq!(r.domain, "x.com");
    }
}
