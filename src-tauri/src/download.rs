//! The download scheduler.
//!
//! Requests are enqueued rather than spawned, and a dispatcher keeps at most
//! `max_concurrent` yt-dlp processes alive. Spawning on request meant a
//! playlist of 40 items would fork 40 downloaders and 40 ffmpeg merges at once
//! — enough to saturate the CPU and get the host to rate-limit us.

use crate::diagnose::{self, Failure};
use crate::rules::EffectiveConfig;
use crate::settings::clamp_concurrency;
use crate::store;
use crate::tools;
use crate::ytdlp;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use tauri::{AppHandle, Emitter};

// ─── Payloads ─────────────────────────────────────────────────────────────────

#[derive(serde::Serialize, Clone)]
pub struct StartedPayload {
    pub id: String,
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
    /// Size of the finished file on disk. The last progress line reports only
    /// the final *stream*, which understates a merged download.
    pub size: Option<u64>,
}

#[derive(serde::Serialize, Clone)]
pub struct FailurePayload {
    pub id: String,
    pub failure: Failure,
}

// ─── Jobs ─────────────────────────────────────────────────────────────────────

/// Everything needed to launch one yt-dlp process, held until a slot frees up.
/// The config is resolved at enqueue time, so the UI never has to assemble it.
#[derive(Clone, Debug)]
pub struct DownloadJob {
    pub id: String,
    pub url: String,
    pub title: String,
    pub thumbnail: Option<String>,
    pub format_id: String,
    pub merge_audio: bool,
    pub cfg: EffectiveConfig,
}

impl DownloadJob {
    /// Identifies the file this job will write. Two jobs for the same URL and
    /// folder resolve to the same output name, so running them together makes
    /// them fight over the same `.part` files and both usually fail.
    pub fn dedupe_key(&self) -> String {
        format!("{}\u{0}{}", self.cfg.output_dir.trim(), self.url)
    }
}

/// A running download: the process plus the output it has claimed.
pub struct RunningDownload {
    pub child: Child,
    pub key: String,
}

/// `Mutex::lock` only fails if another thread panicked while holding the guard.
/// The data behind these mutexes stays consistent in that case, so recover
/// instead of poisoning every later command with a panic.
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// ─── State ────────────────────────────────────────────────────────────────────

/// Every field is an `Arc`, so worker threads take a cheap clone of the whole
/// state rather than half a dozen separate handles.
#[derive(Clone)]
pub struct AppState {
    pub queue: Arc<Mutex<VecDeque<DownloadJob>>>,
    pub downloads: Arc<Mutex<HashMap<String, RunningDownload>>>,
    /// Ids the user explicitly cancelled, so the reaper can tell a kill apart
    /// from a genuine failure.
    pub cancelled: Arc<Mutex<HashSet<String>>>,
    pub max_concurrent: Arc<Mutex<u32>>,
    /// Serialises dispatch and cancellation, so two threads can't both claim
    /// the last free slot, and a job can't be spawned out from under a cancel
    /// that just checked the queue.
    pub dispatch_lock: Arc<Mutex<()>>,
    pub db: Arc<Mutex<rusqlite::Connection>>,
}

impl AppState {
    pub fn new(db: rusqlite::Connection, max_concurrent: u32) -> Self {
        Self {
            queue: Arc::new(Mutex::new(VecDeque::new())),
            downloads: Arc::new(Mutex::new(HashMap::new())),
            cancelled: Arc::new(Mutex::new(HashSet::new())),
            max_concurrent: Arc::new(Mutex::new(clamp_concurrency(max_concurrent))),
            dispatch_lock: Arc::new(Mutex::new(())),
            db: Arc::new(Mutex::new(db)),
        }
    }

    /// Running and waiting counts, for the queue header.
    pub fn counts(&self) -> (usize, usize) {
        (lock(&self.downloads).len(), lock(&self.queue).len())
    }

    pub fn set_concurrency(&self, n: u32) {
        *lock(&self.max_concurrent) = clamp_concurrency(n);
    }

    /// Adds a job unless one is already writing the same file.
    pub fn enqueue(&self, job: DownloadJob) -> Result<(), String> {
        let key = job.dedupe_key();
        let _guard = lock(&self.dispatch_lock);
        let clash = lock(&self.queue).iter().any(|j| j.dedupe_key() == key)
            || lock(&self.downloads).values().any(|r| r.key == key);
        if clash {
            return Err("That URL is already downloading to this folder.".to_string());
        }
        lock(&self.queue).push_back(job);
        Ok(())
    }

    /// Launches queued jobs until the concurrency limit is reached. Safe to
    /// call from anywhere: on enqueue, when a download finishes, and when the
    /// limit is raised in settings.
    pub fn dispatch(&self, app: &AppHandle) {
        let _guard = lock(&self.dispatch_lock);
        loop {
            let limit = *lock(&self.max_concurrent) as usize;
            if lock(&self.downloads).len() >= limit {
                return;
            }
            let job = match lock(&self.queue).pop_front() {
                Some(j) => j,
                None => return,
            };
            // Cancelled while it sat in the queue — drop it without a trace.
            if lock(&self.cancelled).remove(&job.id) {
                continue;
            }
            if let Err(e) = self.spawn_job(app, &job) {
                self.report_failure(app, &job, &e);
            }
        }
    }

