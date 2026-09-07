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

    let has_video = if !formats.is_empty() {
        formats.iter().any(|f| f.vcodec != "none")
    } else {
        // Some extractors (direct links, many podcast hosts) report no format
        // list. Trust an explicit codec field when there is one; otherwise
        // assume video, because wrongly hiding the quality picker is worse
        // than showing one whose presets fall back to "best" anyway.
        v["vcodec"].as_str().map(|c| c != "none").unwrap_or(true)
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

/// Whether a line is a progress report, as opposed to the other things
/// `[download]` prints.
///
/// Testing for a percent sign anywhere on the line is not enough: a title
/// containing one makes `Destination:` and `has already been downloaded` lines
/// look like progress, and they parse into a bogus 0% with a fragment of the
/// filename for a speed. A real progress line always leads with the
/// percentage, so that is what we require.
pub fn is_progress_line(line: &str) -> bool {
    let Some((_, rest)) = line.split_once("[download]") else {
        return false;
    };
    let Some(first) = rest.split_whitespace().next() else {
        return false;
    };
    first.strip_suffix('%').map_or(false, |n| n.parse::<f64>().is_ok())
}

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

    // ── Command-building helpers ──────────────────────────────────────────

    /// The arguments a `Command` would be launched with, as plain strings.
    fn args_of(cmd: &Command) -> Vec<String> {
        cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect()
    }

    /// The value following `flag`, or None if the flag isn't present.
    fn value_after(args: &[String], flag: &str) -> Option<String> {
        args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned())
    }

    fn has(args: &[String], flag: &str) -> bool {
        args.iter().any(|a| a == flag)
    }

    fn cfg() -> EffectiveConfig {
        EffectiveConfig {
            output_dir: "/out".into(),
            format_id: "best".into(),
            audio_only: false,
            audio_format: "best".into(),
            cookies_browser: String::new(),
            cookies_file: None,
            embed_thumbnail: false,
            embed_subtitles: false,
            subtitle_langs: "en".into(),
            prefer_mp4: false,
            speed_limit: String::new(),
        }
    }

    // ── Audio ─────────────────────────────────────────────────────────────

    #[test]
    fn original_audio_does_not_ask_for_a_transcode() {
        let mut c = cfg();
        c.audio_only = true;
        c.audio_format = "best".into();
        let args = args_of(&download_command("u", "best", false, &c));

        assert!(has(&args, "-x"));
        assert_eq!(value_after(&args, "--audio-format").as_deref(), Some("best"));
        // --audio-quality only applies to a conversion; passing it here would
        // ask yt-dlp to re-encode audio it could otherwise copy untouched.
        assert!(!has(&args, "--audio-quality"), "quality must not be forced for a copy");
    }

    #[test]
    fn converted_audio_asks_for_the_best_quality() {
        let mut c = cfg();
        c.audio_only = true;
        c.audio_format = "mp3".into();
        let args = args_of(&download_command("u", "best", false, &c));

        assert_eq!(value_after(&args, "--audio-format").as_deref(), Some("mp3"));
        assert_eq!(value_after(&args, "--audio-quality").as_deref(), Some("0"));
    }

    #[test]
    fn audio_only_never_selects_a_video_format_or_container() {
        let mut c = cfg();
        c.audio_only = true;
        c.prefer_mp4 = true;
        c.embed_subtitles = true;
        let args = args_of(&download_command("u", "1080p", false, &c));

        assert!(!has(&args, "-f"), "a format selector is meaningless for -x");
        assert!(!has(&args, "--merge-output-format"), "nothing to merge into");
        assert!(!has(&args, "--embed-subs"), "no video track to carry subtitles");
    }

    // ── Container ─────────────────────────────────────────────────────────

    #[test]
    fn mp4_is_not_forced_by_default() {
        // Imposing mp4 everywhere breaks sites whose formats shouldn't be
        // remuxed just because YouTube's usually can be.
        let args = args_of(&download_command("u", "best", false, &cfg()));
        assert!(!has(&args, "--merge-output-format"));
    }

    #[test]
    fn mp4_is_forced_when_asked_for() {
        let mut c = cfg();
        c.prefer_mp4 = true;
        let args = args_of(&download_command("u", "best", false, &c));
        assert_eq!(value_after(&args, "--merge-output-format").as_deref(), Some("mp4"));
    }

    // ── Format selection ──────────────────────────────────────────────────

    #[test]
    fn the_format_selector_reaches_the_command_line() {
        let args = args_of(&download_command("u", "720p", false, &cfg()));
        assert_eq!(
            value_after(&args, "-f").as_deref(),
            Some("bestvideo[height<=720]+bestaudio/best[height<=720]")
        );
    }

    #[test]
    fn a_video_only_exact_format_gets_audio_merged_on_the_command_line() {
        let args = args_of(&download_command("u", "137", true, &cfg()));
        assert_eq!(value_after(&args, "-f").as_deref(), Some("137+bestaudio/137"));
    }

    // ── Auth ──────────────────────────────────────────────────────────────

    #[test]
    fn no_cookie_arguments_when_nothing_is_configured() {
        let args = args_of(&download_command("u", "best", false, &cfg()));
        assert!(!has(&args, "--cookies"));
        assert!(!has(&args, "--cookies-from-browser"));
    }

    #[test]
    fn a_cookies_file_beats_browser_extraction() {
        // Browser extraction is fragile — Chrome encrypts its store on desktop
        // and Safari needs disk access — so an exported file must win.
        let mut c = cfg();
        c.cookies_browser = "chrome".into();
        c.cookies_file = Some("/tmp/cookies.txt".into());
        let args = args_of(&download_command("u", "best", false, &c));

        assert_eq!(value_after(&args, "--cookies").as_deref(), Some("/tmp/cookies.txt"));
        assert!(!has(&args, "--cookies-from-browser"));
    }

    #[test]
    fn the_browser_is_used_when_there_is_no_file() {
        let mut c = cfg();
        c.cookies_browser = "firefox".into();
        let args = args_of(&download_command("u", "best", false, &c));
        assert_eq!(value_after(&args, "--cookies-from-browser").as_deref(), Some("firefox"));
    }

    #[test]
    fn a_blank_cookies_file_falls_back_to_the_browser() {
        let mut c = cfg();
        c.cookies_browser = "brave".into();
        c.cookies_file = Some("   ".into());
        let args = args_of(&download_command("u", "best", false, &c));
        assert!(!has(&args, "--cookies"));
        assert_eq!(value_after(&args, "--cookies-from-browser").as_deref(), Some("brave"));
    }

    // ── Subtitles ─────────────────────────────────────────────────────────

    #[test]
    fn subtitle_languages_are_passed_through() {
        let mut c = cfg();
        c.embed_subtitles = true;
        c.subtitle_langs = "en,es,ja".into();
        let args = args_of(&download_command("u", "best", false, &c));
        assert_eq!(value_after(&args, "--sub-langs").as_deref(), Some("en,es,ja"));
        assert!(has(&args, "--embed-subs"));
    }

    #[test]
    fn a_blank_subtitle_language_does_not_produce_an_empty_argument() {
        let mut c = cfg();
        c.embed_subtitles = true;
        c.subtitle_langs = "  ".into();
        let args = args_of(&download_command("u", "best", false, &c));
        assert_eq!(value_after(&args, "--sub-langs").as_deref(), Some("en"));
    }

    #[test]
    fn subtitles_are_absent_unless_enabled() {
        let args = args_of(&download_command("u", "best", false, &cfg()));
        assert!(!has(&args, "--embed-subs"));
        assert!(!has(&args, "--sub-langs"));
    }

    // ── Rate limit ────────────────────────────────────────────────────────

    #[test]
    fn a_valid_rate_limit_is_passed() {
        let mut c = cfg();
        c.speed_limit = " 500K ".into();
        let args = args_of(&download_command("u", "best", false, &c));
        assert_eq!(value_after(&args, "--rate-limit").as_deref(), Some("500K"));
    }

    #[test]
    fn an_invalid_rate_limit_is_dropped_rather_than_aborting_the_download() {
        // Validation happens at enqueue; if something slips through, a bad
        // value must not reach yt-dlp, where it fails as an opaque usage error.
        let mut c = cfg();
        c.speed_limit = "fast".into();
        let args = args_of(&download_command("u", "best", false, &c));
        assert!(!has(&args, "--rate-limit"));
    }

    // ── Output ────────────────────────────────────────────────────────────

    #[test]
    fn the_output_template_is_joined_onto_the_folder() {
        let args = args_of(&download_command("u", "best", false, &cfg()));
        let out = value_after(&args, "-o").expect("-o must be present");
        assert!(out.ends_with("%(title)s.%(ext)s"), "got {out}");
        assert!(out.starts_with("/out"), "got {out}");
    }

    #[test]
    fn a_trailing_slash_on_the_folder_does_not_double_up() {
        let mut c = cfg();
        c.output_dir = "/out/".into();
        let args = args_of(&download_command("u", "best", false, &c));
        let out = value_after(&args, "-o").unwrap();
        assert!(!out.contains("//"), "got {out}");
    }

    #[test]
    fn the_url_is_the_last_argument() {
        let args = args_of(&download_command("https://example.com/v", "best", false, &cfg()));
        assert_eq!(args.last().map(String::as_str), Some("https://example.com/v"));
    }

    #[test]
    fn machine_readable_progress_is_always_requested() {
        // The whole progress pipeline depends on these two.
        let args = args_of(&download_command("u", "best", false, &cfg()));
        assert!(has(&args, "--newline"));
        assert!(has(&args, "--progress"));
    }

    #[test]
    fn the_users_own_yt_dlp_config_is_ignored() {
        // A stray ~/.config/yt-dlp/config could otherwise silently change the
        // output path or format and make results impossible to explain.
        assert!(has(&args_of(&download_command("u", "best", false, &cfg())), "--ignore-config"));
        assert!(has(&args_of(&info_command("u", &cfg(), false)), "--ignore-config"));
    }

    // ── Metadata lookup ───────────────────────────────────────────────────

    #[test]
    fn metadata_lookup_applies_auth_too() {
        // A members-only page fails at the lookup step, not just at download.
        let mut c = cfg();
        c.cookies_browser = "safari".into();
        let args = args_of(&info_command("u", &c, false));
        assert_eq!(value_after(&args, "--cookies-from-browser").as_deref(), Some("safari"));
    }

    #[test]
    fn metadata_lookup_bounds_how_long_it_can_hang() {
        let args = args_of(&info_command("u", &cfg(), false));
        assert_eq!(value_after(&args, "--socket-timeout").as_deref(), Some("15"));
        assert!(has(&args, "--retries"));
    }

    #[test]
    fn playlist_expansion_is_opt_in() {
        assert!(has(&args_of(&info_command("u", &cfg(), false)), "--no-playlist"));
        assert!(!has(&args_of(&info_command("u", &cfg(), true)), "--no-playlist"));
    }

    // ── Recognising a progress line ───────────────────────────────────────

    #[test]
    fn real_progress_lines_are_recognised() {
        for line in [
            "[download]   4.2% of   10.00MiB at    1.05MiB/s ETA 00:09",
            "[download] 100% of 10.00MiB in 00:05 at 2.00MiB/s",
            "[download]  50.0% of ~  10.00MiB at Unknown B/s ETA Unknown (frag 3/10)",
            "[download]   0.0% of    1.00GiB at  Unknown B/s ETA Unknown",
        ] {
            assert!(is_progress_line(line), "{line}");
        }
    }

    #[test]
    fn a_percent_sign_in_the_title_does_not_fake_a_progress_line() {
        // These are the two lines `[download]` prints that carry a filename, so
        // a title like "50% Off" used to parse as 0% with a fragment of the
        // name for a speed.
        for line in [
            "[download] Destination: /out/50% Off Everything.mp4",
            "[download] /out/100% Real at Home.mp4 has already been downloaded",
            "[Merger] Merging formats into \"/out/50% Off.mp4\"",
        ] {
            assert!(!is_progress_line(line), "{line}");
        }
    }

    #[test]
    fn a_title_with_a_percent_still_yields_its_path() {
        // The destination parser must keep working on the same lines the
        // progress guard now rejects.
        assert_eq!(
            parse_destination("[download] Destination: /out/50% Off Everything.mp4").as_deref(),
            Some("/out/50% Off Everything.mp4")
        );
        assert_eq!(
            parse_destination("[download] /out/100% Real at Home.mp4 has already been downloaded")
                .as_deref(),
            Some("/out/100% Real at Home.mp4")
        );
    }

    #[test]
    fn other_download_chatter_is_not_progress() {
        for line in [
            "[download] Downloading item 1 of 3",
            "[download] Resuming download at byte 1048576",
            "[youtube] abc: Downloading webpage",
            "",
        ] {
            assert!(!is_progress_line(line), "{line}");
        }
    }

    #[test]
    fn a_size_only_line_is_not_treated_as_a_percentage() {
        // yt-dlp drops the percentage when it doesn't know the total. There is
        // nothing to report, and 0% would read as a download that reset.
        assert!(!is_progress_line("[download]  12.34MiB at    1.00MiB/s (00:05)"));
    }

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

    // ── Choosing what to show for a failure ───────────────────────────────
    //
    // `best_error` picks the one line out of yt-dlp's stderr that a failed
    // download is described by: it is what `diagnose` classifies and what the
    // user reads. Everything downstream of a failure depends on this choice,
    // so the fallback chain is pinned here.

    fn lines(raw: &str) -> Vec<String> {
        raw.lines().map(String::from).collect()
    }

    #[test]
    fn the_error_line_is_picked_out_of_surrounding_noise() {
        // A real run buries the error in progress and warning chatter.
        let stderr = lines(
            "[youtube] Extracting URL: https://youtu.be/abc\n\
             WARNING: [youtube] abc: Some formats may be missing\n\
             ERROR: [youtube] abc: Video unavailable\n\
             [debug] Exiting with code 1",
        );
        assert_eq!(best_error(&stderr).as_deref(), Some("ERROR: [youtube] abc: Video unavailable"));
    }

    #[test]
    fn the_first_error_wins_because_later_ones_are_usually_consequences() {
        let stderr = lines(
            "ERROR: unable to download webpage: HTTP Error 429: Too Many Requests\n\
             ERROR: Postprocessing: ffmpeg exited with code 1",
        );
        assert_eq!(
            best_error(&stderr).as_deref(),
            Some("ERROR: unable to download webpage: HTTP Error 429: Too Many Requests"),
            "the root cause is printed first"
        );
    }

    #[test]
    fn a_lowercase_error_is_found_when_nothing_carries_the_prefix() {
        // Not every failure is prefixed; ffmpeg and urllib write their own.
        let stderr = lines("[Merger] Merging formats\nffmpeg: Unknown error occurred\n");
        assert_eq!(best_error(&stderr).as_deref(), Some("ffmpeg: Unknown error occurred"));
    }

    #[test]
    fn the_prefixed_line_beats_an_earlier_unprefixed_mention_of_error() {
        // Otherwise a chatty debug line would stand in for the real failure.
        let stderr = lines(
            "[debug] error handling enabled\n\
             ERROR: [vimeo] 1: The web client only works when logged-in",
        );
        assert_eq!(
            best_error(&stderr).as_deref(),
            Some("ERROR: [vimeo] 1: The web client only works when logged-in")
        );
    }

    #[test]
    fn the_last_useful_line_stands_in_when_nothing_says_error_at_all() {
        // yt-dlp can exit non-zero having said nothing quotable. Reporting the
        // final line is better than reporting nothing, which would leave the
        // row stuck on "downloading" with no explanation.
        let stderr = lines("[generic] Extracting URL: https://example.com/x\nAborted.\n\n   \n");
        assert_eq!(best_error(&stderr).as_deref(), Some("Aborted."));
    }

    #[test]
    fn nothing_at_all_yields_none_so_the_caller_supplies_its_own_sentence() {
        assert_eq!(best_error(&[]), None);
        assert_eq!(best_error(&lines("\n   \n\t\n")), None, "blank lines are not a message");
    }

    #[test]
    fn the_chosen_line_is_trimmed() {
        let stderr = vec!["   ERROR: Unsupported URL: https://x/y   ".to_string()];
        assert_eq!(best_error(&stderr).as_deref(), Some("ERROR: Unsupported URL: https://x/y"));
    }

    #[test]
    fn the_line_it_picks_is_the_one_that_classifies_correctly() {
        // The contract with `diagnose`: whichever line comes back must be the
        // one carrying the signal, or every remedy downstream is wrong. Here
        // the warning would classify as ToolOutdated and the error as NeedsAuth.
        use crate::diagnose::{classify_kind, FailureKind};
        let stderr = lines(
            "WARNING: [youtube] abc: nsig extraction failed\n\
             ERROR: [youtube] abc: Sign in to confirm you're not a bot",
        );
        let picked = best_error(&stderr).expect("a message must be produced");
        assert_eq!(classify_kind(&picked), FailureKind::NeedsAuth);
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
    fn a_live_stream_is_flagged_so_the_ui_can_stop_promising_a_percentage() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"id":"1","title":"Live now","is_live":true,"extractor_key":"Twitch"}"#).unwrap();
        assert!(media_from_json(&v, "https://twitch.tv/x").is_live);
    }

    #[test]
    fn a_sparse_entry_still_produces_something_displayable() {
        // Plenty of extractors return almost nothing; the panel must not show
        // blanks or lose the URL the user pasted.
        let v: serde_json::Value = serde_json::from_str("{}").unwrap();
        let m = media_from_json(&v, "https://example.com/v");
        assert_eq!(m.title, "Untitled");
        assert_eq!(m.webpage_url, "https://example.com/v");
        assert_eq!(m.extractor, "");
        assert!(!m.is_live);
        assert!(m.formats.is_empty());
        // Nothing said it was audio; hiding the quality picker on a guess
        // would be the more damaging mistake.
        assert!(!m.audio_only_source);
    }

    #[test]
    fn the_uploader_falls_back_to_the_channel() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"id":"1","title":"t","channel":"Some Channel"}"#).unwrap();
        assert_eq!(media_from_json(&v, "u").uploader.as_deref(), Some("Some Channel"));
    }

    #[test]
    fn a_format_without_an_id_is_skipped_rather_than_faked() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"id":"1","title":"t","formats":[{"ext":"mp4"},{"format_id":"22","ext":"mp4","vcodec":"avc1","acodec":"mp4a"}]}"#).unwrap();
        let m = media_from_json(&v, "u");
        assert_eq!(m.formats.len(), 1);
        assert_eq!(m.formats[0].format_id, "22");
    }

    #[test]
    fn an_entry_with_no_format_list_falls_back_to_top_level_codecs() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"id":"1","title":"Episode","vcodec":"none","acodec":"mp3"}"#).unwrap();
        assert!(media_from_json(&v, "https://pod.example/ep").audio_only_source);
    }
}
