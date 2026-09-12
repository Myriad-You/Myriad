/** 沙箱只传声明 id。宿主按安装期 allowlist 重建 URL。仅 HTTPS（loopback 可 http）。 */

export type OpenUrlMatchMode = 'exact' | 'prefix' | 'origin'

export interface OpenUrlDeclaration {
  id: string
  url: string
  match?: OpenUrlMatchMode | string
}

export interface OpenUrlRequest {
  id: string
  /** 相对路径或 origin 绝对路径；从不是完整 URL。 */
  path?: string
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
  const segments = pathname.split('/')
  for (const segment of segments) {
    let decoded = segment
    try {
      decoded = decodeURIComponent(segment)
    } catch {
      return true
    }
    if (decoded === '..' || decoded === '.') {
      if (decoded === '..') return true
    }
    if (decoded.includes('\\') || decoded.includes('\0')) return true
  }
  return false
}

function looksLikeAbsoluteUrl(path: string): boolean {
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
    // 比较时去掉 hash；声明不得带 fragment。
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

  let basePath = base.pathname
  if (!basePath.endsWith('/')) {
    const resolvedPath = resolved.pathname
    if (resolvedPath === basePath) return true
    if (!basePath.endsWith('/')) basePath = `${basePath}/`
    return resolvedPath.startsWith(basePath)
  }
  return resolved.pathname.startsWith(basePath)
}

/** 按安装期声明解析。不信任调用方绝对 URL。 */
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