    fn report_failure(&self, app: &AppHandle, job: &DownloadJob, raw: &str) {
        let browser = Some(job.cfg.cookies_browser.as_str()).filter(|b| !b.is_empty());
        let failure = diagnose::diagnose(raw, browser, tools::can_self_update());
        let _ = app.emit("download:error", FailurePayload { id: job.id.clone(), failure });
        store::insert_history(
            &lock(&self.db),
            &job.url,
            &job.title,
            &job.thumbnail,
            &job.format_id,
            job.cfg.audio_only,
            None,
            "failed",
        );
    }

    /// Starts one job. Called only from `dispatch`, which owns the slot count.
    fn spawn_job(&self, app: &AppHandle, job: &DownloadJob) -> Result<(), String> {
        let mut cmd =
            ytdlp::download_command(&job.url, &job.format_id, job.merge_audio, &job.cfg);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| format!("Failed to start yt-dlp: {e}"))?;
        let stdout = child.stdout.take().ok_or("Could not capture stdout")?;
        let stderr = child.stderr.take().ok_or("Could not capture stderr")?;

        lock(&self.downloads)
            .insert(job.id.clone(), RunningDownload { child, key: job.dedupe_key() });
        let _ = app.emit("download:started", StartedPayload { id: job.id.clone() });

        // Drained on its own thread so a chatty stderr can never fill the pipe
        // buffer and deadlock yt-dlp. The lines go to the reaper below, so
        // exactly one thread decides the outcome.
        let stderr_thread = std::thread::spawn(move || {
            BufReader::new(stderr).lines().map_while(Result::ok).collect::<Vec<String>>()
        });

        let app_p = app.clone();
        let state = self.clone();
        let job = job.clone();

        std::thread::spawn(move || {
            let audio_only = job.cfg.audio_only;
            let mut final_path: Option<String> = None;
            let mut stage = if audio_only { "audio" } else { "video" }.to_string();
            let mut stream_idx = 0usize;

            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if line.contains("Merging formats into ") {
                    stage = "merging".to_string();
                } else if line.contains("[ExtractAudio]") {
                    stage = "processing".to_string();
                }

                if let Some(dest) = ytdlp::parse_destination(&line) {
                    // yt-dlp writes the selected streams in selector order:
                    // video first, then audio. Counting them is more reliable
                    // than sniffing the container, since both can be .webm.
                    if line.contains("Destination: ") && !line.contains("[ExtractAudio]") {
                        stream_idx += 1;
                        if !audio_only {
                            stage =
                                if stream_idx == 1 { "video" } else { "audio" }.to_string();
                        }
                    }
                    final_path = Some(dest);
                }
                if line.contains("[download]") && line.contains('%') {
                    let (percent, speed, eta, size) = ytdlp::parse_progress(&line);
                    let _ = app_p.emit("download:progress", ProgressPayload {
                        id: job.id.clone(), percent, speed, eta, size, stage: stage.clone(),
                    });
                }
            }

            // Take the child out of the map *before* waiting: holding the lock
            // across `wait()` would block every other download's cancel button
            // for as long as this process runs.
            let running = lock(&state.downloads).remove(&job.id);
            let was_cancelled = lock(&state.cancelled).remove(&job.id);

            let exit_ok = match running {
                Some(mut r) => r.child.wait().map(|s| s.success()).unwrap_or(false),
                // Already reaped by `cancel`.
                None => false,
            };
            let stderr_lines = stderr_thread.join().unwrap_or_default();

            if was_cancelled {
                // The frontend already moved the item to "cancelled"; a kill is
                // not a failure and must not land in history.
            } else if exit_ok {
                let path = final_path.unwrap_or_else(|| job.cfg.output_dir.clone());
                let size =
                    std::fs::metadata(&path).ok().filter(|m| m.is_file()).map(|m| m.len());
                let _ = app_p.emit("download:complete", CompletePayload {
                    id: job.id.clone(), path: path.clone(), size,
                });
                store::insert_history(
                    &lock(&state.db), &job.url, &job.title, &job.thumbnail,
                    &job.format_id, audio_only, Some(&path), "completed",
                );
            } else {
                // An item whose yt-dlp exits non-zero without printing a line
                // containing "ERROR:" must still be reported, or it hangs on
                // "downloading" forever.
                let raw = ytdlp::best_error(&stderr_lines)
                    .unwrap_or_else(|| "yt-dlp exited with an error.".to_string());
                state.report_failure(&app_p, &job, &raw);
            }

            // This slot is free now — let the next queued job take it. Must be
            // last, and only after the child is out of the map.
            state.dispatch(&app_p);
        });

        Ok(())
    }

    /// Stops a download, whether it is running or still waiting.
    pub fn cancel(&self, id: &str) {
        // Held for the whole check, so the job can't be dispatched between
        // "it isn't queued" and "it isn't running".
        let _guard = lock(&self.dispatch_lock);

        {
            let mut queue = lock(&self.queue);
            let before = queue.len();
            queue.retain(|j| j.id != id);
            if queue.len() != before {
                return; // never started; nothing to kill and nothing to report
            }
        }

        // The marker is only set when we actually have a child to kill —
        // setting it for an already-finished download would suppress its
        // completion event.
        if let Some(running) = lock(&self.downloads).remove(id) {
            lock(&self.cancelled).insert(id.to_string());
            std::thread::spawn(move || terminate(running.child));
        }
    }
}

