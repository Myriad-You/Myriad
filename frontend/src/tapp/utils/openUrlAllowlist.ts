/**
 * Host-side resolution for manifest `openUrls` + `Tapp.ui.openUrl`.
 *
 * Security model:
 * - Tapp never passes a free-form URL; only a declared `id` plus optional path/query.
 * - Host rebuilds the URL from the install-time allowlist and re-validates the result.
 * - HTTPS only (http allowed solely for loopback hosts).
 */

export type OpenUrlMatchMode = 'exact' | 'prefix' | 'origin'

export interface OpenUrlDeclaration {
  id: string
  url: string
  /** Defaults to exact when omitted / unknown. */
  match?: OpenUrlMatchMode | string
}

export interface OpenUrlRequest {
  id: string
  /** Relative path or origin-absolute path; never a full URL. */
  path?: string
  /** Plain string query pairs only. */
  query?: Record<string, string>
}

export type OpenUrlResolveResult =
  | { ok: true; url: string; id: string; match: OpenUrlMatchMode }
  | { ok: false; error: string }

const MAX_URL_LEN = 2048
const MAX_QUERY_KEYS = 16
const MAX_QUERY_VALUE_LEN = 512

function isLoopbackHost(hostname: string): boolean {
  const h = hostname.toLowerCase()
  return h === 'localhost' || h === '127.0.0.1' || h === '[::1]' || h === '::1'
}

/** Strict target URL rules for declaration and resolved open. */
export function isAllowedOpenUrlTarget(raw: string): boolean {
  if (!raw || raw.length > MAX_URL_LEN) return false
  if (/[\s\u0000-\u001F\u007F]/.test(raw)) return false
  let u: URL
  try {
    u = new URL(raw)
  } catch {
    return false
  }
  if (u.username || u.password) return false
  if (!u.hostname) return false
  if (u.protocol === 'https:') return true
  if (u.protocol === 'http:' && isLoopbackHost(u.hostname)) return true
  return false
}

function normalizeMatch(raw: unknown): OpenUrlMatchMode {
  if (raw === 'prefix' || raw === 'origin' || raw === 'exact') return raw
  return 'exact'
}

function pathHasTraversal(pathname: string): boolean {
  // Split on / and reject empty-after-decode ".." segments.
  const segments = pathname.split('/')
  for (const segment of segments) {
    let decoded = segment
    try {
      decoded = decodeURIComponent(segment)
    } catch {
      return true
    }
    if (decoded === '..' || decoded === '.') {
      // "." is harmless but unusual in declared navigation; reject both.
      if (decoded === '..') return true
    }
    if (decoded.includes('\\') || decoded.includes('\0')) return true
  }
  return false
}

function looksLikeAbsoluteUrl(path: string): boolean {
  // scheme: or //host
  if (path.startsWith('//')) return true
  if (/^[a-z][a-z0-9+.-]*:/i.test(path)) return true
  return false
}

function applyQuery(url: URL, query: Record<string, string> | undefined): string | null {
  if (!query) return null
  const keys = Object.keys(query)
  if (keys.length > MAX_QUERY_KEYS) return 'Too many query parameters'
  for (const key of keys) {
    if (!key || key.length > 128) return 'Invalid query key'
    if (!/^[\w.~-]+$/.test(key)) return 'Invalid query key characters'
    const value = query[key]
    if (typeof value !== 'string') return 'Query values must be strings'
    if (value.length > MAX_QUERY_VALUE_LEN) return 'Query value too long'
    if (/[\u0000-\u001F\u007F]/.test(value)) return 'Query value has control characters'
    url.searchParams.set(key, value)
  }
  return null
}

function matchesAllowlist(resolved: URL, base: URL, mode: OpenUrlMatchMode): boolean {
  if (!isAllowedOpenUrlTarget(resolved.href)) return false
  if (resolved.username || resolved.password) return false
  if (pathHasTraversal(resolved.pathname)) return false

  if (mode === 'exact') {
    // Compare without hash; declarations must not use fragments.
    const a = new URL(resolved.href)
    const b = new URL(base.href)
    a.hash = ''
    b.hash = ''
    return a.href === b.href
  }

  if (resolved.origin !== base.origin) return false

  if (mode === 'origin') {
    return true
  }

  // prefix: pathname must stay under base.pathname
  let basePath = base.pathname
  if (!basePath.endsWith('/')) {
    // Treat file-like prefix as directory-or-exact-file prefix:
    // https://ex.com/docs allows /docs and /docs/... but not /docsEvil
    const resolvedPath = resolved.pathname
    if (resolvedPath === basePath) return true
    if (!basePath.endsWith('/')) basePath = `${basePath}/`
    return resolvedPath.startsWith(basePath)
  }
  return resolved.pathname.startsWith(basePath)
}

