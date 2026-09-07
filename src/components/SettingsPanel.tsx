import { useEffect, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  AUDIO_FORMATS, BROWSERS, FORMAT_PRESETS,
  type Settings, type SiteRule, type ToolStatus,
} from "../types";

interface Props {
  settings: Settings;
  onSave: (next: Settings) => void;
  saved: boolean;
  error: string | null;
  rules: SiteRule[];
  onDeleteRule: (domain: string) => void;
  tools: ToolStatus | null;
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

/** One-line description of what a rule actually does, for the rules list. */
function describeRule(r: SiteRule): string {
  const parts: string[] = [];
  if (r.cookies_browser) parts.push(`${r.cookies_browser} cookies`);
  if (r.cookies_file) parts.push("cookies.txt");
  if (r.format_id) parts.push(r.format_id);
  if (r.audio_only) parts.push("audio only");
  if (r.output_dir) parts.push(`→ ${r.output_dir}`);
  return parts.length ? parts.join(" · ") : "no overrides";
}

export default function SettingsPanel({
  settings, onSave, saved, error, rules, onDeleteRule, tools,
}: Props) {
  // Edits live in a local draft so nothing takes effect until Save is pressed.
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

      <SectionHead title="Download defaults" />

      <Row label="Default save folder" hint="Used when the app starts">
        <div className="path-row">
          <div className="path-val" title={draft.default_save_folder}>
            {draft.default_save_folder || "Not set — uses system Downloads"}
          </div>
          <button className="btn-icon" onClick={pickFolder} title="Choose folder">⌘</button>
          {draft.default_save_folder && (
            <button className="btn-icon" onClick={() => set("default_save_folder", "")} title="Reset">✕</button>
          )}
        </div>
      </Row>

      <Row label="Default quality">
        <select className="select" value={draft.default_format}
                onChange={(e) => set("default_format", e.target.value)}>
          {FORMAT_PRESETS.map((p) => (
            <option key={p.id} value={p.id}>{p.label} — {p.detail}</option>
          ))}
        </select>
      </Row>

      <Row label="Audio format" hint="Original keeps the source codec — converting always loses a little quality">
        <select className="select" value={draft.audio_format}
                onChange={(e) => set("audio_format", e.target.value)}>
          {AUDIO_FORMATS.map((a) => (
            <option key={a.id} value={a.id}>{a.label} — {a.detail}</option>
          ))}
        </select>
      </Row>

      <Row label="Prefer MP4" hint="Remux video into MP4 for maximum compatibility. Off keeps whatever the site serves">
        <Toggle on={draft.prefer_mp4} onClick={() => set("prefer_mp4", !draft.prefer_mp4)} />
      </Row>

      <SectionHead title="yt-dlp behaviour" />

      <Row label="Embed thumbnail" hint="Adds cover art — needs ffmpeg">
        <Toggle on={draft.embed_thumbnail} onClick={() => set("embed_thumbnail", !draft.embed_thumbnail)} />
      </Row>

      <Row label="Embed subtitles" hint="Downloads and embeds subtitles when the site has them">
        <Toggle on={draft.embed_subtitles} onClick={() => set("embed_subtitles", !draft.embed_subtitles)} />
      </Row>

      {draft.embed_subtitles && (
        <Row label="Subtitle languages" hint="yt-dlp codes, comma separated — e.g. en,es or all">
          <input
            className="url-input"
            style={{ height: "40px", padding: "0 12px", fontSize: "13px" }}
            type="text"
            placeholder="en"
            value={draft.subtitle_langs}
            onChange={(e) => set("subtitle_langs", e.target.value)}
            spellCheck={false}
          />
        </Row>
      )}

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

      <Row label="Cookies from browser" hint="Fallback for sites without their own rule below">
        <select className="select" value={draft.cookies_browser}
                onChange={(e) => set("cookies_browser", e.target.value)}>
          <option value="">Off</option>
          {BROWSERS.map((b) => (
            <option key={b} value={b}>{b[0].toUpperCase() + b.slice(1)}</option>
          ))}
        </select>
      </Row>

      <SectionHead title="App behaviour" />

      <Row label="Auto-open folder on complete" hint="Reveals the file when a download finishes">
        <Toggle on={draft.auto_open_folder} onClick={() => set("auto_open_folder", !draft.auto_open_folder)} />
      </Row>

      <Row label="Simultaneous downloads" hint="Extra downloads wait in the queue. Higher isn't always faster — sites throttle, and each merge costs CPU">
        <select className="select" value={String(draft.max_concurrent)}
                onChange={(e) => set("max_concurrent", Number(e.target.value))}>
          <option value="1">1 — one at a time</option>
          <option value="2">2</option>
          <option value="3">3 (recommended)</option>
          <option value="4">4</option>
          <option value="5">5</option>
        </select>
      </Row>

      <Row label="Auto-delete history" hint="Removes entries older than the selected period, applied at launch">
        <select className="select" value={String(draft.auto_delete_history_days)}
                onChange={(e) => set("auto_delete_history_days", Number(e.target.value))}>
          <option value="0">Never</option>
          <option value="7">After 7 days</option>
          <option value="30">After 30 days</option>
          <option value="90">After 90 days</option>
        </select>
      </Row>

      {error && (
        <div className="banner error">
          <span className="banner-icon">⚠</span>
          <span>{error}</span>
        </div>
      )}

      <div className="settings-footer">
        {dirty && <span className="settings-hint">Unsaved changes</span>}
        <button className="btn-download" onClick={() => onSave(draft)} disabled={!dirty && !saved}>
          {saved && !dirty ? "✓ Saved" : "Save settings"}
        </button>
      </div>

      {/* ── Per-site rules ───────────────────────────────────────────────── */}
      <SectionHead title="Site rules" />
      {rules.length === 0 ? (
        <div className="settings-note">
          Nothing yet. When a download fails because a site needs a login, accepting
          the fix saves a rule here so you're only asked once.
        </div>
      ) : (
        <div className="rules-list">
          {rules.map((r) => (
            <div key={r.domain} className="rule-row">
              <div className="rule-domain">{r.domain}</div>
              <div className="rule-detail">{describeRule(r)}</div>
              <button className="act-btn danger" onClick={() => onDeleteRule(r.domain)} title="Forget this rule">✕</button>
            </div>
          ))}
        </div>
      )}

      {/* ── Environment ──────────────────────────────────────────────────── */}
      <SectionHead title="Tools" />
      <div className="settings-note">
        <div>
          <strong>yt-dlp</strong>{" "}
          {tools?.ytdlp.available
            ? <>{tools.ytdlp.version} · {tools.ytdlp.source}
                {tools.ytdlp.age_days !== null && <> · {tools.ytdlp.age_days} days old</>}</>
            : "not available"}
        </div>
        <div>
          <strong>ffmpeg</strong>{" "}
          {tools?.ffmpeg.available
            ? tools.ffmpeg.version
            : "not found — needed to combine video with audio and to convert audio"}
        </div>
      </div>

    </div>
  );
}