/// Stops yt-dlp, giving it a chance to exit cleanly first.
///
/// A bare SIGKILL orphans whatever yt-dlp spawned — most importantly the ffmpeg
/// doing a merge, which would then keep running unsupervised and writing to a
/// file nothing is tracking. SIGTERM lets yt-dlp tear its own children down and
/// leave a resumable `.part` behind.
fn terminate(mut child: Child) {
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(child.id() as i32, libc::SIGTERM);
        }
        for _ in 0..20 {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    let _ = child.kill();
    let _ = child.wait(); // reap, so the child doesn't linger as a zombie
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{effective, SiteRule};
    use crate::settings::Settings;

    fn job(id: &str, url: &str, dir: &str) -> DownloadJob {
        let mut s = Settings::default();
        s.default_save_folder = dir.to_string();
        DownloadJob {
            id: id.into(),
            url: url.into(),
            title: String::new(),
            thumbnail: None,
            format_id: "best".into(),
            merge_audio: false,
            cfg: effective(&s, None, url),
        }
    }

    fn state() -> AppState {
        let c = rusqlite::Connection::open_in_memory().unwrap();
        store::init(&c).unwrap();
        AppState::new(c, 2)
    }

    #[test]
    fn same_url_and_folder_collide_but_a_different_folder_does_not() {
        assert_eq!(job("1", "https://x/v", "/out").dedupe_key(), job("2", "https://x/v", "/out").dedupe_key());
        assert_ne!(job("1", "https://x/v", "/out").dedupe_key(), job("3", "https://x/v", "/other").dedupe_key());
        assert_ne!(job("1", "https://x/v", "/out").dedupe_key(), job("4", "https://x/w", "/out").dedupe_key());
    }

    #[test]
    fn dedupe_key_cannot_be_forged_by_splicing_folder_and_url() {
        // A separator that appears in neither a path nor a URL keeps
        // "/a" + "b/c" from colliding with "/a/b" + "c".
        assert_ne!(job("1", "b/c", "/a").dedupe_key(), job("2", "c", "/a/b").dedupe_key());
    }

    #[test]
    fn enqueue_rejects_a_second_job_for_the_same_output() {
        let s = state();
        assert!(s.enqueue(job("1", "https://x/v", "/out")).is_ok());
        assert!(s.enqueue(job("2", "https://x/v", "/out")).is_err(), "duplicate must be refused");
        assert!(s.enqueue(job("3", "https://x/w", "/out")).is_ok(), "a different URL is fine");
        assert_eq!(s.counts(), (0, 2));
    }

    #[test]
    fn cancelling_a_queued_job_removes_it_and_frees_the_key() {
        let s = state();
        s.enqueue(job("1", "https://x/v", "/out")).unwrap();
        s.enqueue(job("2", "https://x/w", "/out")).unwrap();

        s.cancel("1");
        assert_eq!(s.counts(), (0, 1));
        // The slot it held is released, so the same URL can be queued again.
        assert!(s.enqueue(job("3", "https://x/v", "/out")).is_ok());
    }

    #[test]
    fn cancelling_a_queued_job_preserves_the_order_of_the_rest() {
        let s = state();
        for (i, u) in ["a", "b", "c"].iter().enumerate() {
            s.enqueue(job(&i.to_string(), &format!("https://x/{u}"), "/out")).unwrap();
        }
        s.cancel("1");
        let ids: Vec<String> = lock(&s.queue).iter().map(|j| j.id.clone()).collect();
        assert_eq!(ids, vec!["0", "2"]);
    }

    #[test]
    fn concurrency_setting_is_clamped_on_the_way_in() {
        let s = state();
        s.set_concurrency(0);
        assert_eq!(*lock(&s.max_concurrent), 1);
        s.set_concurrency(999);
        assert_eq!(*lock(&s.max_concurrent), crate::settings::MAX_CONCURRENCY);
    }

    #[test]
    fn a_site_rule_redirects_the_output_and_therefore_the_dedupe_key() {
        let mut s = Settings::default();
        s.default_save_folder = "/downloads".into();
        let rule = SiteRule {
            domain: "bandcamp.com".into(),
            output_dir: Some("/music".into()),
            ..Default::default()
        };
        let cfg = effective(&s, Some(&rule), "https://x.bandcamp.com/track/y");
        assert_eq!(cfg.output_dir, "/music");
    }
}
