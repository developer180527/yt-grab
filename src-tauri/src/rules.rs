//! Per-site rules.
//!
//! Different sites genuinely want different treatment — Bandcamp wants original
//! audio in your music folder, YouTube wants 1080p video, Instagram wants your
//! cookies. A single set of global defaults can't express that, so a rule keyed
//! by site overlays the defaults for URLs from that site.
//!
//! Rules are also where a remedy lands: accepting "use Chrome cookies" on a
//! failed Instagram download writes a rule, so the answer is given once.

use crate::settings::Settings;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
pub struct SiteRule {
    /// Registrable domain, e.g. "youtube.com". Lowercase, no leading "www.".
    pub domain: String,
    pub cookies_browser: Option<String>,
    /// Path to an exported cookies.txt. Browser extraction is fragile — Chrome
    /// encrypts its cookie store on desktop and Safari needs disk access — so a
    /// file is the reliable fallback.
    pub cookies_file: Option<String>,
    pub output_dir: Option<String>,
    pub format_id: Option<String>,
    pub audio_only: Option<bool>,
}

/// Everything a download needs, after defaults and the site rule are merged.
/// The UI never assembles this: it says *what* to grab, the backend decides
/// *how*, which is what keeps a future UI redesign from having to know policy.
#[derive(Clone, Debug)]
pub struct EffectiveConfig {
    pub output_dir: String,
    pub format_id: String,
    pub audio_only: bool,
    pub audio_format: String,
    pub cookies_browser: String,
    pub cookies_file: Option<String>,
    pub embed_thumbnail: bool,
    pub embed_subtitles: bool,
    pub subtitle_langs: String,
    pub prefer_mp4: bool,
    pub speed_limit: String,
}

/// Suffixes that are effectively TLDs, so "bbc.co.uk" doesn't collapse to
/// "co.uk". Not exhaustive — the full public suffix list isn't worth a
/// dependency here — but it covers what people actually paste.
const MULTI_PART_SUFFIXES: &[&str] = &[
    "co.uk", "org.uk", "ac.uk", "gov.uk", "me.uk",
    "com.au", "net.au", "org.au", "edu.au",
    "co.jp", "or.jp", "ne.jp", "ac.jp",
    "com.br", "com.mx", "com.ar", "com.tr", "com.cn", "com.tw", "com.hk",
    "co.nz", "co.za", "co.in", "co.kr", "co.il",
];

