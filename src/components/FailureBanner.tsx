import { useState } from "react";
import { BROWSERS, REMEDY, type Failure, type Remedy } from "../types";

interface Props {
  failure: Failure;
  url: string;
  onRemedy: (remedy: Remedy, value?: string) => void;
  pickCookiesFile: () => Promise<string | undefined>;
}

/**
 * A failure with its fixes attached.
 *
 * The point of this component is that a failure is never a dead end: the
 * backend classifies the error and names the actions that would resolve it, and
 * each one is a button here. Raw yt-dlp output is kept but folded away, since
 * it is the thing you want fourth, not first.
 */
export default function FailureBanner({ failure, url, onRemedy, pickCookiesFile }: Props) {
  const [pickingBrowser, setPickingBrowser] = useState(false);
  const [showRaw, setShowRaw] = useState(false);

  async function handle(remedy: Remedy) {
    if (remedy.id === REMEDY.cookiesFromBrowser) {
      // Which browser is the one thing we can't guess, so ask rather than
      // picking wrong and failing again with the same message.
      setPickingBrowser((v) => !v);
      return;
    }
    if (remedy.id === REMEDY.importCookiesFile) {
      const file = await pickCookiesFile();
      if (file) onRemedy(remedy, file);
      return;
    }
    onRemedy(remedy);
  }

  const site = (() => {
    try {
      return new URL(url).hostname.replace(/^www\./, "");
    } catch {
      return null;
    }
  })();

  return (
    <div className="banner error failure">
      <span className="banner-icon">⚠</span>
      <div className="failure-body">
        <div className="banner-title">{failure.summary}</div>

        {failure.remedies.length > 0 && (
          <div className="failure-actions">
            {failure.remedies.map((r) => (
              <button key={r.id} className="act-btn act-btn-text" onClick={() => handle(r)}>
                {r.label}
              </button>
            ))}
          </div>
        )}

        {pickingBrowser && (
          <div className="failure-choices">
            <span className="failure-choices-label">
              Sign in to {site ?? "the site"} in which browser?
            </span>
            <div className="failure-actions">
              {BROWSERS.map((b) => (
                <button
                  key={b}
                  className="act-btn act-btn-text"
                  onClick={() => {
                    setPickingBrowser(false);
                    onRemedy(
                      { id: REMEDY.cookiesFromBrowser, label: `Use ${b} cookies` },
                      b,
                    );
                  }}
                >
                  {b[0].toUpperCase() + b.slice(1)}
                </button>
              ))}
            </div>
          </div>
        )}

        {failure.raw && failure.raw !== failure.summary && (
          <div className="failure-raw">
            <button className="link-btn" onClick={() => setShowRaw((v) => !v)}>
              {showRaw ? "Hide details" : "Show what yt-dlp said"}
            </button>
            {showRaw && <pre className="failure-raw-text">{failure.raw}</pre>}
          </div>
        )}
      </div>
    </div>
  );
}
