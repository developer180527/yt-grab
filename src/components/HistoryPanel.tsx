import { invoke } from "@tauri-apps/api/core";
import type { HistoryItem } from "../types";

interface Props {
  items: HistoryItem[];
  onDelete: (id: number) => void;
  onClear: () => void;
  onOpenPath: (path: string) => void;
}

function timeAgo(unixStr: string) {
  const secs = Math.floor(Date.now() / 1000) - parseInt(unixStr, 10);
  if (secs < 60)  return "just now";
  if (secs < 3600) return `${Math.floor(secs / 60)}m ago`;
  if (secs < 86400) return `${Math.floor(secs / 3600)}h ago`;
  return `${Math.floor(secs / 86400)}d ago`;
}

export default function HistoryPanel({ items, onDelete, onClear, onOpenPath }: Props) {
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
        <button
          className="act-btn danger"
          style={{ marginLeft: "auto" }}
          onClick={onClear}
          title="Clear all history"
        >
          Clear all
        </button>
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
                <div className="dl-title">{item.title}</div>
                <div className="dl-meta">
                  <span className={`status-chip ${item.status}`}>
                    {item.status === "completed" ? "DONE" : "FAILED"}
                  </span>
                  <div className="dl-stats">
                    <span className="stat-kv">
                      <span className="stat-k">fmt</span>
                      <span>{item.audio_only ? "MP3" : item.format_id.toUpperCase()}</span>
                    </span>
                    <span>{timeAgo(item.created_at)}</span>
                  </div>
                </div>
              </div>

              <div className="dl-actions">
                {item.output_path && item.status === "completed" && (
                  <button
                    className="act-btn success"
                    onClick={() => onOpenPath(item.output_path!)}
                    title="Reveal in Finder"
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
