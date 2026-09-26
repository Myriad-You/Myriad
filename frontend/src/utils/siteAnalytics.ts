import { API_URL } from '../config'
import { getUIConfigDeduped } from './requestDedup'

export const ANALYTICS_PAGEVIEW_FLUSHED_EVENT = 'myriad:analytics-pageview-flushed'

const VID_KEY = 'myriad_vid'
/** Keep in sync with is_valid_vid. */
const VID_PATTERN = /^[\w-]{16,64}$/
const OPT_OUT_KEY = 'myriad_analytics_optout'
const SESSION_LAST_KEY = 'myriad_pv_last'
const SESSION_PATH_TTL_MS = 2500
const SKIP_PREFIXES = ['/setup']

const MAX_QUEUE = 24
const FLUSH_INTERVAL_MS = 4000
const MAX_ENGAGEMENT_FLUSH_MS = 30 * 60 * 1000
const MIN_ENGAGEMENT_MS = 800
const ENGAGE_TICK_MS = 15000
/** Re-queue once, then drop. */
const MAX_FLUSH_RETRIES = 1

type CollectKind = 'pageview' | 'engagement' | 'event'

interface CollectItem {
  type: CollectKind
  path?: string
  referrer?: string
  ms?: number
  name?: string
  target?: string
}

let excludeStaffSelf = false
/** Fail open until loaded; server enforces. */
let siteCollectionEnabled: boolean | null = null
let siteCollectionLoad: Promise<boolean> | null = null
let queue: CollectItem[] = []
let flushTimer: ReturnType<typeof setTimeout> | null = null
let flushInFlight = false
let flushAgainAfter = false
let flushRetriesLeft = MAX_FLUSH_RETRIES

let engagePath: string | null = null
let engageAccumMs = 0
let engageVisibleSince: number | null = null
let engageTickTimer: ReturnType<typeof setInterval> | null = null
let listenersBound = false

function collectUrl(): string {
  const base = (API_URL || '').replaceAll(/\/$/g, '')
  return `${base}/api/analytics/collect`
}

function isBrowserBot(): boolean {
  if (typeof navigator === 'undefined') return true
  const nav = navigator as Navigator & { webdriver?: boolean }
  if (nav.webdriver) return true
  const ua = (navigator.userAgent || '').toLowerCase()
  if (!ua) return true
  return (
    ua.includes('bot') ||
    ua.includes('spider') ||
    ua.includes('crawler') ||
    ua.includes('headless')
  )
}

function prefersReducedData(): boolean {
  try {
    const c = (
      navigator as Navigator & {
        connection?: { saveData?: boolean }
      }
    ).connection
    if (c?.saveData) return true
  } catch {
    /* ignore */
  }
  return false
}

function isOffline(): boolean {
  try {
    return typeof navigator !== 'undefined' && navigator.onLine === false
  } catch {
    return false
  }
}

export function isAnalyticsOptedOut(): boolean {
  try {
    return localStorage.getItem(OPT_OUT_KEY) === '1'
  } catch {
    return false
  }
}

export function setAnalyticsOptOut(optOut: boolean) {
  try {
    if (optOut) localStorage.setItem(OPT_OUT_KEY, '1')
    else localStorage.removeItem(OPT_OUT_KEY)
  } catch {
    /* ignore */
  }
  if (optOut) haltCollection()
}

function ensureSiteCollectionFlag(): void {
  if (siteCollectionEnabled !== null || siteCollectionLoad) return
  siteCollectionLoad = getUIConfigDeduped()
    .then((data: { analytics_enabled?: boolean | string }) => {
      const raw = data?.analytics_enabled
      const enabled =
        raw === undefined || raw === null
          ? true
          : raw !== false && raw !== 'false' && raw !== '0'
      siteCollectionEnabled = enabled
      if (!enabled) haltCollection()
      return enabled
    })
    .catch(() => {
      // Fail open; server enforces.
      siteCollectionEnabled = true
      return true
    })
    .finally(() => {
      siteCollectionLoad = null
    })
}

function clearEngagementTimers() {
  engagePath = null
  engageAccumMs = 0
  engageVisibleSince = null
  if (engageTickTimer) {
    clearInterval(engageTickTimer)
    engageTickTimer = null
  }
}

function haltCollection() {
  queue = []
  flushAgainAfter = false
  if (flushTimer) {
    clearTimeout(flushTimer)
    flushTimer = null
  }
  clearEngagementTimers()
}

/** Staff must not inflate stats. */
export function setAnalyticsStaffSession(opts: {
  isAdmin?: boolean
  isOwner?: boolean
}) {
  const next = Boolean(opts.isAdmin || opts.isOwner)
  excludeStaffSelf = next
  if (next) haltCollection()
}

