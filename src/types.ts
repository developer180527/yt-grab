// ─── Tools ────────────────────────────────────────────────────────────────────

/** Where the yt-dlp we're running came from. */
export type ToolSource = "managed" | "bundled" | "system" | "missing";

export interface ToolInfo {
  available: boolean;
  version: string | null;
  path: string | null;
  source: ToolSource;
  /** Days since the yt-dlp release. Stale builds break sites silently. */
  age_days: number | null;
}

export interface ToolStatus {
  ytdlp: ToolInfo;
  ffmpeg: ToolInfo;
}

/** Past this, a yt-dlp build is old enough to be a likely cause of failure. */
export const STALE_AFTER_DAYS = 60;

// ─── Failures ─────────────────────────────────────────────────────────────────

export type FailureKind =
  | "needs_auth"
  | "rate_limited"
  | "tool_outdated"
  | "format_unavailable"
  | "unsupported"
  | "geo_blocked"
  | "unavailable"
  | "missing_ffmpeg"
  | "network"
  | "disk"
  | "unknown";

/** `id` is the contract with the backend; `label` is presentation. */
export interface Remedy {
  id: string;
  label: string;
}

export interface Failure {
  kind: FailureKind;
  summary: string;
  raw: string;
  remedies: Remedy[];
}

export const REMEDY = {
  cookiesFromBrowser: "cookies_from_browser",
  importCookiesFile: "import_cookies_file",
  retryBestAvailable: "retry_best_available",
  updateYtdlp: "update_ytdlp",
  lowerConcurrency: "lower_concurrency",
  installFfmpeg: "install_ffmpeg",
  retry: "retry",
  chooseFolder: "choose_folder",
} as const;

// ─── Media ────────────────────────────────────────────────────────────────────

export interface YtFormat {
  format_id: string;
  ext: string;
  resolution: string;
  filesize: number | null;
  vcodec: string;
  acodec: string;
  format_note: string;
  tbr: number | null;
}

export interface MediaInfo {
  id: string;
  title: string;
  thumbnail: string | null;
  duration: number | null;
  uploader: string | null;
  formats: YtFormat[];
  webpage_url: string;
  /** Extractor that handled it ("Bandcamp", "Vimeo", …). */
  extractor: string;
  /** No video track at all — the UI hides resolution controls. */
  audio_only_source: boolean;
  is_live: boolean;
}

export interface FormatPreset {
  id: string;
  label: string;
  detail: string;
}

export const FORMAT_PRESETS: FormatPreset[] = [
  { id: "best",  label: "Best quality", detail: "Highest available" },
  { id: "2160p", label: "2160p",        detail: "4K" },
  { id: "1440p", label: "1440p",        detail: "2K" },
  { id: "1080p", label: "1080p",        detail: "Full HD" },
  { id: "720p",  label: "720p",         detail: "HD" },
  { id: "480p",  label: "480p",         detail: "SD" },
];

export const PRESET_IDS = new Set(FORMAT_PRESETS.map((p) => p.id));

export const AUDIO_FORMATS = [
  { id: "best", label: "Original", detail: "No re-encoding" },
  { id: "mp3",  label: "MP3",      detail: "Converted" },
  { id: "m4a",  label: "M4A",      detail: "Converted" },
];

// ─── Grabs ────────────────────────────────────────────────────────────────────

export type DownloadStatus = "queued" | "downloading" | "completed" | "failed" | "cancelled";

/** Which pass yt-dlp is on. Video and audio arrive as separate streams, so the
 *  progress bar legitimately restarts at 0% — the stage label says why. */
export type DownloadStage = "video" | "audio" | "merging" | "processing";

export const STAGE_LABEL: Record<DownloadStage, string> = {
  video: "video",
  audio: "audio",
  merging: "merging",
  processing: "processing",
};

export interface DownloadItem {
  id: string;
  url: string;
  title: string;
  thumbnail: string | null;
  format_id: string;
  output_dir: string;
  audio_only: boolean;
  /** Whether the chosen exact format needs a separate audio stream merged in.
   *  Kept on the item so a retry rebuilds the same request — without it, a
   *  retried video-only format would be re-fetched silent. */
  merge_audio: boolean;
  status: DownloadStatus;
  percent: number;
  speed: string;
  eta: string;
  size: string;
  stage: DownloadStage;
  /** Real size of the finished file on disk, in bytes. */
  final_size: number | null;
  failure: Failure | null;
  output_path: string | null;
}

/** What the UI asks for. Folder, cookies, subtitles and container are policy
 *  the backend resolves from settings plus the site rule. */
export interface GrabRequest {
  id: string;
  url: string;
  title: string;
  thumbnail: string | null;
  format_id: string;
  audio_only: boolean;
  merge_audio: boolean;
  output_dir: string | null;
}

// ─── Events ───────────────────────────────────────────────────────────────────

export interface StartedPayload { id: string }
export interface ProgressPayload {
  id: string; percent: number; speed: string; eta: string; size: string; stage: DownloadStage;
}
export interface CompletePayload { id: string; path: string; size: number | null }
export interface FailurePayload { id: string; failure: Failure }
export interface DeepLinkPayload { url: string }

// ─── History and rules ────────────────────────────────────────────────────────

export interface HistoryItem {
  id: number;
  url: string;
  title: string;
  thumbnail: string | null;
  format_id: string;
  audio_only: boolean;
  output_path: string | null;
  status: string;
  created_at: string;
}

/** What a site's rule pre-selects, applied after a resolve. A rule's format and
 *  audio-only are starting points for the panel, not settings applied behind
 *  the user's back — a grab uses whatever the panel shows when Grab is pressed. */
export interface SiteDefaults {
  domain: string | null;
  has_rule: boolean;
  format_id: string;
  audio_only: boolean;
  /** Present only when a rule names a folder. */
  output_dir: string | null;
}

export interface SiteRule {
  domain: string;
  cookies_browser: string | null;
  cookies_file: string | null;
  output_dir: string | null;
  format_id: string | null;
  audio_only: boolean | null;
}

// ─── Settings ─────────────────────────────────────────────────────────────────

export interface Settings {
  default_save_folder: string;
  default_format: string;
  /** "best" keeps the source codec; anything else transcodes. */
  audio_format: string;
  prefer_mp4: boolean;
  embed_thumbnail: boolean;
  embed_subtitles: boolean;
  subtitle_langs: string;
  speed_limit: string;
  cookies_browser: string;
  auto_open_folder: boolean;
  auto_delete_history_days: number;
  /** How many downloads run at once. Clamped to 1–8 by the backend. */
  max_concurrent: number;
}

export const DEFAULT_SETTINGS: Settings = {
  default_save_folder: "",
  default_format: "best",
  audio_format: "best",
  prefer_mp4: false,
  embed_thumbnail: false,
  embed_subtitles: false,
  subtitle_langs: "en",
  speed_limit: "",
  cookies_browser: "",
  auto_open_folder: false,
  auto_delete_history_days: 0,
  max_concurrent: 3,
};

export const BROWSERS = [
  "chrome", "chromium", "firefox", "safari", "brave", "edge", "opera", "vivaldi",
];
