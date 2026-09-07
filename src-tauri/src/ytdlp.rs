//! Everything that knows yt-dlp's command line and output format.
//!
//! Keeping it here means the scheduler deals in jobs and the commands deal in
//! requests, and neither has to know how a format selector is spelled.

use crate::rules::EffectiveConfig;
use crate::settings::valid_rate_limit;
use crate::tools::ytdlp_cmd;
use std::path::Path;
use std::process::Command;

// ─── Metadata types ───────────────────────────────────────────────────────────

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
    /// Which extractor handled this ("Bandcamp", "Vimeo", …). Shown as a source
    /// badge, and invaluable when someone reports that a site misbehaves.
    pub extractor: String,
    /// True when the item has no video track at all, so the UI can drop
    /// resolution controls that would be meaningless.
    pub audio_only_source: bool,
    /// A live stream has no total size, so a percentage bar can never fill.
    pub is_live: bool,
}

// ─── Format selection ─────────────────────────────────────────────────────────

/// Resolution presets, coarse to fine. Kept in one place so the UI list and the
/// selector can't drift apart.
pub const HEIGHT_PRESETS: &[(&str, u32)] =
    &[("2160p", 2160), ("1440p", 1440), ("1080p", 1080), ("720p", 720), ("480p", 480)];

pub fn is_preset(id: &str) -> bool {
    id == "best" || HEIGHT_PRESETS.iter().any(|(name, _)| *name == id)
}

/// Builds the `-f` argument.
///
/// `merge_audio` matters only for an exact format id: a video-only DASH stream
/// downloaded as-is produces a silent file.
pub fn format_selector(format_id: &str, merge_audio: bool) -> String {
    if format_id == "best" {
        return "bestvideo+bestaudio/best".to_string();
    }
    if let Some((_, h)) = HEIGHT_PRESETS.iter().find(|(name, _)| *name == format_id) {
        return format!("bestvideo[height<={h}]+bestaudio/best[height<={h}]");
    }
    if merge_audio {
        format!("{format_id}+bestaudio/{format_id}")
    } else {
        format_id.to_string()
    }
}

// ─── Command building ─────────────────────────────────────────────────────────

/// Applies the auth part of a config. Shared by metadata lookup and download,
/// because a members-only page fails at *both* steps and the old code only
/// handled one.
fn apply_auth(cmd: &mut Command, cfg: &EffectiveConfig) {
    if let Some(file) = cfg.cookies_file.as_deref().filter(|f| !f.trim().is_empty()) {
        // An exported file beats browser extraction, which is fragile: Chrome
        // encrypts its cookie store on desktop and Safari needs disk access.
        cmd.arg("--cookies").arg(file);
    } else if !cfg.cookies_browser.trim().is_empty() {
        cmd.args(["--cookies-from-browser", cfg.cookies_browser.trim()]);
    }
}

/// The command for a metadata lookup.
pub fn info_command(url: &str, cfg: &EffectiveConfig, allow_playlist: bool) -> Command {
    let mut cmd = ytdlp_cmd();
    cmd.args(["-J", "--no-warnings", "--ignore-config"]);
    if !allow_playlist {
        cmd.arg("--no-playlist");
    }
    // Without these a dead host leaves the UI waiting indefinitely.
    cmd.args(["--socket-timeout", "15", "--retries", "2"]);
    apply_auth(&mut cmd, cfg);
    cmd.arg(url);
    cmd
}

/// The command for an actual download.
pub fn download_command(
    url: &str,
    format_id: &str,
    merge_audio: bool,
    cfg: &EffectiveConfig,
) -> Command {
    let mut cmd = ytdlp_cmd();
    cmd.args(["--progress", "--newline", "--no-playlist", "--ignore-config"]);

    if cfg.audio_only {
        cmd.args(["-x", "--audio-format", &cfg.audio_format]);
        // Quality only applies to a transcode; passing it for "best" would ask
        // yt-dlp to re-encode audio it could otherwise copy untouched.
        if cfg.audio_format != "best" {
            cmd.args(["--audio-quality", "0"]);
        }
        if cfg.embed_thumbnail {
            cmd.arg("--embed-thumbnail");
        }
    } else {
        cmd.args(["-f", &format_selector(format_id, merge_audio)]);
        // Only force a container when asked. Imposing mp4 everywhere breaks
        // sites whose formats shouldn't be remuxed just because YouTube's
        // usually can be.
        if cfg.prefer_mp4 {
            cmd.args(["--merge-output-format", "mp4"]);
        }
        if cfg.embed_thumbnail {
            cmd.arg("--embed-thumbnail");
        }
        if cfg.embed_subtitles {
            let langs = if cfg.subtitle_langs.trim().is_empty() { "en" } else { cfg.subtitle_langs.trim() };
            cmd.args(["--write-auto-subs", "--embed-subs", "--sub-langs", langs]);
        }
    }

    let limit = cfg.speed_limit.trim();
    if !limit.is_empty() && valid_rate_limit(limit) {
        cmd.args(["--rate-limit", limit]);
    }
    apply_auth(&mut cmd, cfg);

    // `join` keeps the separator correct on Windows and collapses a trailing
    // slash picked up from the folder dialog.
    let template = Path::new(cfg.output_dir.trim()).join("%(title)s.%(ext)s");
    cmd.arg("-o").arg(template).arg(url);
    cmd
}

// ─── Metadata parsing ─────────────────────────────────────────────────────────