function baseAllowed(): boolean {
  if (typeof window === 'undefined') return false
  ensureSiteCollectionFlag()
  if (siteCollectionEnabled === false) return false
  if (excludeStaffSelf) return false
  if (isAnalyticsOptedOut()) return false
  if (isBrowserBot()) return false
  if (isOffline()) return false
  if (prefersReducedData()) return false
  if (
    typeof document !== 'undefined' &&
    (document.visibilityState as string) === 'prerender'
  ) {
    return false
  }
  return true
}

function pathAllowed(pathname: string): boolean {
  if (!pathname) return false
  return !SKIP_PREFIXES.some(
    (p) => pathname === p || pathname.startsWith(`${p}/`),
  )
}

export function peekVisitorId(): string | null {
  try {
    const existing = localStorage.getItem(VID_KEY)
    if (existing && VID_PATTERN.test(existing)) return existing
  } catch {
    /* ignore */
  }
  return null
}

export function getOrCreateVisitorId(): string {
  try {
    const existing = localStorage.getItem(VID_KEY)
    if (existing && VID_PATTERN.test(existing)) return existing
  } catch {
    /* ignore */
  }

  let id: string
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    id = crypto.randomUUID()
  } else {
    const bytes = new Uint8Array(16)
    if (typeof crypto !== 'undefined' && crypto.getRandomValues) {
      crypto.getRandomValues(bytes)
    } else {
      for (let i = 0; i < 16; i++) bytes[i] = Math.floor(Math.random() * 256)
    }
    id = Iterator.from(bytes).map((b) => b.toString(16).padStart(2, '0')).toArray().join('')
  }
  try {
    localStorage.setItem(VID_KEY, id)
  } catch {
    /* ignore */
  }
  return id
}

function referrerHost(): string | undefined {
  if (typeof document === 'undefined' || !document.referrer) return undefined
  try {
    const host = new URL(document.referrer).hostname
    if (typeof location !== 'undefined' && host === location.hostname) {
      return undefined
    }
    return host || undefined
  } catch {
    return undefined
  }
}

function sessionPathRecentlyTracked(path: string): boolean {
  try {
    const raw = sessionStorage.getItem(SESSION_LAST_KEY)
    if (!raw) return false
    const parsed = JSON.parse(raw) as { path?: string; t?: number }
    if (parsed.path !== path || typeof parsed.t !== 'number') return false
    return Date.now() - parsed.t < SESSION_PATH_TTL_MS
  } catch {
    return false
  }
}

function markSessionPath(path: string) {
  try {
    sessionStorage.setItem(
      SESSION_LAST_KEY,
      JSON.stringify({ path, t: Date.now() }),
    )
  } catch {
    /* ignore */
  }
}

function enqueue(item: CollectItem) {
  if (queue.length >= MAX_QUEUE) {
    const dropIdx = queue.findIndex((q) => q.type !== 'pageview')
    queue = queue.toSpliced(dropIdx >= 0 ? dropIdx : 0, 1)
  }
  queue.push(item)
  scheduleFlush()
}

function scheduleFlush(immediate = false) {
  if (flushInFlight) {
    if (immediate) flushAgainAfter = true
    return
  }
  if (immediate) {
    if (flushTimer) {
      clearTimeout(flushTimer)
      flushTimer = null
    }
    void flushQueue()
    return
  }
  if (flushTimer) return
  flushTimer = setTimeout(() => {
    flushTimer = null
    void flushQueue()
  }, FLUSH_INTERVAL_MS)
}

async function flushQueue() {
  if (flushInFlight || queue.length === 0) return
  if (!baseAllowed()) {
    queue = []
    flushAgainAfter = false
    return
  }

  flushInFlight = true
  flushAgainAfter = false
  const items = queue.slice(0, MAX_QUEUE)
  queue = queue.toSpliced(0, MAX_QUEUE)
  const body = JSON.stringify({
    vid: getOrCreateVisitorId(),
    items,
  })
  const url = collectUrl()

  // Prefer fetch; sendBeacon ignores 503.
  let ok: boolean
  try {
    const res = await fetch(url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body,
      credentials: 'same-origin',
      keepalive: true,
    })
    // 429/5xx: retry once.
    ok = res.ok || res.status === 204
    if (res.status === 429 || res.status >= 500) {
      ok = false
    }
  } catch {
    ok = false
    // Beacon on unload; do not claim flush success.
    if (typeof navigator !== 'undefined' && typeof navigator.sendBeacon === 'function') {
      try {
        navigator.sendBeacon(url, new Blob([body], { type: 'application/json' }))
      } catch {
        /* ignore */
      }
    }
  }

  if (!ok && flushRetriesLeft > 0) {
    flushRetriesLeft -= 1
    queue = items.concat(queue).slice(0, MAX_QUEUE)
  } else if (ok) {
    flushRetriesLeft = MAX_FLUSH_RETRIES
    if (items.some((i) => i.type === 'pageview')) {
      try {
        window.dispatchEvent(new Event(ANALYTICS_PAGEVIEW_FLUSHED_EVENT))
      } catch {
        /* ignore */
      }
    }
  }
  // Drop after retry to bound memory.

  flushInFlight = false
  if (queue.length > 0 || flushAgainAfter) {
    flushAgainAfter = false
    scheduleFlush(queue.length > 0)
  }
}

