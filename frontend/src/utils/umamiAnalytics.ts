declare global {
  interface Window {
    umami?: {
      track: {
        (event?: string | Record<string, unknown>): void
        (
          event: string,
          data?: Record<string, unknown>,
        ): void
        (
          props: (payload: Record<string, unknown>) => Record<string, unknown>,
        ): void
      }
    }
  }
}

const OPT_OUT_KEY = 'myriad_analytics_optout'
const SKIP_PREFIXES = ['/setup']
const SCRIPT_ATTR = 'data-myriad-umami'
const WEBSITE_ID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i

let activeWebsiteId: string | null = null
let scriptGeneration = 0
let excludeStaff = false
let pendingPath: string | null = null

export function normalizeUmamiWebsiteId(raw: string | null | undefined): string {
  return (raw || '').trim()
}

export function normalizeUmamiScriptUrl(raw: string | null | undefined): string {
  return (raw || '').trim().replaceAll(/\/+$/g, '')
}

export function isValidUmamiWebsiteId(id: string): boolean {
  if (!id) return false
  if (WEBSITE_ID_PATTERN.test(id)) return true
  return /^[\w-]{8,64}$/.test(id)
}

export function isValidUmamiScriptUrl(url: string): boolean {
  if (!url) return false
  try {
    const u = new URL(url)
    if (u.protocol !== 'https:' && u.protocol !== 'http:') return false
    if (!u.hostname) return false
    return true
  } catch {
    return false
  }
}

export function setUmamiStaffExcluded(excluded: boolean): void {
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

function removeInjectedScript(): void {
  if (typeof document === 'undefined') return
  document
    .querySelectorAll(`script[${SCRIPT_ATTR}]`)
    .forEach((el) => el.remove())
  scriptGeneration += 1
  try {
    if (typeof window !== 'undefined') {
      delete (window as { umami?: unknown }).umami
    }
  } catch {
  }
}

function disableUmami(): void {
  activeWebsiteId = null
  pendingPath = null
  removeInjectedScript()
}

function injectScript(scriptUrl: string, websiteId: string): void {
  if (typeof document === 'undefined') return

  const existing = document.querySelector<HTMLScriptElement>(
    `script[${SCRIPT_ATTR}]`,
  )
  if (
    existing?.getAttribute('src') === scriptUrl &&
    existing.getAttribute('data-website-id') === websiteId
  ) {
    return
  }

  if (existing) {
    existing.remove()
  }

  const gen = ++scriptGeneration

  const script = document.createElement('script')
  script.defer = true
  script.src = scriptUrl
  script.setAttribute(SCRIPT_ATTR, '1')
  script.setAttribute('data-website-id', websiteId)
  script.setAttribute('data-auto-track', 'false')
  script.onload = () => {
    if (gen !== scriptGeneration) return
    const flushPath = pendingPath
    pendingPath = null
    if (flushPath) trackUmamiPageview(flushPath)
  }
  script.onerror = () => {
    if (gen !== scriptGeneration) return
    console.warn('[Umami] failed to load tracker script')
  }
  document.head.appendChild(script)
}

export function configureUmami(
  websiteId: string | null | undefined,
  scriptUrl: string | null | undefined,
): void {
  if (typeof window === 'undefined') return

  const id = normalizeUmamiWebsiteId(websiteId)
  let url = normalizeUmamiScriptUrl(scriptUrl)

  if (url && isValidUmamiScriptUrl(url) && !/\.js(\?|$)/i.test(url)) {
    try {
      const u = new URL(url)
      if (!u.pathname || u.pathname === '/') {
        u.pathname = '/script.js'
        url = u.toString().replaceAll(/\/$/g, '')
      }
    } catch {
    }
  }

  if (
    !id ||
    !url ||
    !isValidUmamiWebsiteId(id) ||
    !isValidUmamiScriptUrl(url) ||
    isOptedOut()
  ) {
    disableUmami()
    return
  }

  activeWebsiteId = id
  injectScript(url, id)

  if (typeof window.umami?.track === 'function') {
    const flushPath = pendingPath
    pendingPath = null
    if (flushPath) trackUmamiPageview(flushPath)
  }
}

export function trackUmamiPageview(path?: string): void {
  if (typeof window === 'undefined') return
  if (excludeStaff) return
  if (isOptedOut()) return

  const pathname =
    path ||
    (typeof location !== 'undefined' ? location.pathname : '') ||
    '/'
  if (!pathAllowed(pathname)) return

  if (!activeWebsiteId || typeof window.umami?.track !== 'function') {
    pendingPath = pathname
    return
  }

  const pagePath =
    pathname +
    (typeof location !== 'undefined' ? location.search || '' : '')

  try {
    window.umami.track((payload) => ({
      ...payload,
      url: pagePath,
      title: typeof document !== 'undefined' ? document.title : payload.title,
    }))
  } catch {
    try {
      window.umami.track({
        url: pagePath,
        title: typeof document !== 'undefined' ? document.title : undefined,
      })
    } catch {
      /* ignore */
    }
  }
}
