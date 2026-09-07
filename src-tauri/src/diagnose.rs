//! Turning yt-dlp's stderr into something the UI can act on.
//!
//! On sites other than YouTube, failing on the first attempt is the normal
//! case, and almost every failure has a specific, known fix. Showing raw stderr
//! makes each one a dead end; classifying it lets the UI offer the fix as a
//! button.
//!
//! Patterns are matched in order, so the specific ones must come before the
//! general ones — "Sign in to confirm you're not a bot" has to win over the
//! bare "unavailable" that often appears in the same message.

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    /// The site wants a signed-in session.
    NeedsAuth,
    /// We're being throttled.
    RateLimited,
    /// yt-dlp is too old for the site's current defences.
    ToolOutdated,
    /// The chosen format doesn't exist for this item.
    FormatUnavailable,
    /// No extractor matched, and the generic one found nothing.
    Unsupported,
    /// Region locked.
    GeoBlocked,
    /// Gone, deleted, or never public.
    Unavailable,
    /// ffmpeg is needed and absent.
    MissingFfmpeg,
    /// Couldn't reach the site.
    Network,
    /// Couldn't write the output.
    Disk,
    Unknown,
}

/// An action the UI can offer as a button. `id` is the stable contract; the
/// label is presentation and may change freely.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Remedy {
    pub id: String,
    pub label: String,
}

impl Remedy {
    fn new(id: &str, label: &str) -> Self {
        Self { id: id.to_string(), label: label.to_string() }
    }
}

/// Remedy ids. The UI dispatches on these, so they are part of the interface.
pub mod remedy {
    pub const COOKIES_FROM_BROWSER: &str = "cookies_from_browser";
    pub const IMPORT_COOKIES_FILE: &str = "import_cookies_file";
    pub const RETRY_BEST_AVAILABLE: &str = "retry_best_available";
    pub const UPDATE_YTDLP: &str = "update_ytdlp";
    pub const LOWER_CONCURRENCY: &str = "lower_concurrency";
    pub const INSTALL_FFMPEG: &str = "install_ffmpeg";
    pub const RETRY: &str = "retry";
    pub const CHOOSE_FOLDER: &str = "choose_folder";
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Failure {
    pub kind: FailureKind,
    /// A plain sentence for the user.
    pub summary: String,
    /// The original yt-dlp line, kept so nothing is hidden.
    pub raw: String,
    pub remedies: Vec<Remedy>,
}

/// Lowercase substrings that identify each kind, most specific first.
const PATTERNS: &[(FailureKind, &[&str])] = &[
    (FailureKind::MissingFfmpeg, &[
        "ffmpeg is not installed",
        "ffmpeg not found",
        "ffprobe and ffmpeg not found",
        "you have requested merging of multiple formats",
    ]),
    (FailureKind::ToolOutdated, &[
        "nsig extraction failed",
        "signature extraction failed",
        "unable to extract yt initial data",
        "please update to the latest version",
        "unable to extract player",
        "player response",
    ]),
    (FailureKind::NeedsAuth, &[
        "sign in to confirm you're not a bot",
        "sign in to confirm your age",
        "use --cookies-from-browser",
        "use --cookies",
        "this video is private",
        "private video",
        "members-only",
        "join this channel",
        "login required",
        "requires authentication",
        "you must be logged in",
        "account is private",
        "requested content is not available",
        "age-restricted",
        "age restricted",
    ]),
    (FailureKind::RateLimited, &[
        "http error 429",
        "too many requests",
        "rate-limit reached",
        "rate limit",
    ]),
    (FailureKind::GeoBlocked, &[
        // The geography is the signal, not the verb: sites say "not available
        // in your country", "unavailable in your country" and "blocked in your
        // region" interchangeably.
        "in your country",
        "in your region",
        "in your location",
        "geo restricted",
        "geo-restricted",
        "geo blocked",
        "geo-blocked",
    ]),
    (FailureKind::FormatUnavailable, &[
        "requested format is not available",
        "requested format not available",
        "no video formats found",
        "no formats found",
    ]),
    (FailureKind::Unsupported, &[
        "unsupported url",
        "no suitable extractor",
        "is not a valid url",
        "unable to find any media",
    ]),
    (FailureKind::Disk, &[
        "no space left",
        "permission denied",
        "unable to rename file",
        "read-only file system",
        "errno 13",
        "errno 28",
    ]),
    (FailureKind::Network, &[
        "unable to download webpage",
        "connection reset",
        "connection refused",
        "timed out",
        "temporary failure in name resolution",
        "getaddrinfo",
        "network is unreachable",
        "ssl:",
    ]),
    (FailureKind::Unavailable, &[
        // Every site phrases this around its own noun — "this tweet is
        // unavailable", "this reel is unavailable", "video is unavailable" —
        // so match the shape rather than enumerating nouns. Safe here because
        // the auth, geo-block and format kinds are all tested before this one.
        "is unavailable",
        "video unavailable",
        "no longer available",
        "this video has been removed",
        "removed by the uploader",
        "content isn't available",
        "this post is not available",
        "http error 404",
        "account has been terminated",
        "has been deleted",
    ]),
];

fn summary_for(kind: FailureKind) -> &'static str {
    match kind {
        FailureKind::NeedsAuth => "This needs you signed in to the site.",
        FailureKind::RateLimited => "The site is rate-limiting us. Waiting a while usually clears it.",
        FailureKind::ToolOutdated => "yt-dlp looks out of date for this site.",
        FailureKind::FormatUnavailable => "That quality isn't offered for this item.",
        FailureKind::Unsupported => "yt-dlp couldn't find any media at this link.",
        FailureKind::GeoBlocked => "This isn't available in your region.",
        FailureKind::Unavailable => "This is private, deleted, or no longer public.",
        FailureKind::MissingFfmpeg => "ffmpeg is needed to combine or convert this, and isn't installed.",
        FailureKind::Network => "Couldn't reach the site.",
        FailureKind::Disk => "Couldn't write the file.",
        FailureKind::Unknown => "The download failed.",
    }
}

