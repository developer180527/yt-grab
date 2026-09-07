//! Locating and describing the external tools the app drives.
//!
//! yt-dlp ships inside the bundle, so a fresh install works with nothing
//! installed on the machine. It is still resolved through a search order rather
//! than hardcoded, because two other copies can legitimately take precedence:
//! a newer one the app downloaded for itself, and whatever the user has on
//! PATH (which is what `tauri dev` and packaged-from-source builds see).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Where the yt-dlp we're actually running came from.
#[derive(serde::Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ToolSource {
    /// Downloaded by the app into its data directory — newer than the bundle.
    Managed,
    /// Shipped inside the app bundle.
    Bundled,
    /// Found on PATH.
    System,
    /// Not found at all.
    Missing,
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct ToolInfo {
    pub available: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub source: ToolSource,
    /// Days since the yt-dlp release date. Stale builds are the single most
    /// common cause of a setup that worked yesterday and doesn't today.
    pub age_days: Option<i64>,
}

impl ToolInfo {
    fn missing() -> Self {
        Self { available: false, version: None, path: None, source: ToolSource::Missing, age_days: None }
    }
}

static YTDLP: OnceLock<(PathBuf, ToolSource)> = OnceLock::new();

fn exe_name(stem: &str) -> String {
    if cfg!(windows) { format!("{stem}.exe") } else { stem.to_string() }
}

/// Directory holding our own executable — where Tauri puts bundled sidecars.
fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(Path::to_path_buf)
}

/// Resolves yt-dlp once, in priority order, and caches the answer.
///
/// `app_data` is the app's data directory; a copy there wins so a future
/// self-update can supersede the bundled binary without having to modify the
/// (code-signed, therefore immutable) app bundle.
pub fn init(app_data: &Path) {
    let name = exe_name("yt-dlp");

    let managed = app_data.join("bin").join(&name);
    if managed.is_file() {
        let _ = YTDLP.set((managed, ToolSource::Managed));
        return;
    }

    if let Some(bundled) = exe_dir().map(|d| d.join(&name)) {
        if bundled.is_file() {
            let _ = YTDLP.set((bundled, ToolSource::Bundled));
            return;
        }
    }

    // Nothing bundled (dev builds, or a distro package that unbundles it).
    let _ = YTDLP.set((PathBuf::from("yt-dlp"), ToolSource::System));
}

fn resolved() -> (PathBuf, ToolSource) {
    YTDLP
        .get()
        .cloned()
        .unwrap_or_else(|| (PathBuf::from("yt-dlp"), ToolSource::System))
}

/// Whether the app can update yt-dlp itself.
///
/// Currently never: the bundled copy lives inside a code-signed app bundle and
/// rewriting it would break the signature, and the `Managed` path that would
/// hold a downloaded replacement has no downloader yet. The seam exists so the
/// "Update yt-dlp" remedy can be switched on in one place once it does — until
/// then, offering a button that cannot work is worse than not offering one.
pub fn can_self_update() -> bool {
    false
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Builds a `Command` for an external tool, patched so it works when the app is
/// launched from the desktop rather than from a shell.
pub fn tool_cmd(program: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);

    // A GUI app inherits a minimal PATH from its launcher (Finder, GNOME, ...),
    // which usually misses Homebrew and pip's --user bin dirs. This matters for
    // ffmpeg, which we don't bundle, and for a PATH-resolved yt-dlp.
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
            extra.push(format!("{home}/.local/bin"));
        }
        extra.push(cur);
        cmd.env("PATH", extra.join(":"));
    }

    // Without this every invocation flashes a console window on Windows.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    cmd
}

pub fn ytdlp_cmd() -> std::process::Command {
    tool_cmd(&resolved().0)
}

pub fn ffmpeg_cmd() -> std::process::Command {
    tool_cmd(Path::new("ffmpeg"))
}

/// yt-dlp versions are release dates (`2026.08.19`), which makes staleness
/// directly measurable rather than something we have to ask a server about.
pub fn age_in_days(version: &str, today: (i32, u32, u32)) -> Option<i64> {
    let mut parts = version.trim().split('.');
    let y: i32 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.trim_end_matches(|c: char| !c.is_ascii_digit()).parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(days_from_civil(today.0, today.1, today.2) - days_from_civil(y, m, d))
}

/// Days since the civil epoch (Howard Hinnant's algorithm). Avoids pulling in a
/// date crate just to subtract two dates.
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = m as i64;
    let d = d as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn today_utc() -> (i32, u32, u32) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    civil_from_days(secs / 86_400)
}

fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    ((if m <= 2 { y + 1 } else { y }) as i32, m as u32, d as u32)
}

fn probe(mut cmd: std::process::Command, arg: &str) -> Option<String> {
    let out = cmd.arg(arg).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().next().map(|l| l.trim().to_string())
}

pub fn ytdlp_info() -> ToolInfo {
    let (path, source) = resolved();
    match probe(tool_cmd(&path), "--version") {
        Some(version) => ToolInfo {
            available: true,
            age_days: age_in_days(&version, today_utc()),
            version: Some(version),
            path: Some(path.display().to_string()),
            source,
        },
        None => ToolInfo::missing(),
    }
}

pub fn ffmpeg_info() -> ToolInfo {
    match probe(ffmpeg_cmd(), "-version") {
        // "ffmpeg version 7.1 Copyright (c) ..." — keep it short for the UI.
        Some(line) => {
            let version = line
                .split_whitespace()
                .nth(2)
                .map(str::to_string)
                .unwrap_or_else(|| line.clone());
            ToolInfo {
                available: true,
                version: Some(version),
                path: None,
                source: ToolSource::System,
                age_days: None,
            }
        }
        None => ToolInfo::missing(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_age_is_measured_from_the_release_date() {
        assert_eq!(age_in_days("2026.08.19", (2026, 8, 19)), Some(0));
        assert_eq!(age_in_days("2026.08.19", (2026, 9, 18)), Some(30));
        assert_eq!(age_in_days("2025.12.31", (2026, 1, 1)), Some(1));
    }

    #[test]
    fn nightly_and_dev_suffixes_still_parse() {
        // Nightlies look like 2026.08.19.232710 and dev builds carry a suffix.
        assert_eq!(age_in_days("2026.08.19.232710", (2026, 8, 20)), Some(1));
    }

    #[test]
    fn unparseable_versions_report_no_age_rather_than_a_wrong_one() {
        assert_eq!(age_in_days("unknown", (2026, 8, 19)), None);
        assert_eq!(age_in_days("2026.13.01", (2026, 8, 19)), None);
        assert_eq!(age_in_days("1.2", (2026, 8, 19)), None);
    }

    #[test]
    fn civil_date_roundtrips() {
        for (y, m, d) in [(2026, 8, 19), (2000, 2, 29), (1970, 1, 1), (2024, 12, 31)] {
            assert_eq!(civil_from_days(days_from_civil(y, m, d)), (y, m, d));
        }
    }
}
