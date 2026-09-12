/** Staff sessions excluded. */

import { trackEvent } from './siteAnalytics'

export const AnalyticsEvents = {
  LOGIN_SUCCESS: 'login_success',
  LOGIN_OAUTH_CLICK: 'login_oauth_click',
  LOGIN_OAUTH_SUCCESS: 'login_oauth_success',
  REGISTER_SUCCESS: 'register_success',
  LOGOUT: 'logout',

  MUSIC_PLAY: 'music_play',
  MUSIC_PAUSE: 'music_pause',
  MUSIC_NEXT: 'music_next',
  MUSIC_PREV: 'music_prev',
  MUSIC_SOURCE_SWITCH: 'music_source_switch',
  MUSIC_LIBRARY_PLAY: 'music_library_play',

  LIBRARY_FILTER: 'library_filter',

  BREW_OPEN_SOURCE: 'brew_open_source',
  BREW_OPEN_ITEM: 'brew_open_item',
  BREW_STAR: 'brew_star',
  BREW_UNSTAR: 'brew_unstar',

  REPORT_STAGE_OPEN: 'report_stage_open',
  REPORT_PLAY_ALL: 'report_play_all',

  AGENT_OPEN: 'agent_open',
  AGENT_SEND: 'agent_send',
  AGENT_VOICE: 'agent_voice',

  TAPP_OPEN_DETAIL: 'tapp_open_detail',
  TAPP_RUN: 'tapp_run',
  TAPP_PLAYGROUND: 'tapp_playground',

  FRIEND_LINK_CLICK: 'friend_link_click',
  FRIEND_LINKS_BREW: 'friend_links_brew',

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
  target?: string | number | null
  flush?: boolean
  /** Dedupe window (ms). */
  throttleMs?: number
}

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
