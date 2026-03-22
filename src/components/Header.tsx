interface Props {
  version: string | null;
  missing: boolean;
}

export default function Header({ version, missing }: Props) {
  const dotCls = missing ? "dot red" : version ? "dot" : "dot amber";

  return (
    <header className="header">
      <span className="header-logo">FETCH</span>

      <div className="header-badge">
        <span className={dotCls} />
        <span>
          {missing ? "yt-dlp missing" : version ? `yt-dlp ${version}` : "checking…"}
        </span>
      </div>

      <span style={{ marginLeft: "auto", fontSize: "11px", color: "var(--text-dim)", letterSpacing: "0.05em" }}>
        Media Downloader
      </span>
    </header>
  );
}
