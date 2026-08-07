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
}

export type DownloadStatus = "queued" | "downloading" | "completed" | "failed" | "cancelled";

/** Which pass yt-dlp is on. Video and audio arrive as separate streams, so the
 *  progress bar legitimately restarts at 0% — the stage label says why. */
export type DownloadStage = "video" | "audio" | "merging" | "processing";

export const STAGE_LABEL: Record<DownloadStage, string> = {
  video:      "video",
  audio:      "audio",
  merging:    "merging",
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
  status: DownloadStatus;
  percent: number;
  speed: string;
  eta: string;
  size: string;
  stage: DownloadStage;
  /** Real size of the finished file on disk, in bytes. */
  final_size: number | null;
  error: string | null;
  output_path: string | null;
}

export interface FormatPreset {
  id: string;
  label: string;
  detail: string;
}

export const FORMAT_PRESETS: FormatPreset[] = [
  { id: "best",  label: "Best Quality", detail: "Highest video + audio" },
  { id: "1080p", label: "1080p MP4",    detail: "Full HD" },
  { id: "720p",  label: "720p MP4",     detail: "HD" },
  { id: "480p",  label: "480p MP4",     detail: "SD" },
];

export interface ProgressPayload {
  id: string; percent: number; speed: string; eta: string; size: string; stage: DownloadStage;
}
export interface CompletePayload { id: string; path: string; size: number | null; }
export interface ErrorPayload    { id: string; message: string; }

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

export interface Settings {
  default_save_folder: string;
  default_format: string;
  embed_thumbnail: boolean;
  embed_subtitles: boolean;
  speed_limit: string;
  cookies_browser: string;
  auto_open_folder: boolean;
  auto_delete_history_days: number;
}
