import type { KeyboardEvent } from "react";

interface Props {
  url: string;
  setUrl: (v: string) => void;
  onFetch: () => void;
  fetching: boolean;
}

export default function URLBar({ url, setUrl, onFetch, fetching }: Props) {
  function handleKey(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Enter") onFetch();
  }

  return (
    <div className="url-bar">
      <div className="url-input-wrap">
        <span className="url-input-icon">$</span>
        <input
          className="url-input"
          type="url"
          placeholder="Paste a URL — YouTube, SoundCloud, Vimeo, and 1000+ more"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          onKeyDown={handleKey}
          spellCheck={false}
          autoComplete="off"
          autoCorrect="off"
          autoCapitalize="off"
        />
      </div>
      <button
        className="btn-primary"
        onClick={onFetch}
        disabled={fetching || !url.trim()}
      >
        {fetching ? (
          <>
            <span className="spinner" />
            Fetching
          </>
        ) : (
          "Fetch"
        )}
      </button>
    </div>
  );
}
