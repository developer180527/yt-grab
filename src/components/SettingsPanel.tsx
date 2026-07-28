import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { Settings } from "../types";
import { FORMAT_PRESETS } from "../types";

interface Props {
  settings: Settings;
  onChange: (s: Settings) => void;
  onSave: () => void;
  saved: boolean;
}

function Row({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="settings-row">
      <div className="settings-row-label">
        <span>{label}</span>
        {hint && <span className="settings-hint">{hint}</span>}
      </div>
      <div className="settings-row-control">{children}</div>
    </div>
  );
}

function SectionHead({ title }: { title: string }) {
  return <div className="settings-section">{title}</div>;
}

export default function SettingsPanel({ settings, onChange, onSave, saved }: Props) {
  function set<K extends keyof Settings>(key: K, value: Settings[K]) {
    onChange({ ...settings, [key]: value });
  }

  async function pickFolder() {
    const selected = await openDialog({ directory: true, multiple: false });
    if (typeof selected === "string") set("default_save_folder", selected);
  }

  return (
    <div className="settings-wrap">

      {/* ── Download Defaults ─────────────────────────────────────────────── */}
      <SectionHead title="Download Defaults" />

      <Row label="Default save folder" hint="Used when app starts">
        <div className="path-row">
          <div className="path-val" title={settings.default_save_folder}>
            {settings.default_save_folder || "Not set — uses system Downloads"}
          </div>
          <button className="btn-icon" onClick={pickFolder} title="Choose folder">⌘</button>
        </div>
      </Row>

      <Row label="Default format">
        <select
          className="select"
          value={settings.default_format}
          onChange={(e) => set("default_format", e.target.value)}
        >
          {FORMAT_PRESETS.map((p) => (
            <option key={p.id} value={p.id}>{p.label} — {p.detail}</option>
          ))}
        </select>
      </Row>

      {/* ── yt-dlp Behaviour ──────────────────────────────────────────────── */}
      <SectionHead title="yt-dlp Behaviour" />

      <Row label="Embed thumbnail" hint="Adds cover art to MP3 and MP4">
        <div
          className="toggle-wrap"
          onClick={() => set("embed_thumbnail", !settings.embed_thumbnail)}
        >
          <div className={`toggle-track ${settings.embed_thumbnail ? "on" : ""}`}>
            <div className="toggle-thumb" />
          </div>
          <span className="toggle-label">{settings.embed_thumbnail ? "On" : "Off"}</span>
        </div>
      </Row>

      <Row label="Embed subtitles" hint="Auto-downloads and embeds English subs">
        <div
          className="toggle-wrap"
          onClick={() => set("embed_subtitles", !settings.embed_subtitles)}
        >
          <div className={`toggle-track ${settings.embed_subtitles ? "on" : ""}`}>
            <div className="toggle-thumb" />
          </div>
          <span className="toggle-label">{settings.embed_subtitles ? "On" : "Off"}</span>
        </div>
      </Row>

      <Row label="Speed limit" hint="e.g. 2M, 500K — leave empty for unlimited">
        <input
          className="url-input"
          style={{ height: "40px", padding: "0 12px", fontSize: "13px" }}
          type="text"
          placeholder="Unlimited"
          value={settings.speed_limit}
          onChange={(e) => set("speed_limit", e.target.value)}
          spellCheck={false}
        />
      </Row>

      <Row label="Cookies from browser" hint="Lets yt-dlp access age-restricted or members-only content">
        <select
          className="select"
          value={settings.cookies_browser}
          onChange={(e) => set("cookies_browser", e.target.value)}
        >
          <option value="">Off</option>
          <option value="chrome">Chrome</option>
          <option value="firefox">Firefox</option>
          <option value="safari">Safari</option>
          <option value="brave">Brave</option>
          <option value="edge">Edge</option>
        </select>
      </Row>

      {/* ── App Behaviour ─────────────────────────────────────────────────── */}
      <SectionHead title="App Behaviour" />

      <Row label="Auto-open folder on complete" hint="Reveals file in Finder when download finishes">
        <div
          className="toggle-wrap"
          onClick={() => set("auto_open_folder", !settings.auto_open_folder)}
        >
          <div className={`toggle-track ${settings.auto_open_folder ? "on" : ""}`}>
            <div className="toggle-thumb" />
          </div>
          <span className="toggle-label">{settings.auto_open_folder ? "On" : "Off"}</span>
        </div>
      </Row>

      <Row label="Clear queue on launch" hint="Removes all items from queue when app starts">
        <div
          className="toggle-wrap"
          onClick={() => set("clear_queue_on_launch", !settings.clear_queue_on_launch)}
        >
          <div className={`toggle-track ${settings.clear_queue_on_launch ? "on" : ""}`}>
            <div className="toggle-thumb" />
          </div>
          <span className="toggle-label">{settings.clear_queue_on_launch ? "On" : "Off"}</span>
        </div>
      </Row>

      <Row label="Auto-delete history" hint="Removes entries older than selected period">
        <select
          className="select"
          value={String(settings.auto_delete_history_days)}
          onChange={(e) => set("auto_delete_history_days", Number(e.target.value))}
        >
          <option value="0">Never</option>
          <option value="7">After 7 days</option>
          <option value="30">After 30 days</option>
          <option value="90">After 90 days</option>
        </select>
      </Row>

      {/* ── Save ──────────────────────────────────────────────────────────── */}
      <div className="settings-footer">
        <button className="btn-download" onClick={onSave}>
          {saved ? "✓ Saved" : "Save Settings"}
        </button>
      </div>

    </div>
  );
}
