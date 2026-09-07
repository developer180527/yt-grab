import { useState, useEffect, useRef, useCallback } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { downloadDir } from "@tauri-apps/api/path";
import * as api from "./api";
import Header from "./components/Header";
import URLBar from "./components/URLBar";
import MediaPanel from "./components/MediaPanel";
import DownloadQueue from "./components/DownloadQueue";
import HistoryPanel from "./components/HistoryPanel";
import SettingsPanel from "./components/SettingsPanel";
import FailureBanner from "./components/FailureBanner";
import {
  DEFAULT_SETTINGS, PRESET_IDS, REMEDY, STALE_AFTER_DAYS,
  type DownloadItem, type Failure, type HistoryItem, type MediaInfo,
  type Remedy, type Settings, type SiteRule, type ToolStatus,
} from "./types";

type Tab = "download" | "history" | "settings";

export default function App() {
  const [tools, setTools] = useState<ToolStatus | null>(null);

  const [url, setUrl] = useState("");
  const [fetching, setFetching] = useState(false);
  const [fetchFailure, setFetchFailure] = useState<Failure | null>(null);
  const [media, setMedia] = useState<MediaInfo | null>(null);

  const [format, setFormat] = useState("best");
  const [audioOnly, setAudioOnly] = useState(false);
  const [outDir, setOutDir] = useState("");

  const [downloads, setDownloads] = useState<DownloadItem[]>([]);
  const [history, setHistory] = useState<HistoryItem[]>([]);
  const [rules, setRules] = useState<SiteRule[]>([]);
  const [tab, setTab] = useState<Tab>("download");

  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [settingsSaved, setSettingsSaved] = useState(false);
  const [settingsErr, setSettingsErr] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  // Read inside event handlers. Keeping the live value in a ref lets the
  // subscription be set up exactly once instead of tearing down and re-adding
  // itself whenever a setting changes.
  const settingsRef = useRef(settings);
  settingsRef.current = settings;

  const savedTimer = useRef<number | null>(null);
  const noticeTimer = useRef<number | null>(null);

  const flashNotice = useCallback((msg: string) => {
    setNotice(msg);
    if (noticeTimer.current !== null) window.clearTimeout(noticeTimer.current);
    noticeTimer.current = window.setTimeout(() => setNotice(null), 3500);
  }, []);

  const loadHistory = useCallback(() => {
    api.getHistory().then(setHistory).catch(console.error);
  }, []);
  const loadRules = useCallback(() => {
    api.listSiteRules().then(setRules).catch(console.error);
  }, []);

  // ── init ────────────────────────────────────────────────────────────────
  useEffect(() => {
    api.toolStatus().then(setTools).catch(console.error);

    api.getSettings().then((s) => {
      setSettings(s);
      setFormat(s.default_format || "best");
      if (s.default_save_folder) setOutDir(s.default_save_folder);
      else downloadDir().then(setOutDir).catch(() => {});
      if (s.auto_delete_history_days > 0) {
        api.purgeOldHistory(s.auto_delete_history_days).then(loadHistory).catch(console.error);
      }
    }).catch(() => {
      downloadDir().then(setOutDir).catch(() => {});
    });

    loadHistory();
    loadRules();

    return () => {
      if (savedTimer.current !== null) window.clearTimeout(savedTimer.current);
      if (noticeTimer.current !== null) window.clearTimeout(noticeTimer.current);
    };
  }, [loadHistory, loadRules]);

  // ── backend events ──────────────────────────────────────────────────────
  useEffect(() => api.subscribe({
    onStarted: ({ id }) => {
      setDownloads((prev) => prev.map((d) =>
        d.id === id && d.status === "queued" ? { ...d, status: "downloading" } : d));
    },
    onProgress: (p) => {
      setDownloads((prev) => prev.map((d) =>
        // A cancelled item can still receive one last in-flight progress event;
        // without this guard it would flip back to downloading.
        d.id === p.id && (d.status === "queued" || d.status === "downloading")
          ? { ...d, status: "downloading", percent: p.percent, speed: p.speed, eta: p.eta, size: p.size, stage: p.stage }
          : d));
    },
    onComplete: (p) => {
      setDownloads((prev) => prev.map((d) =>
        d.id === p.id
          ? { ...d, status: "completed", percent: 100, output_path: p.path, final_size: p.size }
          : d));
      loadHistory();
      if (settingsRef.current.auto_open_folder && p.path) {
        api.openPath(p.path).catch(console.error);
      }
    },
    onFailure: (p) => {
      setDownloads((prev) => prev.map((d) =>
        d.id === p.id ? { ...d, status: "failed", failure: p.failure } : d));
      loadHistory();
    },
    // A deep link only ever fills the bar. Starting a download from a link any
    // web page can trigger would let a page cause network fetches and disk
    // writes without the user choosing to.
    onDeepLink: ({ url: incoming }) => {
      setTab("download");
      setUrl(incoming);
      setFetchFailure(null);
      void runResolve(incoming);
    },
  }), [loadHistory]);

  // ── resolving ───────────────────────────────────────────────────────────
  const runResolve = useCallback(async (raw: string) => {
    const trimmed = raw.trim();
    if (!trimmed) return;

    setFetching(true);
    setFetchFailure(null);
    setMedia(null);
    try {
      const info = await api.resolveUrl(trimmed);
      setMedia(info);
      // An audio-only source has no resolutions to choose between.
      setAudioOnly(info.audio_only_source);
      setFormat(settingsRef.current.default_format || "best");
    } catch (err) {
      // Classify, so the same remedy buttons appear as on a failed download.
      try {
        setFetchFailure(await api.diagnoseError(trimmed, String(err)));
      } catch {
        setFetchFailure({ kind: "unknown", summary: String(err), raw: String(err), remedies: [] });
      }
    } finally {
      setFetching(false);
    }
  }, []);

  function handleUrlChange(v: string) {
    setUrl(v);
    if (fetchFailure) setFetchFailure(null);
  }

  async function handleFetch() {
    const trimmed = url.trim();
    if (!trimmed || fetching) return;
    if (!/^https?:\/\/\S+$/i.test(trimmed)) {
      setFetchFailure({
        kind: "unsupported",
        summary: "That doesn't look like a link. Paste a full URL starting with http:// or https://",
        raw: trimmed,
        remedies: [],
      });
      return;
    }
    await runResolve(trimmed);
  }

  async function handleSelectDir() {
    const selected = await openDialog({ directory: true, multiple: false });
    if (typeof selected === "string") setOutDir(selected);
  }

  // ── grabbing ────────────────────────────────────────────────────────────
  const startGrab = useCallback(async (
    src: { url: string; title: string; thumbnail: string | null },
    formatId: string,
    isAudioOnly: boolean,
    mergeAudio: boolean,
  ) => {
    const id = crypto.randomUUID();
    setDownloads((prev) => [{
      id,
      url: src.url,
      title: src.title,
      thumbnail: src.thumbnail,
      format_id: formatId,
      output_dir: outDir,
      audio_only: isAudioOnly,
      status: "queued",
      percent: 0,
      speed: "--", eta: "--", size: "--",
      stage: isAudioOnly ? "audio" : "video",
      final_size: null,
      failure: null,
      output_path: null,
    }, ...prev]);

    try {
      await api.enqueueGrab({
        id,
        url: src.url,
        title: src.title,
        thumbnail: src.thumbnail,
        format_id: formatId,
        audio_only: isAudioOnly,
        merge_audio: mergeAudio,
        output_dir: outDir || null,
      });
    } catch (err) {
      const failure = await api.diagnoseError(src.url, String(err)).catch(() => null);
      setDownloads((prev) => prev.map((d) => d.id === id
        ? { ...d, status: "failed", failure: failure ?? { kind: "unknown", summary: String(err), raw: String(err), remedies: [] } }
        : d));
    }
  }, [outDir]);

  async function handleDownload() {
    if (!media || !outDir) return;

    const duplicate = downloads.some((d) =>
      (d.status === "queued" || d.status === "downloading") &&
      d.url === media.webpage_url && d.output_dir === outDir);
    if (duplicate) {
      flashNotice("That's already in the queue.");
      return;
    }

    // A custom (non-preset) format id may point at a video-only stream;
    // downloading it as-is would produce a silent file.
    const chosen = PRESET_IDS.has(format) ? null : media.formats.find((f) => f.format_id === format);
    const mergeAudio = !audioOnly && !!chosen && chosen.vcodec !== "none" && chosen.acodec === "none";

    await startGrab(
      { url: media.webpage_url, title: media.title, thumbnail: media.thumbnail },
      format, audioOnly, mergeAudio,
    );
  }

  async function handleCancel(id: string) {
    setDownloads((prev) => prev.map((d) =>
      d.id === id && (d.status === "downloading" || d.status === "queued")
        ? { ...d, status: "cancelled" } : d));
    await api.cancelGrab(id).catch(console.error);
  }

  function handleRemove(id: string) {
    setDownloads((prev) => prev.filter((d) => d.id !== id));
  }

  function handleClearFinished() {
    setDownloads((prev) => prev.filter((d) => d.status === "downloading" || d.status === "queued"));
  }

  // ── remedies ────────────────────────────────────────────────────────────
  /**
   * Applies a remedy and retries. `item` is the failed download, or null when
   * the failure came from resolving rather than downloading.
   */
  const handleRemedy = useCallback(async (
    targetUrl: string,
    remedy: Remedy,
    value: string | undefined,
    item: DownloadItem | null,
  ) => {
    const retry = async (formatOverride?: string) => {
      if (item) {
        setDownloads((prev) => prev.filter((d) => d.id !== item.id));
        await startGrab(
          { url: item.url, title: item.title, thumbnail: item.thumbnail },
          formatOverride ?? item.format_id, item.audio_only, false,
        );
      } else {
        await runResolve(targetUrl);
      }
    };

    switch (remedy.id) {
      case REMEDY.cookiesFromBrowser:
      case REMEDY.importCookiesFile: {
        if (!value) return;
        try {
          const rule = await api.applyRemedy(targetUrl, remedy.id, value);
          loadRules();
          if (rule) flashNotice(`Saved for ${rule.domain} — you won't be asked again.`);
        } catch (err) {
          flashNotice(String(err));
          return;
        }
        await retry();
        break;
      }
      case REMEDY.lowerConcurrency: {
        await api.applyRemedy(targetUrl, remedy.id).catch(console.error);
        const s = await api.getSettings().catch(() => null);
        if (s) {
          setSettings(s);
          flashNotice(`Now running ${s.max_concurrent} at a time.`);
        }
        await retry();
        break;
      }
      case REMEDY.retryBestAvailable:
        await retry("best");
        break;
      case REMEDY.retry:
        await retry();
        break;
      case REMEDY.chooseFolder: {
        const selected = await openDialog({ directory: true, multiple: false });
        if (typeof selected === "string") {
          setOutDir(selected);
          await retry();
        }
        break;
      }
      case REMEDY.installFfmpeg:
        flashNotice("Install ffmpeg with: brew install ffmpeg");
        break;
      default:
        flashNotice(`Nothing wired up for "${remedy.id}" yet.`);
    }
  }, [flashNotice, loadRules, runResolve, startGrab]);

  /** Picks a cookies.txt for the site and remembers it. */
  const pickCookiesFile = useCallback(async () => {
    const selected = await openDialog({
      multiple: false,
      filters: [{ name: "Cookies", extensions: ["txt"] }],
    });
    return typeof selected === "string" ? selected : undefined;
  }, []);

  // ── history and settings ────────────────────────────────────────────────
  async function handleDeleteHistory(id: number) {
    await api.deleteHistoryItem(id).catch(console.error);
    setHistory((prev) => prev.filter((h) => h.id !== id));
  }

  async function handleClearHistory() {
    await api.clearHistory().catch(console.error);
    setHistory([]);
  }

  async function handleSaveSettings(next: Settings) {
    try {
      await api.saveSettings(next);
    } catch (err) {
      setSettingsErr(String(err));
      return;
    }
    setSettingsErr(null);
    setSettings(next);
    if (next.default_save_folder) setOutDir(next.default_save_folder);
    if (!media) setFormat(next.default_format);
    setSettingsSaved(true);
    if (savedTimer.current !== null) window.clearTimeout(savedTimer.current);
    savedTimer.current = window.setTimeout(() => setSettingsSaved(false), 2000);
  }

  async function handleDeleteRule(domain: string) {
    await api.deleteSiteRule(domain).catch(console.error);
    loadRules();
  }

  // ── derived ─────────────────────────────────────────────────────────────
  const activeCount = downloads.filter((d) => d.status === "downloading" || d.status === "queued").length;
  const finishedCount = downloads.length - activeCount;
  const ytdlpMissing = tools ? !tools.ytdlp.available : false;
  const ffmpegMissing = tools ? !tools.ffmpeg.available : false;
  const ytdlpStale = (tools?.ytdlp.age_days ?? 0) > STALE_AFTER_DAYS;
  const showEmpty = tab === "download" && !fetching && !media
    && downloads.length === 0 && !fetchFailure && !ytdlpMissing;

  return (
    <div className="app-root">
      <Header tools={tools} />

      <div className="tab-bar">
        <button className={`tab-btn ${tab === "download" ? "tab-active" : ""}`} onClick={() => setTab("download")}>
          Download
          {activeCount > 0 && <span className="tab-pill">{activeCount}</span>}
        </button>
        <button className={`tab-btn ${tab === "history" ? "tab-active" : ""}`}
                onClick={() => { setTab("history"); loadHistory(); }}>
          History
          {history.length > 0 && <span className="tab-pill">{history.length}</span>}
        </button>
        <button className={`tab-btn ${tab === "settings" ? "tab-active" : ""}`}
                onClick={() => { setTab("settings"); loadRules(); }}>
          Settings
        </button>
      </div>

      <div className="main">
        {tab === "download" && (
          <>
            {ytdlpMissing && (
              <div className="banner warn">
                <span className="banner-icon">⚠</span>
                <div>
                  <div className="banner-title">yt-dlp isn't available</div>
                  <div className="banner-body">
                    The bundled copy couldn't run. Install it with <code>brew install yt-dlp</code> and restart.
                  </div>
                </div>
              </div>
            )}

            {ytdlpStale && !ytdlpMissing && (
              <div className="banner warn">
                <span className="banner-icon">⟳</span>
                <div>
                  <div className="banner-title">yt-dlp is {tools?.ytdlp.age_days} days old</div>
                  <div className="banner-body">
                    Stale builds are the usual cause of sites that suddenly stop working.
                    {tools?.ytdlp.source === "system"
                      ? " Update it the way you installed it."
                      : " A newer app build will bring a newer copy."}
                  </div>
                </div>
              </div>
            )}

            {ffmpegMissing && !ytdlpMissing && (
              <div className="banner warn">
                <span className="banner-icon">⚠</span>
                <div>
                  <div className="banner-title">ffmpeg not found</div>
                  <div className="banner-body">
                    Needed to combine video with audio and to convert audio. Install with <code>brew install ffmpeg</code>
                  </div>
                </div>
              </div>
            )}

            <URLBar url={url} setUrl={handleUrlChange} onFetch={handleFetch} fetching={fetching} />

            {fetchFailure && (
              <FailureBanner
                failure={fetchFailure}
                url={url.trim()}
                onRemedy={(r, v) => handleRemedy(url.trim(), r, v, null)}
                pickCookiesFile={pickCookiesFile}
              />
            )}

            {media && !fetching && (
              <MediaPanel
                info={media}
                format={format}
                setFormat={setFormat}
                audioOnly={audioOnly}
                setAudioOnly={setAudioOnly}
                audioFormat={settings.audio_format}
                outDir={outDir}
                onSelectDir={handleSelectDir}
                onDownload={handleDownload}
              />
            )}

            {notice && (
              <div className="banner">
                <span className="banner-icon">i</span>
                <span>{notice}</span>
              </div>
            )}

            {downloads.length > 0 && (
              <DownloadQueue
                downloads={downloads}
                finishedCount={finishedCount}
                onCancel={handleCancel}
                onRemove={handleRemove}
                onClearFinished={handleClearFinished}
                onOpenPath={(p) => api.openPath(p).catch(console.error)}
                onRemedy={(item, r, v) => handleRemedy(item.url, r, v, item)}
                pickCookiesFile={pickCookiesFile}
              />
            )}

            {showEmpty && (
              <div className="empty">
                <div className="empty-glyph">GRAB</div>
                <div className="empty-text">
                  Paste a link — YouTube, Bandcamp, SoundCloud, Vimeo and 1000+ more
                </div>
              </div>
            )}
          </>
        )}

        {tab === "history" && (
          <HistoryPanel
            items={history}
            onDelete={handleDeleteHistory}
            onClear={handleClearHistory}
            onOpenPath={(p) => api.openPath(p).catch(console.error)}
          />
        )}

        {tab === "settings" && (
          <SettingsPanel
            settings={settings}
            onSave={handleSaveSettings}
            saved={settingsSaved}
            error={settingsErr}
            rules={rules}
            onDeleteRule={handleDeleteRule}
            tools={tools}
          />
        )}
      </div>
    </div>
  );
}