fn format_from_json(f: &serde_json::Value) -> Option<YtFormat> {
    let format_id = f["format_id"].as_str()?.to_string();
    let ext = f["ext"].as_str().unwrap_or("?").to_string();
    let vcodec = f["vcodec"].as_str().unwrap_or("none").to_string();
    let acodec = f["acodec"].as_str().unwrap_or("none").to_string();
    let format_note = f["format_note"].as_str().unwrap_or("").to_string();
    let tbr = f["tbr"].as_f64();
    let resolution = if vcodec != "none" {
        match (f["width"].as_u64(), f["height"].as_u64()) {
            (Some(w), Some(h)) => format!("{w}×{h}"),
            _ => "video".to_string(),
        }
    } else if acodec != "none" {
        f["abr"].as_f64().map(|a| format!("{a:.0}kbps")).unwrap_or_else(|| "audio".to_string())
    } else {
        "n/a".to_string()
    };
    let filesize = f["filesize"].as_u64().or_else(|| f["filesize_approx"].as_u64());
    Some(YtFormat { format_id, ext, resolution, filesize, vcodec, acodec, format_note, tbr })
}

/// Turns one `-J` entry into a `MediaInfo`.
pub fn media_from_json(v: &serde_json::Value, fallback_url: &str) -> MediaInfo {
    let formats: Vec<YtFormat> = v["formats"]
        .as_array()
        .map(|arr| arr.iter().filter_map(format_from_json).collect())
        .unwrap_or_default();

    // Some extractors (direct audio links, many podcast hosts) report no
    // format list at all; fall back to the top-level codec fields.
    let has_video = if formats.is_empty() {
        v["vcodec"].as_str().map(|c| c != "none").unwrap_or(false)
    } else {
        formats.iter().any(|f| f.vcodec != "none")
    };

    MediaInfo {
        id: v["id"].as_str().unwrap_or("").to_string(),
        title: v["title"].as_str().unwrap_or("Untitled").to_string(),
        thumbnail: v["thumbnail"].as_str().map(String::from),
        duration: v["duration"].as_f64(),
        uploader: v["uploader"].as_str().or(v["channel"].as_str()).map(String::from),
        formats,
        webpage_url: v["webpage_url"].as_str().unwrap_or(fallback_url).to_string(),
        extractor: v["extractor_key"].as_str().or(v["extractor"].as_str()).unwrap_or("").to_string(),
        audio_only_source: !has_video,
        is_live: v["is_live"].as_bool().unwrap_or(false),
    }
}

// ─── Output parsing ───────────────────────────────────────────────────────────

/// Parses one `[download] ... %` progress line.
///
/// Shapes handled:
///   `[download]   4.2% of   10.00MiB at    1.05MiB/s ETA 00:09`
///   `[download]  50.0% of ~ 10.00MiB at Unknown B/s ETA Unknown (frag 3/10)`
pub fn parse_progress(line: &str) -> (f64, String, String, String) {
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
        if let Some(p) = e.find('(') {
            e = e[..p].trim().to_string();
        }
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
pub fn parse_destination(line: &str) -> Option<String> {
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
pub fn best_error(stderr: &[String]) -> Option<String> {
    stderr
        .iter()
        .find(|l| l.contains("ERROR:"))
        .or_else(|| stderr.iter().find(|l| l.to_lowercase().contains("error")))
        .or_else(|| stderr.iter().rev().find(|l| !l.trim().is_empty()))
        .map(|l| l.trim().to_string())
}

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
    fn presets_map_to_capped_selectors_and_exact_ids_pass_through() {
        assert_eq!(format_selector("best", false), "bestvideo+bestaudio/best");
        assert_eq!(
            format_selector("720p", false),
            "bestvideo[height<=720]+bestaudio/best[height<=720]"
        );
        assert_eq!(format_selector("137", false), "137");
    }

    #[test]
    fn a_video_only_format_gets_audio_merged_in() {
        assert_eq!(format_selector("137", true), "137+bestaudio/137");
    }

    #[test]
    fn every_preset_is_recognised_as_one() {
        assert!(is_preset("best"));
        for (name, _) in HEIGHT_PRESETS {
            assert!(is_preset(name), "{name}");
        }
        assert!(!is_preset("137"));
    }

    #[test]
    fn an_audio_only_source_is_detected_so_the_ui_can_hide_resolutions() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"id":"1","title":"Track","extractor_key":"Bandcamp","formats":[
                {"format_id":"mp3-128","ext":"mp3","vcodec":"none","acodec":"mp3","abr":128}]}"#,
        )
        .unwrap();
        let m = media_from_json(&v, "https://x.bandcamp.com/track/y");
        assert!(m.audio_only_source);
        assert_eq!(m.extractor, "Bandcamp");
        assert_eq!(m.formats.len(), 1);
    }

    #[test]
    fn a_video_source_is_not_flagged_audio_only() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"id":"1","title":"Clip","extractor_key":"Vimeo","formats":[
                {"format_id":"h264","ext":"mp4","vcodec":"avc1","acodec":"none","width":1920,"height":1080}]}"#,
        )
        .unwrap();
        let m = media_from_json(&v, "https://vimeo.com/1");
        assert!(!m.audio_only_source);
        assert_eq!(m.formats[0].resolution, "1920×1080");
    }

    #[test]
    fn an_entry_with_no_format_list_falls_back_to_top_level_codecs() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"id":"1","title":"Episode","vcodec":"none","acodec":"mp3"}"#).unwrap();
        assert!(media_from_json(&v, "https://pod.example/ep").audio_only_source);
    }
}
