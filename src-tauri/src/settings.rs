//! Global defaults. A site rule can override some of these per site; see
//! `rules::effective`.

pub const DEFAULT_CONCURRENCY: u32 = 3;
/// Beyond this, sites start rate-limiting and the merges saturate the CPU.
pub const MAX_CONCURRENCY: u32 = 8;

/// `#[serde(default)]` makes the stored JSON forward- and backward-compatible:
/// a blob written by a different build can gain or lose fields without the
/// whole thing failing to parse and silently resetting everything.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub default_save_folder: String,
    /// Preset id ("best", "1080p", …) or an exact yt-dlp format id.
    pub default_format: String,
    /// "best" keeps the source codec; "mp3" transcodes. Defaulting to mp3 —
    /// as this app used to — silently re-encodes a Bandcamp FLAC or an
    /// already-lossy stream, losing quality for no gain.
    pub audio_format: String,
    /// Force `--merge-output-format mp4`. Good for compatibility, wrong to
    /// impose everywhere: plenty of sites serve formats that shouldn't be
    /// remuxed just because YouTube usually should be.
    pub prefer_mp4: bool,
    pub embed_thumbnail: bool,
    pub embed_subtitles: bool,
    /// Comma-separated yt-dlp language codes. Hardcoding "en" was wrong for
    /// most of the non-English web.
    pub subtitle_langs: String,
    pub speed_limit: String,       // "" = no limit, "2M", "500K", …
    pub cookies_browser: String,   // "" = off, "chrome", "firefox", …
    pub auto_open_folder: bool,
    pub auto_delete_history_days: u32, // 0 = never
    pub max_concurrent: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_save_folder: String::new(),
            default_format: "best".to_string(),
            audio_format: "best".to_string(),
            prefer_mp4: false,
            embed_thumbnail: false,
            embed_subtitles: false,
            subtitle_langs: "en".to_string(),
            speed_limit: String::new(),
            cookies_browser: String::new(),
            auto_open_folder: false,
            auto_delete_history_days: 0,
            max_concurrent: DEFAULT_CONCURRENCY,
        }
    }
}

pub fn clamp_concurrency(n: u32) -> u32 {
    n.clamp(1, MAX_CONCURRENCY)
}

/// `--rate-limit` rejects anything that isn't a number with an optional unit,
/// and a rejected value aborts the whole download with an opaque usage error.
pub fn valid_rate_limit(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    let (num, unit) = match s.find(|c: char| c.is_ascii_alphabetic()) {
        Some(i) => (&s[..i], &s[i..]),
        None => (s, ""),
    };
    if num.is_empty() || num.parse::<f64>().is_err() {
        return false;
    }
    matches!(unit.to_ascii_uppercase().as_str(), "" | "K" | "M" | "G" | "KB" | "MB" | "GB")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrency_is_clamped_to_a_usable_range() {
        assert_eq!(clamp_concurrency(0), 1, "zero would stall the queue forever");
        assert_eq!(clamp_concurrency(3), 3);
        assert_eq!(clamp_concurrency(99), MAX_CONCURRENCY);
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

    #[test]
    fn settings_from_a_newer_build_do_not_wipe_the_known_fields() {
        // serde ignores unknown keys, so a blob written by a build with extra
        // options still round-trips everything this build understands.
        let future = r#"{"default_format":"480p","max_concurrent":5,
            "some_option_added_later":true,"another":{"nested":1}}"#;
        let s: Settings = serde_json::from_str(future).expect("unknown keys must be ignored");
        assert_eq!(s.default_format, "480p");
        assert_eq!(s.max_concurrent, 5);
    }

    #[test]
    fn settings_survive_a_round_trip_through_json() {
        let before = Settings {
            subtitle_langs: "en,ja".into(),
            prefer_mp4: true,
            audio_format: "m4a".into(),
            max_concurrent: 4,
            ..Default::default()
        };

        let after: Settings =
            serde_json::from_str(&serde_json::to_string(&before).unwrap()).unwrap();
        assert_eq!(after.subtitle_langs, "en,ja");
        assert!(after.prefer_mp4);
        assert_eq!(after.audio_format, "m4a");
        assert_eq!(after.max_concurrent, 4);
    }

    #[test]
    fn audio_defaults_to_keeping_the_source_codec() {
        assert_eq!(Settings::default().audio_format, "best");
    }

    #[test]
    fn settings_from_an_older_build_keep_their_values_and_gain_defaults() {
        let old = r#"{"default_save_folder":"/tmp","default_format":"720p",
            "embed_thumbnail":true,"embed_subtitles":false,"speed_limit":"",
            "cookies_browser":"","auto_open_folder":true,
            "clear_queue_on_launch":false,"auto_delete_history_days":7}"#;
        let s: Settings = serde_json::from_str(old).expect("old settings must still parse");
        assert_eq!(s.default_format, "720p");
        assert_eq!(s.auto_delete_history_days, 7);
        assert!(s.embed_thumbnail);
        // Fields that did not exist yet:
        assert_eq!(s.max_concurrent, DEFAULT_CONCURRENCY);
        assert_eq!(s.audio_format, "best");
        assert_eq!(s.subtitle_langs, "en");
    }
}