/// Extracts the host from a URL without pulling in a URL parser.
fn host_of(url: &str) -> Option<String> {
    let rest = match url.find("://") {
        Some(i) => &url[i + 3..],
        // Tolerate a bare "youtube.com/watch?v=x" pasted without a scheme.
        None => url,
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    // Drop any userinfo and port.
    let authority = authority.rsplit('@').next()?;
    let host = authority.split(':').next()?.trim().trim_end_matches('.');
    if host.is_empty() || !host.contains('.') {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

/// The key a rule is stored under: the registrable domain, so `m.youtube.com`
/// and `www.youtube.com` share one rule.
pub fn site_key(url: &str) -> Option<String> {
    let host = host_of(url)?;
    // An IP address has no registrable domain; key on it whole.
    if host.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Some(host);
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() <= 2 {
        return Some(host);
    }
    let last_two = labels[labels.len() - 2..].join(".");
    let take = if MULTI_PART_SUFFIXES.contains(&last_two.as_str()) { 3 } else { 2 };
    if labels.len() < take {
        return Some(host);
    }
    Some(labels[labels.len() - take..].join("."))
}

/// Merges global defaults with the rule for this URL's site.
pub fn effective(settings: &Settings, rule: Option<&SiteRule>, url: &str) -> EffectiveConfig {
    let mut cfg = EffectiveConfig {
        output_dir: settings.default_save_folder.clone(),
        format_id: settings.default_format.clone(),
        audio_only: false,
        audio_format: settings.audio_format.clone(),
        cookies_browser: settings.cookies_browser.clone(),
        cookies_file: None,
        embed_thumbnail: settings.embed_thumbnail,
        embed_subtitles: settings.embed_subtitles,
        subtitle_langs: settings.subtitle_langs.clone(),
        prefer_mp4: settings.prefer_mp4,
        speed_limit: settings.speed_limit.clone(),
    };
    let _ = url;

    if let Some(r) = rule {
        if let Some(v) = &r.output_dir { cfg.output_dir = v.clone(); }
        if let Some(v) = &r.format_id { cfg.format_id = v.clone(); }
        if let Some(v) = r.audio_only { cfg.audio_only = v; }
        if let Some(v) = &r.cookies_browser { cfg.cookies_browser = v.clone(); }
        if let Some(v) = &r.cookies_file { cfg.cookies_file = Some(v.clone()); }
    }
    cfg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subdomains_and_schemes_collapse_to_one_key() {
        for u in [
            "https://www.youtube.com/watch?v=abc",
            "http://m.youtube.com/watch?v=abc",
            "https://music.youtube.com/watch?v=abc",
            "youtube.com/watch?v=abc",
        ] {
            assert_eq!(site_key(u).as_deref(), Some("youtube.com"), "{u}");
        }
    }

    #[test]
    fn short_domains_survive_intact() {
        assert_eq!(site_key("https://youtu.be/abc").as_deref(), Some("youtu.be"));
        assert_eq!(site_key("https://vimeo.com/123").as_deref(), Some("vimeo.com"));
    }

    #[test]
    fn multi_part_suffixes_keep_the_real_name() {
        assert_eq!(site_key("https://www.bbc.co.uk/iplayer/x").as_deref(), Some("bbc.co.uk"));
        assert_eq!(site_key("https://abc.net.au/news").as_deref(), Some("abc.net.au"));
    }

    #[test]
    fn ports_and_credentials_are_stripped() {
        assert_eq!(site_key("https://user:pw@media.example.com:8443/v").as_deref(), Some("example.com"));
    }

    #[test]
    fn things_that_are_not_sites_yield_no_key() {
        assert_eq!(site_key("not a url"), None);
        assert_eq!(site_key("https://localhost:3000/x"), None);
    }

    #[test]
    fn ip_addresses_key_on_the_whole_address() {
        assert_eq!(site_key("http://192.168.1.10:8080/v").as_deref(), Some("192.168.1.10"));
    }

    #[test]
    fn a_rule_overrides_defaults_only_where_it_says_something() {
        let s = Settings {
            default_format: "best".into(),
            default_save_folder: "/downloads".into(),
            embed_thumbnail: true,
            ..Default::default()
        };

        let rule = SiteRule {
            domain: "bandcamp.com".into(),
            audio_only: Some(true),
            output_dir: Some("/music".into()),
            ..Default::default()
        };

        let cfg = effective(&s, Some(&rule), "https://x.bandcamp.com/album/y");
        assert_eq!(cfg.output_dir, "/music", "rule wins");
        assert!(cfg.audio_only);
        assert_eq!(cfg.format_id, "best", "untouched by the rule, so the default stands");
        assert!(cfg.embed_thumbnail, "settings-only fields still apply");
    }

    #[test]
    fn a_rule_can_supply_a_cookies_file_the_globals_have_no_slot_for() {
        let s = Settings::default();
        let rule = SiteRule {
            domain: "instagram.com".into(),
            cookies_file: Some("/tmp/ig.txt".into()),
            ..Default::default()
        };
        let cfg = effective(&s, Some(&rule), "https://instagram.com/reel/x");
        assert_eq!(cfg.cookies_file.as_deref(), Some("/tmp/ig.txt"));
    }

    #[test]
    fn a_site_cookie_choice_overrides_the_global_browser() {
        let s = Settings { cookies_browser: "chrome".into(), ..Default::default() };
        let rule = SiteRule {
            domain: "x.com".into(),
            cookies_browser: Some("firefox".into()),
            ..Default::default()
        };
        let cfg = effective(&s, Some(&rule), "https://x.com/v");
        assert_eq!(cfg.cookies_browser, "firefox");
    }

    #[test]
    fn the_global_browser_still_applies_where_no_rule_names_one() {
        let s = Settings { cookies_browser: "chrome".into(), ..Default::default() };
        let cfg = effective(&s, None, "https://elsewhere.com/v");
        assert_eq!(cfg.cookies_browser, "chrome");
    }

    #[test]
    fn no_rule_means_plain_defaults() {
        let s = Settings::default();
        let cfg = effective(&s, None, "https://example.com/v");
        assert_eq!(cfg.format_id, s.default_format);
        assert!(!cfg.audio_only);
        assert!(cfg.cookies_file.is_none());
    }
}
