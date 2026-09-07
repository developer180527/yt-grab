import { useMemo } from "react";
import { formatBytes } from "../format";
import { AUDIO_FORMATS, FORMAT_PRESETS, type MediaInfo, type YtFormat } from "../types";

interface Props {
  info: MediaInfo;
  format: string;
  setFormat: (v: string) => void;
  audioOnly: boolean;
  setAudioOnly: (v: boolean) => void;
  audioFormat: string;
  outDir: string;
  onSelectDir: () => void;
  onDownload: () => void;
}

function fmtDuration(secs: number | null) {
  if (secs === null || !Number.isFinite(secs) || secs <= 0) return null;
  const total = Math.round(secs);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

/** Pixel height of a `1920×1080`-style resolution string, for sorting. */
function heightOf(f: YtFormat) {
  const m = /×(\d+)/.exec(f.resolution);
  return m ? Number(m[1]) : 0;
}

export default function MediaPanel({
  info, format, setFormat, audioOnly, setAudioOnly, audioFormat,
  outDir, onSelectDir, onDownload,
}: Props) {
  // yt-dlp returns dozens of near-identical variants. Show the best bitrate per
  // resolution+container so the list stays usable.
  const customFormats = useMemo(() => {
    const best = new Map<string, YtFormat>();
    for (const f of info.formats) {
      if (f.vcodec === "none") continue;
      const key = `${f.resolution}|${f.ext}`;
      const cur = best.get(key);
      if (!cur || (f.tbr ?? 0) > (cur.tbr ?? 0)) best.set(key, f);
    }
    return [...best.values()].sort(
      (a, b) => heightOf(b) - heightOf(a) || (b.tbr ?? 0) - (a.tbr ?? 0)
    );
  }, [info.formats]);

  const dur = fmtDuration(info.duration);
  // Resolution controls are meaningless for a source with no video track, so a
  // Bandcamp album or a podcast doesn't get asked what quality of video it is.
  const videoAvailable = !info.audio_only_source;
  const audioLabel = AUDIO_FORMATS.find((a) => a.id === audioFormat)?.label ?? audioFormat;

  function toggleAudioOnly() {
    const next = !audioOnly;
    setAudioOnly(next);
    // A custom video format id is meaningless once the output is audio.
    if (next) setFormat("best");
  }

  return (
    <div className="media-panel">
      <div className="media-header">
        {info.thumbnail
          ? <img className="media-thumb" src={info.thumbnail} alt="" />
          : <div className="media-thumb-ph">▶</div>}
        <div className="media-meta">
          <div className="media-title" title={info.title}>{info.title}</div>
          <div className="media-attrs">
            {info.extractor && (
              <span className="media-attr source-badge" title="Extractor that handled this link">
                {info.extractor}
              </span>
            )}
            {info.uploader && (
              <span className="media-attr"><span className="media-attr-icon">◎</span>{info.uploader}</span>
            )}
            {dur && (
              <span className="media-attr"><span className="media-attr-icon">◷</span>{dur}</span>
            )}
            {info.formats.length > 0 && (
              <span className="media-attr"><span className="media-attr-icon">◈</span>{info.formats.length} formats</span>
            )}
          </div>
        </div>
      </div>

      {info.is_live && (
        <div className="inline-note">
          Live stream — there's no total length, so progress will show elapsed data rather than a percentage.
        </div>
      )}

      <div className="dl-options">
        <div className="row">
          {videoAvailable ? (
            <div className="field">
              <span className="field-label">Format</span>
              <select
                className="select"
                value={format}
                onChange={(e) => setFormat(e.target.value)}
                disabled={audioOnly}
              >
                {FORMAT_PRESETS.map((p) => (
                  <option key={p.id} value={p.id}>{p.label} — {p.detail}</option>
                ))}
                {customFormats.length > 0 && (
                  <optgroup label="Exact formats">
                    {customFormats.map((f) => (
                      <option key={f.format_id} value={f.format_id}>
                        {f.resolution} · {f.ext.toUpperCase()}
                        {f.format_note ? ` · ${f.format_note}` : ""}
                        {formatBytes(f.filesize) ? ` · ${formatBytes(f.filesize)}` : ""}
                      </option>
                    ))}
                  </optgroup>
                )}
              </select>
            </div>
          ) : (
            <div className="field">
              <span className="field-label">Format</span>
              <div className="static-field">
                Audio only — saving as {audioLabel}
              </div>
            </div>
          )}

          {videoAvailable && (
            <div className="field" style={{ flexGrow: 0, minWidth: "auto" }}>
              <span className="field-label">Audio only ({audioLabel})</span>
              <div
                className="toggle-wrap"
                role="switch"
                aria-checked={audioOnly}
                tabIndex={0}
                onClick={toggleAudioOnly}
                onKeyDown={(e) => {
                  if (e.key === " " || e.key === "Enter") { e.preventDefault(); toggleAudioOnly(); }
                }}
              >
                <div className={`toggle-track ${audioOnly ? "on" : ""}`}>
                  <div className="toggle-thumb" />
                </div>
                <span className="toggle-label">{audioOnly ? "On" : "Off"}</span>
              </div>
            </div>
          )}
        </div>

        <div className="row">
          <div className="field">
            <span className="field-label">Save to</span>
            <div className="path-row">
              <div className="path-val" title={outDir}>{outDir || "No folder selected"}</div>
              <button className="btn-icon" onClick={onSelectDir} title="Choose folder">⌘</button>
            </div>
          </div>
        </div>

        <div className="dl-actions">
          <button
            className="btn-download"
            onClick={onDownload}
            disabled={!outDir}
            title={outDir ? undefined : "Choose a save folder first"}
          >
            ↓ Grab
          </button>
        </div>
      </div>
    </div>
  );
}