/// Remedies offered for a kind. `browser` names the user's likely browser so
/// the cookie button can be specific ("Use Chrome cookies") instead of vague.
fn remedies_for(kind: FailureKind, browser: Option<&str>, self_managed: bool) -> Vec<Remedy> {
    let cookie_label = match browser {
        Some(b) => format!("Use {} cookies", title_case(b)),
        None => "Use browser cookies".to_string(),
    };
    match kind {
        FailureKind::NeedsAuth => vec![
            Remedy::new(remedy::COOKIES_FROM_BROWSER, &cookie_label),
            Remedy::new(remedy::IMPORT_COOKIES_FILE, "Import cookies.txt"),
        ],
        FailureKind::RateLimited => vec![
            Remedy::new(remedy::LOWER_CONCURRENCY, "Slow down"),
            Remedy::new(remedy::RETRY, "Try again"),
        ],
        FailureKind::ToolOutdated => {
            let mut v = vec![];
            if self_managed {
                v.push(Remedy::new(remedy::UPDATE_YTDLP, "Update yt-dlp"));
            }
            v.push(Remedy::new(remedy::RETRY, "Try again"));
            v
        }
        FailureKind::FormatUnavailable => vec![
            Remedy::new(remedy::RETRY_BEST_AVAILABLE, "Use best available"),
        ],
        FailureKind::GeoBlocked | FailureKind::Unavailable | FailureKind::Unsupported => vec![],
        FailureKind::MissingFfmpeg => vec![
            Remedy::new(remedy::INSTALL_FFMPEG, "How to install ffmpeg"),
        ],
        FailureKind::Network => vec![Remedy::new(remedy::RETRY, "Try again")],
        FailureKind::Disk => vec![
            Remedy::new(remedy::CHOOSE_FOLDER, "Pick another folder"),
            Remedy::new(remedy::RETRY, "Try again"),
        ],
        FailureKind::Unknown => vec![Remedy::new(remedy::RETRY, "Try again")],
    }
}

