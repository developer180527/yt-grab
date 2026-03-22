import type { MediaInfo } from "../types";
import { FORMAT_PRESETS } from "../types";

interface Props {
  info: MediaInfo;
  format: string;
  setFormat: (v: string) => void;
  audioOnly: boolean;
  setAudioOnly: (v: boolean) => void;
  outDir: string;
  onSelectDir: () => void;
  onDownload: () => void;
}

function fmtDuration(secs: number | null) {
  if (!secs) return null;
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  const mm = String(m).padStart(2, "0");
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${m}:${ss}`;
}

function fmtSize(bytes: number | null) {
  if (!bytes) return null;
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
  return `${(bytes / 1e3).toFixed(0)} KB`;
}

export default function MediaPanel({
  info, format, setFormat, audioOnly, setAudioOnly,
  outDir, onSelectDir, onDownload,
}: Props) {
  const customFormats = info.formats.filter(
    (f) => f.vcodec !== "none" && f.resolution !== "audio only"
  );
  const dur = fmtDuration(info.duration);

  return (
    <div className="media-panel">
      <div className="media-header">
        {info.thumbnail ? (
          <img className="media-thumb" src={info.thumbnail} alt="" />
        ) : (
          <div className="media-thumb-ph">▶</div>
        )}
        <div className="media-meta">
          <div className="media-title">{info.title}</div>
          <div className="media-attrs">
            {info.uploader && (
              <span className="media-attr">
                <span className="media-attr-icon">◎</span>
                {info.uploader}
              </span>
            )}
            {dur && (
              <span className="media-attr">
                <span className="media-attr-icon">◷</span>
                {dur}
              </span>
            )}
            {info.formats.length > 0 && (
              <span className="media-attr">
                <span className="media-attr-icon">◈</span>
                {info.formats.length} formats
              </span>
            )}
          </div>
        </div>
      </div>

      <div className="dl-options">
        <div className="row">
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
                <optgroup label="Custom formats">
                  {customFormats.map((f) => (
                    <option key={f.format_id} value={f.format_id}>
                      {f.resolution} · {f.ext.toUpperCase()}
                      {fmtSize(f.filesize) ? ` · ${fmtSize(f.filesize)}` : ""}
                    </option>
                  ))}
                </optgroup>
              )}
            </select>
          </div>

          <div className="field" style={{ flexGrow: 0, minWidth: "auto" }}>
            <span className="field-label">Audio only (MP3)</span>
            <div
              className="toggle-wrap"
              onClick={() => { setAudioOnly(!audioOnly); if (!audioOnly) setFormat("best"); }}
            >
              <div className={`toggle-track ${audioOnly ? "on" : ""}`}>
                <div className="toggle-thumb" />
              </div>
              <span className="toggle-label">{audioOnly ? "On" : "Off"}</span>
            </div>
          </div>
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
          <button className="btn-download" onClick={onDownload} disabled={!outDir}>
            ↓ Download
          </button>
        </div>
      </div>
    </div>
  );
}
