import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { downloadDir } from "@tauri-apps/api/path";
import Header from "./components/Header";
import URLBar from "./components/URLBar";
import MediaPanel from "./components/MediaPanel";
import DownloadQueue from "./components/DownloadQueue";
import HistoryPanel from "./components/HistoryPanel";
import type {
  MediaInfo,
  DownloadItem,
  ProgressPayload,
  CompletePayload,
  ErrorPayload,
  HistoryItem,
} from "./types";

type Tab = "download" | "history";

export default function App() {
  const [ytVersion, setYtVersion]   = useState<string | null>(null);
  const [ytMissing, setYtMissing]   = useState(false);

  const [url, setUrl]               = useState("");
  const [fetching, setFetching]     = useState(false);
  const [fetchErr, setFetchErr]     = useState<string | null>(null);
  const [media, setMedia]           = useState<MediaInfo | null>(null);

  const [format, setFormat]         = useState("best");
  const [audioOnly, setAudioOnly]   = useState(false);
  const [outDir, setOutDir]         = useState("");

  const [downloads, setDownloads]   = useState<DownloadItem[]>([]);
  const [history, setHistory]       = useState<HistoryItem[]>([]);
  const [tab, setTab]               = useState<Tab>("download");

  // ── init ──────────────────────────────────────────────────────────────────
  useEffect(() => {
    invoke<string>("check_ytdlp")
      .then((v) => setYtVersion(v))
      .catch(() => setYtMissing(true));

    downloadDir().then((d) => setOutDir(d)).catch(() => {});
    loadHistory();
  }, []);

  function loadHistory() {
    invoke<HistoryItem[]>("get_history")
      .then(setHistory)
      .catch(console.error);
  }

  // ── Tauri events ──────────────────────────────────────────────────────────
  useEffect(() => {
    const subs = [
      listen<ProgressPayload>("download:progress", ({ payload: p }) => {
        setDownloads((prev) =>
          prev.map((d) =>
            d.id === p.id
              ? { ...d, status: "downloading", percent: p.percent, speed: p.speed, eta: p.eta, size: p.size, downloaded: p.downloaded }
              : d
          )
        );
      }),

      listen<CompletePayload>("download:complete", ({ payload: p }) => {
        setDownloads((prev) =>
          prev.map((d) =>
            d.id === p.id ? { ...d, status: "completed", percent: 100, output_path: p.path } : d
          )
        );
        loadHistory();
      }),

      listen<ErrorPayload>("download:error", ({ payload: p }) => {
        setDownloads((prev) =>
          prev.map((d) =>
            d.id === p.id ? { ...d, status: "failed", error: p.message } : d
          )
        );
        loadHistory();
      }),
    ];

    return () => { subs.forEach((p) => p.then((fn) => fn())); };
  }, []);

  // ── handlers ──────────────────────────────────────────────────────────────
  async function handleFetch() {
    if (!url.trim()) return;
    setFetching(true);
    setFetchErr(null);
    setMedia(null);
    try {
      const info = await invoke<MediaInfo>("fetch_media_info", { url: url.trim() });
      setMedia(info);
      setFormat("best");
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
    if (!media) return;
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
      speed: "--", eta: "--", size: "--", downloaded: "--",
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
    try { await invoke("cancel_download", { id }); } catch {}
    setDownloads((prev) =>
      prev.map((d) =>
        d.id === id && (d.status === "downloading" || d.status === "queued")
          ? { ...d, status: "cancelled" }
          : d
      )
    );
  }

  function handleRemove(id: string) {
    setDownloads((prev) => prev.filter((d) => d.id !== id));
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

  const showEmpty = tab === "download" && !fetching && !media && downloads.length === 0 && !fetchErr && !ytMissing;

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
          {downloads.filter((d) => d.status === "downloading" || d.status === "queued").length > 0 && (
            <span className="tab-pill">
              {downloads.filter((d) => d.status === "downloading" || d.status === "queued").length}
            </span>
          )}
        </button>
        <button
          className={`tab-btn ${tab === "history" ? "tab-active" : ""}`}
          onClick={() => { setTab("history"); loadHistory(); }}
        >
          History
          {history.length > 0 && <span className="tab-pill">{history.length}</span>}
        </button>
      </div>

      <div className="main">
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

            <URLBar url={url} setUrl={setUrl} onFetch={handleFetch} fetching={fetching} />

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
                onCancel={handleCancel}
                onRemove={handleRemove}
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

        {tab === "history" && (
          <HistoryPanel
            items={history}
            onDelete={handleDeleteHistory}
            onClear={handleClearHistory}
            onOpenPath={handleOpenPath}
          />
        )}
      </div>
    </div>
  );
}
