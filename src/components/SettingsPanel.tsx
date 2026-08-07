import { useEffect, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { Settings } from "../types";
import { FORMAT_PRESETS } from "../types";

interface Props {
  settings: Settings;
  onSave: (next: Settings) => void;
  saved: boolean;
  error: string | null;
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

function Toggle({ on, onClick }: { on: boolean; onClick: () => void }) {
  return (
    <div
      className="toggle-wrap"
      role="switch"
      aria-checked={on}
      tabIndex={0}
      onClick={onClick}
      onKeyDown={(e) => {
        if (e.key === " " || e.key === "Enter") { e.preventDefault(); onClick(); }
      }}
    >
      <div className={`toggle-track ${on ? "on" : ""}`}>
        <div className="toggle-thumb" />
      </div>
      <span className="toggle-label">{on ? "On" : "Off"}</span>
    </div>
  );
}

export default function SettingsPanel({ settings, onSave, saved, error }: Props) {
  // Edits live in a local draft so nothing takes effect until Save is pressed.
  // Previously every toggle mutated the settings the next download would use,
  // which made the Save button meaningless.
  const [draft, setDraft] = useState<Settings>(settings);

  useEffect(() => { setDraft(settings); }, [settings]);

  const dirty = JSON.stringify(draft) !== JSON.stringify(settings);

  function set<K extends keyof Settings>(key: K, value: Settings[K]) {
    setDraft((d) => ({ ...d, [key]: value }));
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
          <div className="path-val" title={draft.default_save_folder}>
            {draft.default_save_folder || "Not set — uses system Downloads"}
          </div>
          <button className="btn-icon" onClick={pickFolder} title="Choose folder">⌘</button>
          {draft.default_save_folder && (
            <button
              className="btn-icon"
              onClick={() => set("default_save_folder", "")}
              title="Reset to system Downloads"
            >
              ✕
            </button>
          )}
        </div>
      </Row>

      <Row label="Default format">
        <select
          className="select"
          value={draft.default_format}
          onChange={(e) => set("default_format", e.target.value)}
        >
          {FORMAT_PRESETS.map((p) => (
            <option key={p.id} value={p.id}>{p.label} — {p.detail}</option>
          ))}
        </select>
      </Row>

      {/* ── yt-dlp Behaviour ──────────────────────────────────────────────── */}
      <SectionHead title="yt-dlp Behaviour" />

      <Row label="Embed thumbnail" hint="Adds cover art to MP3 and MP4 — needs ffmpeg">
        <Toggle on={draft.embed_thumbnail} onClick={() => set("embed_thumbnail", !draft.embed_thumbnail)} />
      </Row>

      <Row label="Embed subtitles" hint="Auto-downloads and embeds English subs">
        <Toggle on={draft.embed_subtitles} onClick={() => set("embed_subtitles", !draft.embed_subtitles)} />
      </Row>

      <Row label="Speed limit" hint="e.g. 2M, 500K — leave empty for unlimited">
        <input
          className="url-input"
          style={{ height: "40px", padding: "0 12px", fontSize: "13px" }}
          type="text"
          placeholder="Unlimited"
          value={draft.speed_limit}
          onChange={(e) => set("speed_limit", e.target.value)}
          spellCheck={false}
        />
      </Row>

      <Row label="Cookies from browser" hint="Lets yt-dlp access age-restricted or members-only content">
        <select
          className="select"
          value={draft.cookies_browser}
          onChange={(e) => set("cookies_browser", e.target.value)}
        >
          <option value="">Off</option>
          <option value="chrome">Chrome</option>
          <option value="chromium">Chromium</option>
          <option value="firefox">Firefox</option>
          <option value="safari">Safari</option>
          <option value="brave">Brave</option>
          <option value="edge">Edge</option>
          <option value="opera">Opera</option>
          <option value="vivaldi">Vivaldi</option>
        </select>
      </Row>

      {/* ── App Behaviour ─────────────────────────────────────────────────── */}
      <SectionHead title="App Behaviour" />

      <Row label="Auto-open folder on complete" hint="Reveals the file in your file manager when a download finishes">
        <Toggle on={draft.auto_open_folder} onClick={() => set("auto_open_folder", !draft.auto_open_folder)} />
      </Row>

      <Row label="Auto-delete history" hint="Removes entries older than selected period, applied at launch">
        <select
          className="select"
          value={String(draft.auto_delete_history_days)}
          onChange={(e) => set("auto_delete_history_days", Number(e.target.value))}
        >
          <option value="0">Never</option>
          <option value="7">After 7 days</option>
          <option value="30">After 30 days</option>
          <option value="90">After 90 days</option>
        </select>
      </Row>

      {/* ── Save ──────────────────────────────────────────────────────────── */}
      {error && (
        <div className="banner error">
          <span className="banner-icon">⚠</span>
          <span>{error}</span>
        </div>
      )}

      <div className="settings-footer">
        {dirty && <span className="settings-hint">Unsaved changes</span>}
        <button className="btn-download" onClick={() => onSave(draft)} disabled={!dirty && !saved}>
          {saved && !dirty ? "✓ Saved" : "Save Settings"}
        </button>
      </div>

    </div>
  );
}
