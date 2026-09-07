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
  /** Creates or updates a rule. Rejects with a message the editor shows inline. */
  onSaveRule: (rule: SiteRule, isNew: boolean) => Promise<void>;
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
  // A rule that says "video" is not the same as one with no opinion — only the
  // first overrides a global audio-only default, so it has to be visible here.
  if (r.audio_only === true) parts.push("audio only");
  else if (r.audio_only === false) parts.push("video");
  if (r.output_dir) parts.push(`→ ${r.output_dir}`);
  return parts.length ? parts.join(" · ") : "no overrides";
}

/** Sentinel for "the rule has no opinion here" — distinct from any real value. */
const NO_PREFERENCE = "\u0000none";

const EMPTY_RULE: SiteRule = {
  domain: "",
  cookies_browser: null,
  cookies_file: null,
  output_dir: null,
  format_id: null,
  audio_only: null,
};

/**
 * Creates or edits one site rule.
 *
 * Every override is optional, and absent means "no opinion" — the global
 * default applies. That distinction is the entire point of a rule, so each
 * control offers it explicitly instead of collapsing it into an empty value:
 * a rule saying "video" is not the same as a rule that says nothing about
 * audio, and only the first one overrides a global audio-only default.
 */
function RuleEditor({
  initial, onSave, onCancel,
}: {
  /** The rule to edit, or null to create one. */
  initial: SiteRule | null;
  onSave: (rule: SiteRule, isNew: boolean) => Promise<void>;
  onCancel: () => void;
}) {
  const [draft, setDraft] = useState<SiteRule>(initial ?? EMPTY_RULE);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const isNew = initial === null;

  function set<K extends keyof SiteRule>(key: K, value: SiteRule[K]) {
    setDraft((d) => ({ ...d, [key]: value }));
  }

  // The two cookie sources are mutually exclusive because yt-dlp is given only
  // one, and a file always wins. Keeping both would advertise a browser setting
  // that never takes effect.
  function setBrowser(value: string) {
    setDraft((d) => ({
      ...d,
      cookies_browser: value === NO_PREFERENCE ? null : value,
      cookies_file: value === NO_PREFERENCE ? d.cookies_file : null,
    }));
  }

  async function pickCookiesFile() {
    const selected = await openDialog({
      multiple: false,
      filters: [{ name: "Cookies", extensions: ["txt"] }],
    });
    if (typeof selected === "string") {
      setDraft((d) => ({ ...d, cookies_file: selected, cookies_browser: null }));
    }
  }

  async function pickRuleFolder() {
    const selected = await openDialog({ directory: true, multiple: false });
    if (typeof selected === "string") set("output_dir", selected);
  }

  async function save() {
    setBusy(true);
    setErr(null);
    try {
      await onSave(draft, isNew);
    } catch (e) {
      // Stay open with the message: closing would lose everything typed.
      setErr(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  }

  return (
    <div className="rule-editor">
      <Row label="Site" hint={isNew ? "A domain, or a link from the site" : "A rule is keyed by site and can't be renamed"}>
        {isNew ? (
          <input
            className="url-input"
            style={{ height: "40px", padding: "0 12px", fontSize: "13px" }}
            type="text"
            placeholder="youtube.com"
            value={draft.domain}
            onChange={(e) => set("domain", e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter" && draft.domain.trim()) void save(); }}
            spellCheck={false}
            autoComplete="off"
            autoCapitalize="off"
            autoFocus
          />
        ) : (
          <div className="static-field">{draft.domain}</div>
        )}
      </Row>

      <Row label="Save to" hint="Where downloads from this site land">
        <div className="path-row">
          <div className="path-val" title={draft.output_dir ?? ""}>
            {draft.output_dir || "No preference — uses the default folder"}
          </div>
          <button className="btn-icon" onClick={pickRuleFolder} title="Choose folder">⌘</button>
          {draft.output_dir && (
            <button className="btn-icon" onClick={() => set("output_dir", null)} title="Clear">✕</button>
          )}
        </div>
      </Row>

      <Row label="Quality" hint="What the panel opens on for this site">
        <select
          className="select"
          value={draft.format_id ?? NO_PREFERENCE}
          onChange={(e) => set("format_id", e.target.value === NO_PREFERENCE ? null : e.target.value)}
        >
          <option value={NO_PREFERENCE}>No preference</option>
          {FORMAT_PRESETS.map((p) => (
            <option key={p.id} value={p.id}>{p.label} — {p.detail}</option>
          ))}
        </select>
      </Row>

      <Row label="Audio only" hint="Bandcamp wants audio; the same default would be wrong for YouTube">
        <select
          className="select"
          value={draft.audio_only === null ? NO_PREFERENCE : String(draft.audio_only)}
          onChange={(e) =>
            set("audio_only", e.target.value === NO_PREFERENCE ? null : e.target.value === "true")}
        >
          <option value={NO_PREFERENCE}>No preference</option>
          <option value="true">Audio only</option>
          <option value="false">Video</option>
        </select>
      </Row>

      <Row label="Cookies from browser" hint="Overrides the global browser for this site">
        <select
          className="select"
          value={draft.cookies_browser ?? NO_PREFERENCE}
          onChange={(e) => setBrowser(e.target.value)}
        >
          <option value={NO_PREFERENCE}>No preference</option>
          {BROWSERS.map((b) => (
            <option key={b} value={b}>{b[0].toUpperCase() + b.slice(1)}</option>
          ))}
        </select>
      </Row>

      <Row label="Cookies file" hint="An exported cookies.txt — more reliable than reading the browser, and takes precedence">
        <div className="path-row">
          <div className="path-val" title={draft.cookies_file ?? ""}>
            {draft.cookies_file || "None"}
          </div>
          <button className="btn-icon" onClick={pickCookiesFile} title="Choose cookies.txt">⌘</button>
          {draft.cookies_file && (
            <button className="btn-icon" onClick={() => set("cookies_file", null)} title="Clear">✕</button>
          )}
        </div>
      </Row>

      {err && (
        <div className="banner error">
          <span className="banner-icon">⚠</span>
          <span>{err}</span>
        </div>
      )}

      <div className="rule-editor-actions">
        <button className="act-btn act-btn-text" onClick={onCancel} disabled={busy}>Cancel</button>
        <button
          className="btn-download"
          onClick={() => void save()}
          disabled={busy || !draft.domain.trim()}
          title={draft.domain.trim() ? undefined : "Name the site first"}
        >
          {busy ? "Saving…" : isNew ? "Add rule" : "Save rule"}
        </button>
      </div>
    </div>
  );
}

export default function SettingsPanel({
  settings, onSave, saved, error, rules, onSaveRule, onDeleteRule, tools,
}: Props) {
  // Which rule the editor is open on: a domain, "" for a new one, or null for
  // closed. Keyed by domain rather than index so the list can reorder freely.
  const [editing, setEditing] = useState<string | null>(null);
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
      {rules.length === 0 && editing === null ? (
        <div className="settings-note">
          Nothing yet. When a download fails because a site needs a login, accepting
          the fix saves a rule here so you're only asked once — or add one below to
          send a site's downloads to their own folder, quality or audio setting.
        </div>
      ) : (
        <div className="rules-list">
          {rules.map((r) => (
            editing === r.domain ? (
              <RuleEditor
                key={r.domain}
                initial={r}
                onSave={async (rule, isNew) => { await onSaveRule(rule, isNew); setEditing(null); }}
                onCancel={() => setEditing(null)}
              />
            ) : (
              <div key={r.domain} className="rule-row">
                <div className="rule-domain">{r.domain}</div>
                <div className="rule-detail">{describeRule(r)}</div>
                <button
                  className="act-btn"
                  onClick={() => setEditing(r.domain)}
                  title="Edit this rule"
                  disabled={editing !== null}
                >
                  ✎
                </button>
                <button
                  className="act-btn danger"
                  onClick={() => onDeleteRule(r.domain)}
                  title="Forget this rule"
                  disabled={editing !== null}
                >
                  ✕
                </button>
              </div>
            )
          ))}
        </div>
      )}

      {editing === "" ? (
        <RuleEditor
          initial={null}
          onSave={async (rule, isNew) => { await onSaveRule(rule, isNew); setEditing(null); }}
          onCancel={() => setEditing(null)}
        />
      ) : (
        <div className="rule-add">
          <button
            className="act-btn act-btn-text"
            onClick={() => setEditing("")}
            disabled={editing !== null}
          >
            + Add a site rule
          </button>
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
