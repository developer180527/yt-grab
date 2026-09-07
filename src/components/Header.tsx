import { STALE_AFTER_DAYS, type ToolStatus } from "../types";

interface Props {
  tools: ToolStatus | null;
}

export default function Header({ tools }: Props) {
  const ytdlp = tools?.ytdlp;

  const { cls, label, title } = (() => {
    if (!tools) return { cls: "dot amber", label: "checking…", title: "" };
    if (!ytdlp?.available) {
      return { cls: "dot red", label: "yt-dlp missing", title: "yt-dlp could not be run" };
    }
    const stale = (ytdlp.age_days ?? 0) > STALE_AFTER_DAYS;
    return {
      cls: stale ? "dot amber" : "dot",
      label: `yt-dlp ${ytdlp.version}`,
      // Where it came from matters when something misbehaves: a bundled copy
      // updates with the app, a system one is the user's to manage.
      title: [
        ytdlp.source === "bundled" ? "Bundled with the app" : `From ${ytdlp.source}`,
        ytdlp.path ?? "",
        ytdlp.age_days !== null ? `${ytdlp.age_days} days old` : "",
        tools.ffmpeg.available ? `ffmpeg ${tools.ffmpeg.version}` : "ffmpeg not found",
      ].filter(Boolean).join(" · "),
    };
  })();

  return (
    <header className="header">
      <div className="titlebar" data-tauri-drag-region />
      <div className="titlebar-content">
        <div className="header-badge" title={title}>
          <span className={cls} />
          <span>{label}</span>
        </div>
      </div>
    </header>
  );
}
