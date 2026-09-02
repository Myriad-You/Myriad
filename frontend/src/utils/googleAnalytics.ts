/**
 * Google Analytics 4 (gtag.js) client.
 *
 * - Measurement ID from site metadata / config (G-XXXXXXXX)
 * - SPA page_view via route tracker (send_page_view disabled on config)
 * - Respects first-party analytics opt-out (`myriad_analytics_optout`)
 * - Skips /setup and empty/invalid IDs
 */

declare global {
  interface Window {
    dataLayer?: unknown[]
    gtag?: (...args: unknown[]) => void
  }
}

/** Align with siteAnalytics OPT_OUT_KEY */
const OPT_OUT_KEY = 'myriad_analytics_optout'
const SKIP_PREFIXES = ['/setup']
/** GA4 Measurement ID (also accepts legacy UA- for gtag compatibility) */
const GA_ID_PATTERN = /^(G-[A-Z0-9]+|UA-\d+-\d+)$/i

const SCRIPT_ATTR = 'data-myriad-ga'
const SCRIPT_SRC_BASE = 'https://www.googletagmanager.com/gtag/js?id='

let activeId: string | null = null
let scriptLoading = false
/** Align with first-party staff exclusion (admin / owner self-traffic) */
let excludeStaff = false
/** Last path requested before gtag was ready — flushed once configure succeeds */
let pendingPath: string | null = null

export function normalizeGaMeasurementId(raw: string | null | undefined): string {
  return (raw || '').trim()
}

export function isValidGaMeasurementId(id: string): boolean {
  return GA_ID_PATTERN.test(id)
}

/** Admin/owner sessions should not pollute GA (same policy as first-party stats). */
export function setGoogleAnalyticsStaffExcluded(excluded: boolean): void {
  excludeStaff = excluded
}

function isOptedOut(): boolean {
  try {
    return localStorage.getItem(OPT_OUT_KEY) === '1'
  } catch {
    return false
  }
}

function pathAllowed(pathname: string): boolean {
  if (!pathname) return false
  return !SKIP_PREFIXES.some(
    (p) => pathname === p || pathname.startsWith(`${p}/`),
  )
}

function ensureGtagStub(): void {
  if (typeof window === 'undefined') return
  window.dataLayer = window.dataLayer || []
  if (typeof window.gtag === 'function') return
  window.gtag = function gtag(...args: unknown[]) {
    window.dataLayer?.push(args)
  }
}

function removeInjectedScript(): void {
  if (typeof document === 'undefined') return
  document
    .querySelectorAll(`script[${SCRIPT_ATTR}]`)
    .forEach((el) => el.remove())
  scriptLoading = false
}

/** Stop tracking and remove our gtag loader tag (cannot fully unload gtag from memory). */
function disableGoogleAnalytics(): void {
  activeId = null
  pendingPath = null
  removeInjectedScript()
}

function injectScript(id: string): void {
  if (typeof document === 'undefined') return
  const existing = document.querySelector<HTMLScriptElement>(
    `script[${SCRIPT_ATTR}]`,
  )
  const src = `${SCRIPT_SRC_BASE}${encodeURIComponent(id)}`
  if (existing) {
    if (existing.getAttribute('src') === src) return
    existing.remove()
  }
  if (scriptLoading) return
  scriptLoading = true
  const script = document.createElement('script')
  script.async = true
  script.src = src
  script.setAttribute(SCRIPT_ATTR, id)
  script.onload = () => {
    scriptLoading = false
  }
  script.onerror = () => {
    scriptLoading = false
    console.warn('[GA] failed to load gtag.js')
  }
  document.head.appendChild(script)
}

/**
 * Enable / update / disable GA4 from measurement ID.
 * Empty or invalid id stops page_view sends and removes the injected loader script.
 */
export function configureGoogleAnalytics(
  measurementId: string | null | undefined,
): void {
  if (typeof window === 'undefined') return

  const id = normalizeGaMeasurementId(measurementId)
  if (!id || !isValidGaMeasurementId(id) || isOptedOut()) {
    disableGoogleAnalytics()
    return
  }

  ensureGtagStub()
  injectScript(id)

  // Avoid automatic first hit — SPA tracker owns page_view for path accuracy.
  window.gtag?.('js', new Date())
  window.gtag?.('config', id, {
    send_page_view: false,
    anonymize_ip: true,
  })
  activeId = id

  // Metadata fetch is async: only flush paths the router already tried (no double-hit).
  const flushPath = pendingPath
  pendingPath = null
  if (flushPath) {
    trackGooglePageview(flushPath)
  }
}

/**
 * Send a GA4 page_view for SPA navigations.
 * No-op when GA is not configured, opted out, staff, or path is skipped.
 * If gtag is not ready yet, remembers the path for a one-shot flush after configure.
 */
export function trackGooglePageview(path?: string): void {
  if (typeof window === 'undefined') return
  if (excludeStaff) return
  if (isOptedOut()) return

  const pathname =
    path ||
    (typeof location !== 'undefined' ? location.pathname : '') ||
    '/'
  if (!pathAllowed(pathname)) return

  if (!activeId || typeof window.gtag !== 'function') {
    pendingPath = pathname
    return
  }

  const pagePath =
    pathname +
    (typeof location !== 'undefined' ? location.search || '' : '')

  window.gtag('event', 'page_view', {
    page_path: pagePath,
    page_title: typeof document !== 'undefined' ? document.title : undefined,
    page_location:
      typeof location !== 'undefined' ? location.href : undefined,
    send_to: activeId,
  })
}
