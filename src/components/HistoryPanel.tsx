import { useState } from "react";
import type { HistoryItem } from "../types";

interface Props {
  items: HistoryItem[];
  onDelete: (id: number) => void;
  onClear: () => void;
  onOpenPath: (path: string) => void;
}

/** `created_at` is stored as unix seconds in a TEXT column. */
function parseTs(unixStr: string): number | null {
  const n = Number.parseInt(unixStr, 10);
  return Number.isFinite(n) ? n : null;
}

function timeAgo(unixStr: string) {
  const ts = parseTs(unixStr);
  if (ts === null) return "unknown";
  // Clamp: a clock change (or a row written by a machine in another timezone
  // setup) could otherwise render "-3m ago".
  const secs = Math.max(0, Math.floor(Date.now() / 1000) - ts);
  if (secs < 60) return "just now";
  if (secs < 3600) return `${Math.floor(secs / 60)}m ago`;
  if (secs < 86400) return `${Math.floor(secs / 3600)}h ago`;
  if (secs < 86400 * 30) return `${Math.floor(secs / 86400)}d ago`;
  return `${Math.floor(secs / (86400 * 30))}mo ago`;
}

function fullDate(unixStr: string) {
  const ts = parseTs(unixStr);
  return ts === null ? "" : new Date(ts * 1000).toLocaleString();
}

export default function HistoryPanel({ items, onDelete, onClear, onOpenPath }: Props) {
  const [confirmClear, setConfirmClear] = useState(false);

  if (items.length === 0) {
    return (
      <div className="empty">
        <div className="empty-glyph">HIST</div>
        <div className="empty-text">No downloads yet — completed downloads will appear here</div>
      </div>
    );
  }

  return (
    <div className="queue-wrap">
      <div className="queue-top">
        <span className="queue-heading">HISTORY</span>
        <span className="pill">{items.length} items</span>
        {/* Two-step, because this wipes every row with no undo. */}
        {confirmClear ? (
          <>
            <button
              className="act-btn act-btn-text danger"
              style={{ marginLeft: "auto" }}
              onClick={() => { onClear(); setConfirmClear(false); }}
            >
              Delete {items.length} entries
            </button>
            <button className="act-btn act-btn-text" onClick={() => setConfirmClear(false)}>
              Cancel
            </button>
          </>
        ) : (
          <button
            className="act-btn act-btn-text danger"
            style={{ marginLeft: "auto" }}
            onClick={() => setConfirmClear(true)}
            title="Clear all history"
          >
            Clear all
          </button>
        )}
      </div>

      <div className="queue-list">
        {items.map((item) => (
          <div key={item.id} className={`dl-item ${item.status}`}>
            <div className="dl-item-body">
              {item.thumbnail ? (
                <img className="dl-thumb" src={item.thumbnail} alt="" />
              ) : (
                <div className="dl-thumb-ph">▶</div>
              )}

              <div className="dl-info">
                <div className="dl-title" title={item.title}>{item.title}</div>
                <div className="dl-meta">
                  <span className={`status-chip ${item.status}`}>
                    {item.status === "completed" ? "DONE" : "FAILED"}
                  </span>
                  <div className="dl-stats">
                    <span className="stat-kv">
                      <span className="stat-k">fmt</span>
                      <span>{item.audio_only ? "MP3" : item.format_id.toUpperCase()}</span>
                    </span>
                    <span title={fullDate(item.created_at)}>{timeAgo(item.created_at)}</span>
                  </div>
                </div>
              </div>

              <div className="dl-actions">
                {item.output_path && item.status === "completed" && (
                  <button
                    className="act-btn success"
                    onClick={() => onOpenPath(item.output_path!)}
                    title="Reveal in file manager"
                  >
                    ⌘
                  </button>
                )}
                <button
                  className="act-btn danger"
                  onClick={() => onDelete(item.id)}
                  title="Remove from history"
                >
                  ✕
                </button>
              </div>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
