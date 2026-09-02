/**
 * First-party product analytics event catalog.
 *
 * Names must satisfy siteAnalytics rules: 2–48 chars, `[a-z0-9][a-z0-9_-]*`.
 * Staff (admin/owner) sessions are excluded by siteAnalytics + server JWT.
 *
 * Prefer {@link trackProductEvent} over raw `trackEvent` so call sites share
 * names, optional throttle, and one import path.
 */

import { trackEvent } from './siteAnalytics'

/** Canonical event names used across the app. */
export const AnalyticsEvents = {
  // ── Auth ──────────────────────────────────────────────────────────────
  LOGIN_SUCCESS: 'login_success',
  LOGIN_OAUTH_CLICK: 'login_oauth_click',
  LOGIN_OAUTH_SUCCESS: 'login_oauth_success',
  REGISTER_SUCCESS: 'register_success',
  LOGOUT: 'logout',

  // ── Music ─────────────────────────────────────────────────────────────
  MUSIC_PLAY: 'music_play',
  MUSIC_PAUSE: 'music_pause',
  MUSIC_NEXT: 'music_next',
  MUSIC_PREV: 'music_prev',
  MUSIC_SOURCE_SWITCH: 'music_source_switch',
  MUSIC_LIBRARY_PLAY: 'music_library_play',

  // ── Library ───────────────────────────────────────────────────────────
  LIBRARY_FILTER: 'library_filter',

  // ── Brew ──────────────────────────────────────────────────────────────
  BREW_OPEN_SOURCE: 'brew_open_source',
  BREW_OPEN_ITEM: 'brew_open_item',
  BREW_STAR: 'brew_star',
  BREW_UNSTAR: 'brew_unstar',

  // ── Reports ───────────────────────────────────────────────────────────
  REPORT_STAGE_OPEN: 'report_stage_open',
  REPORT_PLAY_ALL: 'report_play_all',

  // ── Agent (Arael) ─────────────────────────────────────────────────────
  AGENT_OPEN: 'agent_open',
  AGENT_SEND: 'agent_send',
  AGENT_VOICE: 'agent_voice',

  // ── Tapp ──────────────────────────────────────────────────────────────
  TAPP_OPEN_DETAIL: 'tapp_open_detail',
  TAPP_RUN: 'tapp_run',
  TAPP_PLAYGROUND: 'tapp_playground',

  // ── Home / social ─────────────────────────────────────────────────────
  FRIEND_LINK_CLICK: 'friend_link_click',
  FRIEND_LINKS_BREW: 'friend_links_brew',

  // ── UX chrome ─────────────────────────────────────────────────────────
  THEME_SWITCH: 'theme_switch',
  LOCALE_SWITCH: 'locale_switch',
  CONTROL_PANEL_OPEN: 'control_panel_open',
  USER_MODAL_OPEN: 'user_modal_open',
  NOTIFICATION_OPEN: 'notification_open',
} as const

export type AnalyticsEventName =
  (typeof AnalyticsEvents)[keyof typeof AnalyticsEvents]

const lastFired = new Map<string, number>()

export interface TrackProductEventOpts {
  path?: string
  /**
   * Entity dimension for drill-down: tapp id, platform slug, brew source id,
   * music source, filter key, locale, theme, host, … Server normalizes.
   */
  target?: string | number | null
  /** Kick an immediate network flush (login/register hard navigations). */
  flush?: boolean
  /**
   * Drop repeats of the same event+target within this window (ms).
   * Use for high-churn actions (theme toggle spam, play/pause thrash).
   */
  throttleMs?: number
}

/**
 * Fire a catalogued product event (or a custom snake_case name).
 * Respects opt-out / staff / site collection switches via siteAnalytics.
 */
export function trackProductEvent(
  name: AnalyticsEventName | string,
  opts?: TrackProductEventOpts,
): void {
  const n = String(name || '').trim().toLowerCase()
  if (!n) return

  const targetRaw =
    opts?.target == null || opts.target === ''
      ? ''
      : String(opts.target).trim().toLowerCase()
  const throttleKey = targetRaw ? `${n}|${targetRaw}` : n

  const throttleMs = opts?.throttleMs
  if (throttleMs && throttleMs > 0) {
    const last = lastFired.get(throttleKey) ?? 0
    const now = Date.now()
    if (now - last < throttleMs) return
    lastFired.set(throttleKey, now)
  }

  trackEvent(n, {
    path: opts?.path,
    flush: opts?.flush,
    target: targetRaw || undefined,
  })
}