fn title_case(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

pub fn classify_kind(text: &str) -> FailureKind {
    let lower = text.to_ascii_lowercase();
    for (kind, needles) in PATTERNS {
        if needles.iter().any(|n| lower.contains(n)) {
            return *kind;
        }
    }
    FailureKind::Unknown
}

/// Builds the full failure record shown on a failed row.
pub fn diagnose(raw: &str, browser: Option<&str>, self_managed: bool) -> Failure {
    let kind = classify_kind(raw);
    Failure {
        kind,
        summary: summary_for(kind).to_string(),
        raw: tidy(raw),
        remedies: remedies_for(kind, browser, self_managed),
    }
}

/// yt-dlp prefixes lines with "ERROR: " and often appends a bug-report plea
/// that is noise for anyone who isn't filing one.
fn tidy(raw: &str) -> String {
    let line = raw
        .lines()
        .find(|l| l.to_ascii_lowercase().contains("error"))
        .unwrap_or(raw)
        .trim();
    let line = line.strip_prefix("ERROR: ").unwrap_or(line);
    let line = match line.find("; please report this issue") {
        Some(i) => &line[..i],
        None => line,
    };
    line.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind_of(s: &str) -> FailureKind {
        classify_kind(s)
    }

    #[test]
    fn recognises_the_youtube_bot_check() {
        assert_eq!(
            kind_of("ERROR: [youtube] abc: Sign in to confirm you're not a bot. Use --cookies-from-browser"),
            FailureKind::NeedsAuth
        );
    }

    #[test]
    fn auth_wins_over_the_generic_unavailable_in_the_same_message() {
        // Instagram says both; offering "try again" here would be useless.
        assert_eq!(
            kind_of("ERROR: [instagram] Requested content is not available, rate-limit reached"),
            FailureKind::NeedsAuth
        );
    }

    #[test]
    fn recognises_a_stale_binary() {
        assert_eq!(
            kind_of("ERROR: [youtube] xyz: nsig extraction failed: Some formats may be missing"),
            FailureKind::ToolOutdated
        );
    }

    #[test]
    fn recognises_the_common_operational_failures() {
        assert_eq!(kind_of("ERROR: unable to download webpage: HTTP Error 429: Too Many Requests"), FailureKind::RateLimited);
        assert_eq!(kind_of("ERROR: Unsupported URL: https://example.com/page"), FailureKind::Unsupported);
        assert_eq!(kind_of("ERROR: requested format is not available"), FailureKind::FormatUnavailable);
        assert_eq!(kind_of("ERROR: Video unavailable. This video has been removed"), FailureKind::Unavailable);
        assert_eq!(kind_of("ERROR: [Errno 28] No space left on device"), FailureKind::Disk);
    }

    #[test]
    fn ffmpeg_is_detected_before_the_format_complaint_it_arrives_with() {
        // This message contains "requested merging of multiple formats"; the
        // actionable half is that ffmpeg is missing.
        assert_eq!(
            kind_of("ERROR: You have requested merging of multiple formats but ffmpeg is not installed"),
            FailureKind::MissingFfmpeg
        );
    }

    #[test]
    fn auth_failures_offer_both_cookie_routes_and_name_the_browser() {
        let f = diagnose("ERROR: Sign in to confirm you're not a bot", Some("chrome"), true);
        let ids: Vec<&str> = f.remedies.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec![remedy::COOKIES_FROM_BROWSER, remedy::IMPORT_COOKIES_FILE]);
        assert_eq!(f.remedies[0].label, "Use Chrome cookies");
    }

    #[test]
    fn update_is_only_offered_when_the_app_owns_the_binary() {
        let stale = "ERROR: nsig extraction failed";
        let managed = diagnose(stale, None, true);
        assert!(managed.remedies.iter().any(|r| r.id == remedy::UPDATE_YTDLP));

        // A Homebrew-installed yt-dlp must be updated through Homebrew; an
        // in-app button would either fail or fight the package manager.
        let system = diagnose(stale, None, false);
        assert!(!system.remedies.iter().any(|r| r.id == remedy::UPDATE_YTDLP));
    }

    #[test]
    fn dead_ends_offer_nothing_rather_than_a_pointless_retry() {
        assert!(diagnose("ERROR: not available in your country", None, true).remedies.is_empty());
        assert!(diagnose("ERROR: Unsupported URL: https://x/y", None, true).remedies.is_empty());
    }

    /// Messages copied from real yt-dlp runs against non-YouTube sites, which
    /// is where classification earns its keep.
    #[test]
    fn classifies_real_messages_from_other_sites() {
        let cases: &[(&str, FailureKind)] = &[
            ("ERROR: [vimeo] 76979871: The web client only works when logged-in. Use --cookies, --cookies-from-browser, --username and --password",
             FailureKind::NeedsAuth),
            ("ERROR: [soundcloud] 12345: Unable to download JSON metadata: HTTP Error 429: Too Many Requests",
             FailureKind::RateLimited),
            ("ERROR: [generic] page: Unable to download webpage: <urlopen error [Errno 8] nodename nor servname provided>",
             FailureKind::Network),
            ("ERROR: [twitter] 999: This tweet is unavailable",
             FailureKind::Unavailable),
            ("ERROR: [instagram] abc: This reel is unavailable",
             FailureKind::Unavailable),
            ("ERROR: [bandcamp] x: The track is no longer available",
             FailureKind::Unavailable),
            ("ERROR: Unsupported URL: https://example.com/some/article",
             FailureKind::Unsupported),
        ];
        for (msg, want) in cases {
            assert_eq!(kind_of(msg), *want, "{msg}");
        }
    }

    #[test]
    fn the_broad_unavailable_match_does_not_swallow_more_specific_kinds() {
        // "is unavailable" is deliberately loose, so the kinds tested before it
        // must still win when a message contains both.
        assert_eq!(
            kind_of("ERROR: This video is unavailable in your country"),
            FailureKind::GeoBlocked
        );
        assert_eq!(
            kind_of("ERROR: Private video. This video is unavailable. Sign in to confirm"),
            FailureKind::NeedsAuth
        );
        assert_eq!(
            kind_of("ERROR: Requested format is not available"),
            FailureKind::FormatUnavailable
        );
    }

    #[test]
    fn every_kind_produces_a_sentence_rather_than_a_blank() {
        use FailureKind::*;
        for k in [NeedsAuth, RateLimited, ToolOutdated, FormatUnavailable, Unsupported,
                  GeoBlocked, Unavailable, MissingFfmpeg, Network, Disk, Unknown] {
            let s = summary_for(k);
            assert!(!s.trim().is_empty(), "{k:?} has no summary");
            assert!(s.ends_with('.'), "{k:?} summary should read as a sentence");
        }
    }

    #[test]
    fn remedy_ids_are_never_duplicated_within_one_failure() {
        // The UI keys buttons by id; a repeat would collide.
        use FailureKind::*;
        for k in [NeedsAuth, RateLimited, ToolOutdated, FormatUnavailable, Unsupported,
                  GeoBlocked, Unavailable, MissingFfmpeg, Network, Disk, Unknown] {
            let rs = remedies_for(k, Some("chrome"), true);
            let mut ids: Vec<&str> = rs.iter().map(|r| r.id.as_str()).collect();
            let before = ids.len();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), before, "{k:?} repeats a remedy id");
        }
    }

    #[test]
    fn an_unrecognised_message_is_still_actionable() {
        let f = diagnose("ERROR: something nobody has seen before", None, false);
        assert_eq!(f.kind, FailureKind::Unknown);
        assert!(!f.remedies.is_empty(), "unknown failures should still offer a retry");
        assert_eq!(f.raw, "something nobody has seen before");
    }

    #[test]
    fn a_message_with_no_error_line_is_kept_verbatim() {
        let f = diagnose("just some text", None, false);
        assert_eq!(f.raw, "just some text");
    }

    #[test]
    fn raw_text_is_tidied_but_not_swallowed() {
        let f = diagnose(
            "WARNING: something\nERROR: [youtube] abc: Video unavailable; please report this issue on https://github.com/yt-dlp/yt-dlp/issues",
            None, true,
        );
        assert_eq!(f.raw, "[youtube] abc: Video unavailable");
    }
}
