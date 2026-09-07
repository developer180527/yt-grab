//! The only place the UI talks to the backend.
//!
//! Every component imports from here rather than calling `invoke` directly, so
//! the set of operations the app supports is written down in one file. A UI
//! redesign rewrites components against this module and keeps every capability;
//! a backend change surfaces here as a type error rather than as a runtime
//! failure in whichever component happened to use the old name.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  CompletePayload, DeepLinkPayload, Failure, FailurePayload, GrabRequest,
  HistoryItem, MediaInfo, ProgressPayload, Settings, SiteRule, StartedPayload,
  ToolStatus,
} from "./types";

// ─── Environment ──────────────────────────────────────────────────────────────

export const toolStatus = () => invoke<ToolStatus>("tool_status");

// ─── Resolving ────────────────────────────────────────────────────────────────

/** Looks up what is at a URL, applying the site's cookie rule. */
export const resolveUrl = (url: string) => invoke<MediaInfo>("resolve_url", { url });

/** Classifies an error string so a failed resolve gets the same remedy
 *  buttons as a failed download. */
export const diagnoseError = (url: string, message: string) =>
  invoke<Failure>("diagnose_error", { url, message });

// ─── Grabbing ─────────────────────────────────────────────────────────────────

export const enqueueGrab = (request: GrabRequest) => invoke<void>("enqueue_grab", { request });
export const cancelGrab = (id: string) => invoke<void>("cancel_grab", { id });
/** `[running, waiting]`. */
export const queueStatus = () => invoke<[number, number]>("queue_status");

// ─── Remedies ─────────────────────────────────────────────────────────────────

/** Applies the persistent half of a remedy (cookies, concurrency) and returns
 *  the site rule it wrote, if any. Retry-shaped remedies need no backend call —
 *  the caller simply re-issues the grab. */
export const applyRemedy = (url: string, remedyId: string, value?: string) =>
  invoke<SiteRule | null>("apply_remedy", { url, remedyId, value: value ?? null });

// ─── Site rules ───────────────────────────────────────────────────────────────

export const listSiteRules = () => invoke<SiteRule[]>("list_site_rules");
export const setSiteRule = (rule: SiteRule) => invoke<void>("set_site_rule", { rule });
export const deleteSiteRule = (domain: string) => invoke<void>("delete_site_rule", { domain });
export const siteForUrl = (url: string) => invoke<string | null>("site_for_url", { url });

// ─── Files ────────────────────────────────────────────────────────────────────

export const openPath = (path: string) => invoke<void>("open_path", { path });

// ─── History ──────────────────────────────────────────────────────────────────

export const getHistory = () => invoke<HistoryItem[]>("get_history");
export const completedUrls = () => invoke<string[]>("completed_urls");
export const deleteHistoryItem = (id: number) => invoke<void>("delete_history_item", { id });
export const clearHistory = () => invoke<void>("clear_history");
export const purgeOldHistory = (days: number) => invoke<void>("purge_old_history", { days });

// ─── Settings ─────────────────────────────────────────────────────────────────

export const getSettings = () => invoke<Settings>("get_settings");
export const saveSettings = (settings: Settings) => invoke<void>("save_settings", { settings });

// ─── Events ───────────────────────────────────────────────────────────────────

export interface GrabEvents {
  onStarted: (p: StartedPayload) => void;
  onProgress: (p: ProgressPayload) => void;
  onComplete: (p: CompletePayload) => void;
  onFailure: (p: FailurePayload) => void;
  onDeepLink: (p: DeepLinkPayload) => void;
}

/**
 * Subscribes to every backend event at once and returns a single teardown.
 *
 * `listen` is async, so a caller that unsubscribes before it resolves would
 * otherwise leak a live listener; this disposes anything that arrives late.
 */
export function subscribe(handlers: Partial<GrabEvents>): () => void {
  let disposed = false;
  const unlisteners: UnlistenFn[] = [];

  const track = (p: Promise<UnlistenFn>) => {
    p.then((fn) => {
      if (disposed) fn();
      else unlisteners.push(fn);
    }).catch(console.error);
  };

  if (handlers.onStarted) track(listen<StartedPayload>("download:started", (e) => handlers.onStarted!(e.payload)));
  if (handlers.onProgress) track(listen<ProgressPayload>("download:progress", (e) => handlers.onProgress!(e.payload)));
  if (handlers.onComplete) track(listen<CompletePayload>("download:complete", (e) => handlers.onComplete!(e.payload)));
  if (handlers.onFailure) track(listen<FailurePayload>("download:error", (e) => handlers.onFailure!(e.payload)));
  if (handlers.onDeepLink) track(listen<DeepLinkPayload>("deeplink:add", (e) => handlers.onDeepLink!(e.payload)));

  return () => {
    disposed = true;
    unlisteners.forEach((fn) => fn());
  };
}
