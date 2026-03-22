import type { DownloadItem } from "../types";

interface Props {
  downloads: DownloadItem[];
  onCancel: (id: string) => void;
  onRemove: (id: string) => void;
  onOpenPath: (path: string) => void;
}

const STATUS_CHIP: Record<string, { label: string; cls: string }> = {
  queued:      { label: "QUEUED",       cls: "queued" },
  downloading: { label: "DOWNLOADING",  cls: "downloading" },
  completed:   { label: "DONE",         cls: "completed" },
  failed:      { label: "FAILED",       cls: "failed" },
  cancelled:   { label: "CANCELLED",    cls: "cancelled" },
};

function Item({
  d, onCancel, onRemove, onOpenPath,
}: {
  d: DownloadItem;
  onCancel: (id: string) => void;
  onRemove: (id: string) => void;
  onOpenPath: (path: string) => void;
}) {
  const chip = STATUS_CHIP[d.status];
  const active = d.status === "downloading";
  const done   = d.status === "completed";
  const failed = d.status === "failed";
  const showBar = active || done;

  return (
    <div className={`dl-item ${d.status}`}>
      <div className="dl-item-body">
        {d.thumbnail ? (
          <img className="dl-thumb" src={d.thumbnail} alt="" />
        ) : (
          <div className="dl-thumb-ph">▶</div>
        )}

        <div className="dl-info">
          <div className="dl-title">{d.title}</div>
          <div className="dl-meta">
            <span className={`status-chip ${chip.cls}`}>{chip.label}</span>

            {active && (
              <div className="dl-stats">
                <span>{d.percent.toFixed(1)}%</span>
                {d.speed !== "--" && (
                  <span className="stat-kv">
                    <span className="stat-k">↑</span>
                    <span>{d.speed}</span>
                  </span>
                )}
                {d.eta !== "--" && (
                  <span className="stat-kv">
                    <span className="stat-k">ETA</span>
                    <span>{d.eta}</span>
                  </span>
                )}
                {d.size !== "--" && (
                  <span className="stat-kv">
                    <span className="stat-k">of</span>
                    <span>{d.size}</span>
                  </span>
                )}
              </div>
            )}

            {done && d.downloaded !== "--" && (
              <div className="dl-stats">
                <span>{d.downloaded}</span>
              </div>
            )}
          </div>
        </div>

        <div className="dl-actions">
          {done && d.output_path && (
            <button
              className="act-btn success"
              onClick={() => onOpenPath(d.output_path!)}
              title="Reveal in Finder"
            >
              ⌘
            </button>
          )}
          {(active || d.status === "queued") && (
            <button
              className="act-btn danger"
              onClick={() => onCancel(d.id)}
              title="Cancel"
            >
              ✕
            </button>
          )}
          {(done || failed || d.status === "cancelled") && (
            <button
              className="act-btn"
              onClick={() => onRemove(d.id)}
              title="Remove"
            >
              ✕
            </button>
          )}
        </div>
      </div>

      {showBar && (
        <div className="prog-wrap">
          <div className="prog-track">
            <div
              className={`prog-fill ${done ? "completed" : ""}`}
              style={{ width: `${d.percent}%` }}
            />
          </div>
        </div>
      )}

      {failed && d.error && (
        <div className="dl-error">
          {d.error.length > 120 ? d.error.slice(0, 120) + "…" : d.error}
        </div>
      )}
    </div>
  );
}

export default function DownloadQueue({ downloads, onCancel, onRemove, onOpenPath }: Props) {
  const activeCount = downloads.filter(
    (d) => d.status === "downloading" || d.status === "queued"
  ).length;

  return (
    <div className="queue-wrap">
      <div className="queue-top">
        <span className="queue-heading">QUEUE</span>
        <span className={`pill ${activeCount > 0 ? "active" : ""}`}>
          {activeCount > 0 ? `${activeCount} active` : `${downloads.length} total`}
        </span>
      </div>
      <div className="queue-list">
        {downloads.map((d) => (
          <Item key={d.id} d={d} onCancel={onCancel} onRemove={onRemove} onOpenPath={onOpenPath} />
        ))}
      </div>
    </div>
  );
}
