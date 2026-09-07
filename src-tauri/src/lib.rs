mod commands;
mod diagnose;
mod download;
mod rules;
mod settings;
mod store;
mod tools;
mod ytdlp;

use rusqlite::Connection;
use tauri::{Emitter, Manager};

pub use download::AppState;

/// A `ytgrab://` link the app was asked to open.
#[derive(serde::Serialize, Clone)]
struct DeepLinkPayload {
    url: String,
}

/// Pulls the target out of `ytgrab://add?url=<encoded>` (or `ytgrab://<url>`).
///
/// A deep link is untrusted input — any web page can trigger one — so this only
/// ever *extracts* a URL. It never starts a download: the link populates the
/// bar and the user still presses Grab. Auto-starting here would let a page
/// cause arbitrary network fetches and disk writes with one click.
fn parse_deep_link(raw: &str) -> Option<String> {
    // Schemes are case-insensitive, and a hand-written link or a different OS
    // can hand one back in any case.
    const SCHEME: &str = "ytgrab://";
    let raw = raw.trim();
    if !raw.get(..SCHEME.len())?.eq_ignore_ascii_case(SCHEME) {
        return None;
    }
    let rest = &raw[SCHEME.len()..];
    let after_host = rest.split_once('?');

    let candidate = match after_host {
        Some((_, query)) => query
            .split('&')
            .find_map(|pair| pair.strip_prefix("url="))
            .map(percent_decode)?,
        // `ytgrab://https://example.com/watch` — tolerated for hand-written links.
        None => percent_decode(rest),
    };

    let candidate = candidate.trim().to_string();
    // Only http(s) targets. Without this, a link could hand yt-dlp a `file://`
    // path and turn the app into a local-file reader for whoever sent it.
    if candidate.starts_with("http://") || candidate.starts_with("https://") {
        Some(candidate)
    } else {
        None
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                    }
                    None => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_deep_link::init())
        .setup(|app| {
            let app_data = app.path().app_data_dir().expect("could not resolve app data dir");
            std::fs::create_dir_all(&app_data)?;

            // Resolve yt-dlp before anything can want it: a managed copy wins,
            // then the bundled sidecar, then PATH.
            tools::init(&app_data);

            let conn = Connection::open(app_data.join("history.db"))
                .expect("failed to open SQLite database");
            store::init(&conn).expect("failed to create tables");

            // Seed the scheduler from the persisted setting, so the limit is in
            // force from the first download rather than after the first save.
            let concurrency = store::load_settings(&conn).max_concurrent;
            app.manage(AppState::new(conn, concurrency));

            // Deep links. On Linux and Windows the scheme has to be claimed at
            // runtime for development builds; on macOS the Info.plist entry
            // Tauri generates does it.
            #[cfg(any(target_os = "linux", all(debug_assertions, windows)))]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                let _ = app.deep_link().register_all();
            }
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                let handle = app.handle().clone();
                app.deep_link().on_open_url(move |event| {
                    for raw in event.urls() {
                        if let Some(url) = parse_deep_link(raw.as_str()) {
                            // Bring the window forward so the arriving link is
                            // visibly attributable to what the user just did.
                            if let Some(w) = handle.get_webview_window("main") {
                                let _ = w.set_focus();
                            }
                            let _ = handle.emit("deeplink:add", DeepLinkPayload { url });
                        }
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::tool_status,
            commands::resolve_url,
            commands::site_defaults,
            commands::diagnose_error,
            commands::enqueue_grab,
            commands::cancel_grab,
            commands::queue_status,
            commands::apply_remedy,
            commands::list_site_rules,
            commands::set_site_rule,
            commands::delete_site_rule,
            commands::site_for_url,
            commands::open_path,
            commands::get_history,
            commands::completed_urls,
            commands::delete_history_item,
            commands::clear_history,
            commands::purge_old_history,
            commands::get_settings,
            commands::save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running yt-grab");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_an_encoded_url_from_the_add_form() {
        assert_eq!(
            parse_deep_link("ytgrab://add?url=https%3A%2F%2Fyoutu.be%2Fabc123").as_deref(),
            Some("https://youtu.be/abc123")
        );
    }

    #[test]
    fn tolerates_a_plain_appended_url() {
        assert_eq!(
            parse_deep_link("ytgrab://https://vimeo.com/12345").as_deref(),
            Some("https://vimeo.com/12345")
        );
    }

    #[test]
    fn ignores_other_query_parameters() {
        assert_eq!(
            parse_deep_link("ytgrab://add?src=ext&url=https%3A%2F%2Fx.com%2Fv&v=2").as_deref(),
            Some("https://x.com/v")
        );
    }

    #[test]
    fn refuses_non_http_targets() {
        // A page could otherwise point the app at the local filesystem.
        assert_eq!(parse_deep_link("ytgrab://add?url=file%3A%2F%2F%2Fetc%2Fpasswd"), None);
        assert_eq!(parse_deep_link("ytgrab://add?url=javascript%3Aalert(1)"), None);
        assert_eq!(parse_deep_link("ytgrab://add?url="), None);
    }

    #[test]
    fn the_scheme_is_matched_case_insensitively() {
        assert_eq!(
            parse_deep_link("YTGrab://add?url=https%3A%2F%2Fx.com%2Fv").as_deref(),
            Some("https://x.com/v")
        );
    }

    #[test]
    fn surrounding_whitespace_is_tolerated() {
        assert_eq!(
            parse_deep_link("  ytgrab://add?url=https%3A%2F%2Fx.com%2Fv  ").as_deref(),
            Some("https://x.com/v")
        );
    }

    #[test]
    fn a_short_or_empty_input_does_not_panic() {
        // `raw[..9]` on a shorter string would slice out of bounds.
        for s in ["", "y", "ytgrab:/", "ytgrab://"] {
            assert_eq!(parse_deep_link(s), None, "{s:?}");
        }
    }

    #[test]
    fn a_multibyte_input_does_not_panic_on_the_scheme_check() {
        // Slicing the first 9 bytes must not split a character.
        assert_eq!(parse_deep_link("日本語のテキスト"), None);
    }

    #[test]
    fn refuses_links_for_other_schemes() {
        assert_eq!(parse_deep_link("https://example.com/x"), None);
        assert_eq!(parse_deep_link("othergrab://add?url=https%3A%2F%2Fx.com"), None);
    }

    #[test]
    fn decodes_query_strings_in_the_target() {
        assert_eq!(
            parse_deep_link("ytgrab://add?url=https%3A%2F%2Fwww.youtube.com%2Fwatch%3Fv%3Dabc%26t%3D30").as_deref(),
            Some("https://www.youtube.com/watch?v=abc&t=30")
        );
    }
}
