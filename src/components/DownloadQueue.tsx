import { formatBytes } from "../format";
import { STAGE_LABEL, type DownloadItem, type DownloadStatus, type Remedy } from "../types";
import FailureBanner from "./FailureBanner";

interface Props {
  downloads: DownloadItem[];
  finishedCount: number;
  onCancel: (id: string) => void;
  onRemove: (id: string) => void;
  onClearFinished: () => void;
  onOpenPath: (path: string) => void;
  onRemedy: (item: DownloadItem, remedy: Remedy, value?: string) => void;
  pickCookiesFile: () => Promise<string | undefined>;
}

const STATUS_CHIP: Record<DownloadStatus, { label: string; cls: string }> = {
  queued:      { label: "WAITING",     cls: "queued" },
  downloading: { label: "DOWNLOADING", cls: "downloading" },
  completed:   { label: "DONE",        cls: "completed" },
  failed:      { label: "FAILED",      cls: "failed" },
  cancelled:   { label: "CANCELLED",   cls: "cancelled" },
};

function Item({
  d, onCancel, onRemove, onOpenPath, onRemedy, pickCookiesFile,
}: {
  d: DownloadItem;
  onCancel: (id: string) => void;
  onRemove: (id: string) => void;
  onOpenPath: (path: string) => void;
  onRemedy: (item: DownloadItem, remedy: Remedy, value?: string) => void;
  pickCookiesFile: () => Promise<string | undefined>;
}) {
  // A status from a different build shouldn't crash the whole queue.
  const chip = STATUS_CHIP[d.status] ?? { label: String(d.status).toUpperCase(), cls: "queued" };
  const active = d.status === "downloading";
  const done = d.status === "completed";
  const failed = d.status === "failed";

  return (
    <div className={`dl-item ${d.status}`}>
      <div className="dl-item-body">
        {d.thumbnail
          ? <img className="dl-thumb" src={d.thumbnail} alt="" />
          : <div className="dl-thumb-ph">▶</div>}

        <div className="dl-info">
          <div className="dl-title" title={d.title}>{d.title}</div>
          <div className="dl-meta">
            <span className={`status-chip ${chip.cls}`}>{chip.label}</span>

            {active && (
              <div className="dl-stats">
                <span>{d.percent.toFixed(1)}%</span>
                {/* yt-dlp fetches video and audio as separate passes, so the bar
                    restarts at 0%. Naming the pass keeps that from reading as a
                    download that reset itself. */}
                <span className="stat-kv"><span className="stat-k">{STAGE_LABEL[d.stage] ?? d.stage}</span></span>
                {d.speed !== "--" && (
                  <span className="stat-kv"><span className="stat-k">↓</span><span>{d.speed}</span></span>
                )}
                {d.eta !== "--" && (
                  <span className="stat-kv"><span className="stat-k">ETA</span><span>{d.eta}</span></span>
                )}
                {d.size !== "--" && (
                  <span className="stat-kv"><span className="stat-k">of</span><span>{d.size}</span></span>
                )}
              </div>
            )}

            {/* The size that actually landed on disk — for a merged download
                that is more than the last stream yt-dlp reported. */}
            {done && (formatBytes(d.final_size) ?? (d.size !== "--" ? d.size : null)) && (
              <div className="dl-stats"><span>{formatBytes(d.final_size) ?? d.size}</span></div>
            )}
          </div>
        </div>

        <div className="dl-actions">
          {done && d.output_path && (
            <button className="act-btn success" onClick={() => onOpenPath(d.output_path!)} title="Reveal in file manager">⌘</button>
          )}
          {(active || d.status === "queued") && (
            <button className="act-btn danger" onClick={() => onCancel(d.id)} title="Cancel">✕</button>
          )}
          {(done || failed || d.status === "cancelled") && (
            <button className="act-btn" onClick={() => onRemove(d.id)} title="Remove from queue">✕</button>
          )}
        </div>
      </div>

      {(active || done) && (
        <div className="prog-wrap">
          <div className="prog-track">
            <div
              className={`prog-fill ${done ? "completed" : ""}`}
              style={{ width: `${Math.min(100, Math.max(0, d.percent))}%` }}
            />
          </div>
        </div>
      )}

      {failed && d.failure && (
        <FailureBanner
          failure={d.failure}
          url={d.url}
          onRemedy={(r, v) => onRemedy(d, r, v)}
          pickCookiesFile={pickCookiesFile}
        />
      )}
    </div>
  );
}

export default function DownloadQueue({
  downloads, finishedCount, onCancel, onRemove, onClearFinished, onOpenPath,
  onRemedy, pickCookiesFile,
}: Props) {
  // Now that the scheduler caps concurrency, "queued" means genuinely waiting
  // for a slot rather than about to start, so the two are worth separating.
  const running = downloads.filter((d) => d.status === "downloading").length;
  const waiting = downloads.filter((d) => d.status === "queued").length;

  return (
    <div className="queue-wrap">
      <div className="queue-top">
        <span className="queue-heading">QUEUE</span>
        {running + waiting === 0 ? (
          <span className="pill">{downloads.length} total</span>
        ) : (
          <>
            {running > 0 && <span className="pill active">{running} running</span>}
            {waiting > 0 && <span className="pill">{waiting} waiting</span>}
          </>
        )}
        {finishedCount > 0 && (
          <button
            className="act-btn act-btn-text"
            style={{ marginLeft: "auto" }}
            onClick={onClearFinished}
            title="Remove finished, failed and cancelled items"
          >
            Clear finished
          </button>
        )}
      </div>
      <div className="queue-list">
        {downloads.map((d) => (
          <Item
            key={d.id}
            d={d}
            onCancel={onCancel}
            onRemove={onRemove}
            onOpenPath={onOpenPath}
            onRemedy={onRemedy}
            pickCookiesFile={pickCookiesFile}
          />
        ))}
      </div>
    </div>
  );
}
