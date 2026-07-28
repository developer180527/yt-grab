interface Props {
  version: string | null;
  missing: boolean;
}

export default function Header({ version, missing }: Props) {
  const dotCls = missing ? "dot red" : version ? "dot" : "dot amber";

  return (
    <header className="header">
      <div className="titlebar" data-tauri-drag-region />
      <div className="titlebar-content">
        <div className="header-badge">
          <span className={dotCls} />
          <span>
            {missing ? "yt-dlp missing" : version ? `yt-dlp ${version}` : "checking…"}
          </span>
        </div>
      </div>
    </header>
  );
}