function runWhenIdle(fn: () => void) {
  const ric = (
    window as Window & {
      requestIdleCallback?: (
        cb: () => void,
        opts?: { timeout: number },
      ) => number
    }
  ).requestIdleCallback
  if (typeof ric === 'function') {
    ric(fn, { timeout: 2500 })
    return
  }
  if (typeof requestAnimationFrame === 'function') {
    requestAnimationFrame(() => setTimeout(fn, 0))
    return
  }
  setTimeout(fn, 0)
}

function pauseEngagementClock() {
  if (engageVisibleSince != null) {
    engageAccumMs += Date.now() - engageVisibleSince
    engageVisibleSince = null
  }
}

function resumeEngagementClock() {
  if (
    engagePath &&
    engageVisibleSince == null &&
    typeof document !== 'undefined' &&
    document.visibilityState === 'visible'
  ) {
    engageVisibleSince = Date.now()
  }
}

function flushEngagement(force = false) {
  pauseEngagementClock()
  if (!engagePath) return
  const ms = Math.min(engageAccumMs, MAX_ENGAGEMENT_FLUSH_MS)
  engageAccumMs = 0
  if (ms < MIN_ENGAGEMENT_MS && !force) {
    resumeEngagementClock()
    return
  }
  if (ms >= MIN_ENGAGEMENT_MS) {
    enqueue({ type: 'engagement', path: engagePath, ms: Math.round(ms) })
  }
  resumeEngagementClock()
}

function stopEngagement() {
  flushEngagement(true)
  clearEngagementTimers()
}

function startEngagement(path: string) {
  if (engagePath === path) return
  stopEngagement()
  if (!baseAllowed() || !pathAllowed(path)) return
  engagePath = path
  engageAccumMs = 0
  if (typeof document !== 'undefined' && document.visibilityState === 'visible') {
    engageVisibleSince = Date.now()
  }
  engageTickTimer = setInterval(() => {
    const pending =
      engageAccumMs +
      (engageVisibleSince != null ? Date.now() - engageVisibleSince : 0)
    if (pending >= 60000) {
      flushEngagement(true)
    }
  }, ENGAGE_TICK_MS)
}

function ensureLifecycleListeners() {
  if (listenersBound || typeof window === 'undefined') return
  listenersBound = true

  document.addEventListener(
    'visibilitychange',
    () => {
      if (document.visibilityState === 'hidden') {
        pauseEngagementClock()
        flushEngagement(true)
        scheduleFlush(true)
      } else if (document.visibilityState === 'visible') {
        resumeEngagementClock()
      }
    },
    { passive: true },
  )

  window.addEventListener(
    'pagehide',
    () => {
      flushEngagement(true)
      scheduleFlush(true)
    },
    { passive: true },
  )
}

export function trackPageview(path?: string) {
  if (typeof window === 'undefined') return
  const p = path || location.pathname || '/'
  if (!baseAllowed() || !pathAllowed(p)) return
  if (sessionPathRecentlyTracked(p)) {
    startEngagement(p)
    return
  }
  markSessionPath(p)
  ensureLifecycleListeners()

  runWhenIdle(() => {
    if (!baseAllowed()) return
    enqueue({
      type: 'pageview',
      path: p,
      referrer: referrerHost(),
    })
    startEngagement(p)
  })
}

export function sanitizeAnalyticsTarget(raw?: string | null): string | undefined {
  if (raw == null) return undefined
  const s = String(raw)
    .trim()
    .toLowerCase()
    .replaceAll(/[^a-z0-9._:@+-]/g, '')
    .slice(0, 64)
  return s.length >= 1 ? s : undefined
}

export function trackEvent(
  name: string,
  opts?: { path?: string; flush?: boolean; target?: string },
) {
  if (typeof window === 'undefined') return
  if (!baseAllowed()) return
  const n = name.trim().toLowerCase()
  if (n.length < 2 || n.length > 48) return
  if (n.startsWith('__')) return
  if (!/^[a-z0-9][a-z0-9_-]*$/.test(n)) return

  const path = opts?.path || location.pathname || '/'
  if (!pathAllowed(path)) return

  const target = sanitizeAnalyticsTarget(opts?.target)

  ensureLifecycleListeners()
  enqueue({
    type: 'event',
    name: n,
    path,
    ...(target ? { target } : {}),
  })
  if (opts?.flush) {
    scheduleFlush(true)
  }
}