/**
 * Resolve a sandbox open request against install-time declarations.
 * Never trusts a caller-supplied absolute URL.
 */
export function resolveOpenUrl(
  declarations: readonly OpenUrlDeclaration[] | null | undefined,
  request: OpenUrlRequest,
): OpenUrlResolveResult {
  if (!request || typeof request.id !== 'string' || !request.id.trim()) {
    return { ok: false, error: 'openUrl requires a declared id' }
  }
  const id = request.id.trim()
  const list = declarations ?? []
  const entry = list.find(item => item && item.id === id)
  if (!entry) {
    return { ok: false, error: `openUrl id is not declared: ${id}` }
  }
  if (!isAllowedOpenUrlTarget(entry.url)) {
    return { ok: false, error: 'Declared openUrl target is invalid' }
  }

  let base: URL
  try {
    base = new URL(entry.url)
  } catch {
    return { ok: false, error: 'Declared openUrl target is invalid' }
  }
  base.hash = ''

  const mode = normalizeMatch(entry.match)
  const path = request.path
  if (path !== undefined && path !== null) {
    if (typeof path !== 'string') {
      return { ok: false, error: 'openUrl path must be a string' }
    }
    if (path.length > 1024) {
      return { ok: false, error: 'openUrl path is too long' }
    }
    if (path.includes('\\') || /[\u0000-\u001F\u007F]/.test(path)) {
      return { ok: false, error: 'openUrl path has invalid characters' }
    }
    if (looksLikeAbsoluteUrl(path)) {
      return { ok: false, error: 'openUrl path must be relative (not a full URL)' }
    }
  }

  if (mode === 'exact') {
    if (path && path.length > 0) {
      return { ok: false, error: 'exact openUrl does not accept path' }
    }
    if (request.query && Object.keys(request.query).length > 0) {
      return { ok: false, error: 'exact openUrl does not accept query' }
    }
    const exact = new URL(base.href)
    exact.hash = ''
    if (!matchesAllowlist(exact, base, 'exact')) {
      return { ok: false, error: 'Resolved URL is not allowlisted' }
    }
    if (exact.href.length > MAX_URL_LEN) {
      return { ok: false, error: 'Resolved URL is too long' }
    }
    return { ok: true, url: exact.href, id, match: 'exact' }
  }

  let resolved: URL
  try {
    if (path && path.length > 0) {
      // Relative to the declared base (directory-style when base ends with /).
      resolved = new URL(path, base)
    } else {
      resolved = new URL(base.href)
    }
  } catch {
    return { ok: false, error: 'Failed to resolve openUrl path' }
  }
  resolved.hash = ''

  const queryError = applyQuery(resolved, request.query)
  if (queryError) return { ok: false, error: queryError }

  if (!matchesAllowlist(resolved, base, mode)) {
    return { ok: false, error: 'Resolved URL is not allowlisted' }
  }
  if (resolved.href.length > MAX_URL_LEN) {
    return { ok: false, error: 'Resolved URL is too long' }
  }

  return { ok: true, url: resolved.href, id, match: mode }
}

/** Public list payload for `Tapp.ui.listOpenUrls`. */
export function listOpenUrlDeclarations(
  declarations: readonly OpenUrlDeclaration[] | null | undefined,
): Array<{ id: string; url: string; match: OpenUrlMatchMode }> {
  if (!declarations?.length) return []
  return declarations
    .filter(item => item && typeof item.id === 'string' && typeof item.url === 'string')
    .map(item => ({
      id: item.id,
      url: item.url,
      match: normalizeMatch(item.match),
    }))
}

/** Simple sliding-window rate limit for host open calls. */
export class OpenUrlRateLimiter {
  private readonly hits = new Map<string, number[]>()
  private readonly maxHits: number
  private readonly windowMs: number

  constructor(maxHits = 8, windowMs = 10_000) {
    this.maxHits = maxHits
    this.windowMs = windowMs
  }

  allow(key: string, now = Date.now()): boolean {
    const windowStart = now - this.windowMs
    const prev = this.hits.get(key) ?? []
    const recent = prev.filter(ts => ts > windowStart)
    if (recent.length >= this.maxHits) {
      this.hits.set(key, recent)
      return false
    }
    recent.push(now)
    this.hits.set(key, recent)
    return true
  }
}
