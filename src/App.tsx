import { useState, useEffect, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { downloadDir } from "@tauri-apps/api/path";
import Header from "./components/Header";
import URLBar from "./components/URLBar";
import MediaPanel from "./components/MediaPanel";
import DownloadQueue from "./components/DownloadQueue";
import HistoryPanel from "./components/HistoryPanel";
import SettingsPanel from "./components/SettingsPanel";
import type {
  MediaInfo, DownloadItem, ProgressPayload,
  CompletePayload, ErrorPayload, HistoryItem, Settings,
} from "./types";

type Tab = "download" | "history" | "settings";

const DEFAULT_SETTINGS: Settings = {
  default_save_folder: "",
  default_format: "best",
  embed_thumbnail: false,
  embed_subtitles: false,
  speed_limit: "",
  cookies_browser: "",
  auto_open_folder: false,
  auto_delete_history_days: 0,
};

const PRESET_IDS = new Set(["best", "1080p", "720p", "480p"]);

export default function App() {
  const [ytVersion, setYtVersion] = useState<string | null>(null);
  const [ytMissing, setYtMissing] = useState(false);
  const [ffmpegMissing, setFfmpegMissing] = useState(false);

  const [url, setUrl]           = useState("");
  const [fetching, setFetching] = useState(false);
  const [fetchErr, setFetchErr] = useState<string | null>(null);
  const [media, setMedia]       = useState<MediaInfo | null>(null);

  const [format, setFormat]     = useState("best");
  const [audioOnly, setAudioOnly] = useState(false);
  const [outDir, setOutDir]     = useState("");

  const [downloads, setDownloads] = useState<DownloadItem[]>([]);
  const [history, setHistory]     = useState<HistoryItem[]>([]);
  const [tab, setTab]             = useState<Tab>("download");

  const [settings, setSettings]   = useState<Settings>(DEFAULT_SETTINGS);
  const [settingsSaved, setSettingsSaved] = useState(false);
  const [settingsErr, setSettingsErr]     = useState<string | null>(null);

  // Read inside the Tauri event listeners. Keeping the live value in a ref lets
  // the listeners subscribe exactly once instead of tearing down and re-adding
  // themselves whenever a setting changes — which used to open a window where
  // an event could be delivered to no listener at all, or to two.
  const settingsRef = useRef(settings);
  settingsRef.current = settings;

  const savedTimer = useRef<number | null>(null);

  const loadHistory = useCallback(() => {
    invoke<HistoryItem[]>("get_history").then(setHistory).catch(console.error);
  }, []);

  // ── init ────────────────────────────────────────────────────────────────
  useEffect(() => {
    invoke<string>("check_ytdlp")
      .then((v) => setYtVersion(v))
      .catch(() => setYtMissing(true));

    // Merged downloads and MP3 extraction both shell out to ffmpeg; without it
    // they fail late with an error that doesn't name the missing dependency.
    invoke<boolean>("check_ffmpeg")
      .then((ok) => setFfmpegMissing(!ok))
      .catch(() => setFfmpegMissing(true));

    // Load settings first, then set defaults
    invoke<Settings>("get_settings").then((s) => {
      setSettings(s);
      setFormat(s.default_format || "best");
      if (s.default_save_folder) {
        setOutDir(s.default_save_folder);
      } else {
        downloadDir().then((d) => setOutDir(d)).catch(() => {});
      }
      if (s.auto_delete_history_days > 0) {
        invoke("purge_old_history", { days: s.auto_delete_history_days })
          .then(loadHistory)
          .catch(console.error);
      }
    }).catch(() => {
      downloadDir().then((d) => setOutDir(d)).catch(() => {});
    });

    loadHistory();

    return () => {
      if (savedTimer.current !== null) window.clearTimeout(savedTimer.current);
    };
  }, [loadHistory]);

  // ── Tauri events ────────────────────────────────────────────────────────
  useEffect(() => {
    let disposed = false;
    const unlisteners: Array<() => void> = [];

    const track = (p: Promise<() => void>) => {
      p.then((fn) => {
        // The effect may have been cleaned up before `listen` resolved.
        if (disposed) fn();
        else unlisteners.push(fn);
      }).catch(console.error);
    };

    track(listen<ProgressPayload>("download:progress", ({ payload: p }) => {
      setDownloads((prev) =>
        prev.map((d) =>
          // A cancelled item can still receive one last in-flight progress
          // event; without this guard it would flip back to "downloading".
          d.id === p.id && (d.status === "queued" || d.status === "downloading")
            ? { ...d, status: "downloading", percent: p.percent, speed: p.speed, eta: p.eta, size: p.size, stage: p.stage }
            : d
        )
      );
    }));

    track(listen<CompletePayload>("download:complete", ({ payload: p }) => {
      setDownloads((prev) =>
        prev.map((d) =>
          d.id === p.id
            ? { ...d, status: "completed", percent: 100, output_path: p.path, final_size: p.size }
            : d
        )
      );
      loadHistory();
      if (settingsRef.current.auto_open_folder && p.path) {
        invoke("open_path", { path: p.path }).catch(console.error);
      }
    }));

    track(listen<ErrorPayload>("download:error", ({ payload: p }) => {
      setDownloads((prev) =>
        prev.map((d) =>
          d.id === p.id ? { ...d, status: "failed", error: p.message } : d
        )
      );
      loadHistory();
    }));

    return () => {
      disposed = true;
      unlisteners.forEach((fn) => fn());
    };
  }, [loadHistory]);

  // ── handlers ────────────────────────────────────────────────────────────
  function handleUrlChange(v: string) {
    setUrl(v);
    // A stale error sitting under a URL the user has already replaced reads as
    // if the new URL failed.
    if (fetchErr) setFetchErr(null);
  }

  async function handleFetch() {
    const trimmed = url.trim();
    if (!trimmed || fetching) return;

    if (!/^https?:\/\/\S+$/i.test(trimmed)) {
      setFetchErr("That doesn't look like a URL. Paste a full link starting with http:// or https://");
      return;
    }

    setFetching(true);
    setFetchErr(null);
    setMedia(null);
    try {
      const info = await invoke<MediaInfo>("fetch_media_info", {
        url: trimmed,
        cookiesBrowser: settings.cookies_browser,
      });
      setMedia(info);
      setFormat(settings.default_format || "best");
      setAudioOnly(false);
    } catch (err) {
      setFetchErr(String(err));
    } finally {
      setFetching(false);
    }
  }

  async function handleSelectDir() {
    const selected = await openDialog({ directory: true, multiple: false });
    if (typeof selected === "string") setOutDir(selected);
  }

  async function handleDownload() {
    if (!media || !outDir) return;

    // A custom (non-preset) format id may point at a video-only DASH stream.
    // Downloading it as-is produces a silent file, so tell the backend to pull
    // and merge a separate audio track.
    const chosen = PRESET_IDS.has(format)
      ? null
      : media.formats.find((f) => f.format_id === format);
    const mergeAudio = !audioOnly && !!chosen && chosen.vcodec !== "none" && chosen.acodec === "none";

    const id = crypto.randomUUID();
    const item: DownloadItem = {
      id,
      url: media.webpage_url,
      title: media.title,
      thumbnail: media.thumbnail,
      format_id: format,
      output_dir: outDir,
      audio_only: audioOnly,
      status: "queued",
      percent: 0,
      speed: "--", eta: "--", size: "--",
      stage: audioOnly ? "audio" : "video",
      final_size: null,
      error: null, output_path: null,
    };
    setDownloads((prev) => [item, ...prev]);

    try {
      await invoke("start_download", {
        id,
        url: media.webpage_url,
        title: media.title,
        thumbnail: media.thumbnail,
        formatId: format,
        outputDir: outDir,
        audioOnly,
        mergeAudio,
        embedThumbnail: settings.embed_thumbnail,
        embedSubtitles: settings.embed_subtitles,
        speedLimit: settings.speed_limit,
        cookiesBrowser: settings.cookies_browser,
      });
    } catch (err) {
      setDownloads((prev) =>
        prev.map((d) =>
          d.id === id ? { ...d, status: "failed", error: String(err) } : d
        )
      );
    }
  }

  async function handleCancel(id: string) {
    setDownloads((prev) =>
      prev.map((d) =>
        d.id === id && (d.status === "downloading" || d.status === "queued")
          ? { ...d, status: "cancelled" }
          : d
      )
    );
    try { await invoke("cancel_download", { id }); } catch (e) { console.error(e); }
  }

  function handleRemove(id: string) {
    setDownloads((prev) => prev.filter((d) => d.id !== id));
  }

  function handleClearFinished() {
    setDownloads((prev) =>
      prev.filter((d) => d.status === "downloading" || d.status === "queued")
    );
  }

  function handleOpenPath(path: string) {
    invoke("open_path", { path }).catch(console.error);
  }

  async function handleDeleteHistory(id: number) {
    await invoke("delete_history_item", { id }).catch(console.error);
    setHistory((prev) => prev.filter((h) => h.id !== id));
  }

  async function handleClearHistory() {
    await invoke("clear_history").catch(console.error);
    setHistory([]);
  }

  async function handleSaveSettings(next: Settings) {
    try {
      await invoke("save_settings", { settings: next });
    } catch (err) {
      // The backend rejects e.g. an unparseable speed limit. Surfacing that is
      // the whole point of having a Save button.
      setSettingsErr(String(err));
      return;
    }
    setSettingsErr(null);
    setSettings(next);
    // Apply folder + format immediately
    if (next.default_save_folder) setOutDir(next.default_save_folder);
    if (!media) setFormat(next.default_format);
    setSettingsSaved(true);
    if (savedTimer.current !== null) window.clearTimeout(savedTimer.current);
    savedTimer.current = window.setTimeout(() => setSettingsSaved(false), 2000);
  }

  const activeCount = downloads.filter((d) => d.status === "downloading" || d.status === "queued").length;
  const finishedCount = downloads.length - activeCount;
  const showEmpty   = tab === "download" && !fetching && !media && downloads.length === 0 && !fetchErr && !ytMissing;

  return (
    <div className="app-root">
      <Header version={ytVersion} missing={ytMissing} />

      {/* Tab bar */}
      <div className="tab-bar">
        <button
          className={`tab-btn ${tab === "download" ? "tab-active" : ""}`}
          onClick={() => setTab("download")}
        >
          Download
          {activeCount > 0 && <span className="tab-pill">{activeCount}</span>}
        </button>
        <button
          className={`tab-btn ${tab === "history" ? "tab-active" : ""}`}
          onClick={() => { setTab("history"); loadHistory(); }}
        >
          History
          {history.length > 0 && <span className="tab-pill">{history.length}</span>}
        </button>
        <button
          className={`tab-btn ${tab === "settings" ? "tab-active" : ""}`}
          onClick={() => setTab("settings")}
        >
          Settings
        </button>
      </div>

      <div className="main">
        {/* ── Download Tab ──────────────────────────────────────────────── */}
        {tab === "download" && (
          <>
            {ytMissing && (
              <div className="banner warn">
                <span className="banner-icon">⚠</span>
                <div>
                  <div className="banner-title">yt-dlp not found</div>
                  <div className="banner-body">
                    Install with <code>brew install yt-dlp</code> or <code>pip install yt-dlp</code>
                  </div>
                </div>
              </div>
            )}

            {ffmpegMissing && !ytMissing && (
              <div className="banner warn">
                <span className="banner-icon">⚠</span>
                <div>
                  <div className="banner-title">ffmpeg not found</div>
                  <div className="banner-body">
                    Needed to merge video with audio and to save MP3s. Install with{" "}
                    <code>brew install ffmpeg</code>
                  </div>
                </div>
              </div>
            )}

            <URLBar url={url} setUrl={handleUrlChange} onFetch={handleFetch} fetching={fetching} />

            {fetchErr && (
              <div className="banner error">
                <span className="banner-icon">⚠</span>
                <span>{fetchErr}</span>
              </div>
            )}

            {media && !fetching && (
              <MediaPanel
                info={media}
                format={format}
                setFormat={setFormat}
                audioOnly={audioOnly}
                setAudioOnly={setAudioOnly}
                outDir={outDir}
                onSelectDir={handleSelectDir}
                onDownload={handleDownload}
              />
            )}

            {downloads.length > 0 && (
              <DownloadQueue
                downloads={downloads}
                finishedCount={finishedCount}
                onCancel={handleCancel}
                onRemove={handleRemove}
                onClearFinished={handleClearFinished}
                onOpenPath={handleOpenPath}
              />
            )}

            {showEmpty && (
              <div className="empty">
                <div className="empty-glyph">FETCH</div>
                <div className="empty-text">Paste a URL above — YouTube, SoundCloud, Vimeo and 1000+ more</div>
              </div>
            )}
          </>
        )}

        {/* ── History Tab ───────────────────────────────────────────────── */}
        {tab === "history" && (
          <HistoryPanel
            items={history}
            onDelete={handleDeleteHistory}
            onClear={handleClearHistory}
            onOpenPath={handleOpenPath}
          />
        )}

        {/* ── Settings Tab ──────────────────────────────────────────────── */}
        {tab === "settings" && (
          <SettingsPanel
            settings={settings}
            onSave={handleSaveSettings}
            saved={settingsSaved}
            error={settingsErr}
          />
        )}
      </div>
    </div>
  );
}
