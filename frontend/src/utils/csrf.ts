/**
 * CSRF (Cross-Site Request Forgery) 防护工具
 * ✅ 安全修复 P0: 从服务器获取 CSRF Token 而不是客户端生成
 *
 * Guest contract: GET /api/csrf-token returns HTTP 200 with
 * `{ csrf_token: null }` when there is no session (never 401).
 * Mutating guests are still skipped by csrf_middleware without a token.
 */

import { API_URL } from '../config'

const CSRF_TOKEN_KEY = 'csrf_token'
/** Epoch-ms when the cached token was stored (for client-side TTL). */
const CSRF_TOKEN_STORED_AT_KEY = 'csrf_token_stored_at'
/** Absolute epoch-ms when client should treat the token as stale (BE-anchored). */
const CSRF_TOKEN_EXPIRES_AT_KEY = 'csrf_token_expires_at'
const CSRF_TOKEN_HEADER = 'X-CSRF-Token'
/** Versioned stateless backend token prefix. */
export const CSRF_TOKEN_VERSION = 'v1'
/** Keep client parsing bounded before a token reaches sessionStorage/headers. */
export const CSRF_TOKEN_MAX_LENGTH = 1024
const CSRF_TOKEN_SIGNATURE_LENGTH = 43 // 32-byte HMAC-SHA256, base64url

/**
 * Client cache TTL: BE tokens live ~3600s; refresh slightly earlier so we
 * rarely hit a 403 mid-request from a just-expired server token.
 */
export const CSRF_CLIENT_TTL_MS = 55 * 60 * 1000
/** BE hard lifetime (must match middleware csrf 3600s). */
export const CSRF_BE_TTL_SECS = 3600

/** 合流并发请求，避免多个调用方各打一次 /api/csrf-token */
let inflight: Promise<string | null> | null = null
/**
 * Write generation: only the latest fetch may persist to sessionStorage.
 * Prevents a non-force inflight from overwriting a newer forceRefresh result.
 */
let writeGeneration = 0

/** Whether a sessionStorage-cached token is still within client TTL. */
export function isCsrfCacheFresh(
  storedAtMs: number | null,
  nowMs: number = Date.now(),
  ttlMs: number = CSRF_CLIENT_TTL_MS,
  expiresAtMs: number | null = null,
): boolean {
  if (expiresAtMs != null && Number.isFinite(expiresAtMs) && expiresAtMs > 0) {
    return nowMs < expiresAtMs
  }
  if (storedAtMs == null || !Number.isFinite(storedAtMs) || storedAtMs <= 0) {
    return false
  }
  return nowMs - storedAtMs < ttlMs
}

/**
 * Parse CSRF probe body. Accepts string token or null/missing for guests.
 * Optional `expires_in` (seconds remaining on BE) anchors client TTL.
 */
export function parseCsrfTokenResponse(data: unknown): {
  token: string | null
  expiresInSec: number | null
} {
  if (!data || typeof data !== 'object') {
    return { token: null, expiresInSec: null }
  }
  const body = data as { csrf_token?: unknown; expires_in?: unknown }
  const raw = body.csrf_token
  let token: string | null = null
  if (typeof raw === 'string') {
    const trimmed = raw.trim()
    if (trimmed && isValidCSRFToken(trimmed)) {
      token = trimmed
    }
  }
  let expiresInSec: number | null = null
  if (typeof body.expires_in === 'number' && Number.isFinite(body.expires_in)) {
    expiresInSec = Math.max(0, Math.floor(body.expires_in))
  } else if (typeof body.expires_in === 'string') {
    const n = Number.parseInt(body.expires_in, 10)
    if (Number.isFinite(n)) expiresInSec = Math.max(0, n)
  }
  return { token, expiresInSec }
}

function persistCsrfToken(
  token: string | null,
  expiresInSec: number | null,
  generation: number,
): void {
  // Drop stale writes from superseded inflight / race with forceRefresh
  if (generation !== writeGeneration) return

  if (token) {
    const now = Date.now()
    sessionStorage.setItem(CSRF_TOKEN_KEY, token)
    sessionStorage.setItem(CSRF_TOKEN_STORED_AT_KEY, String(now))
    // Anchor to BE remaining life when provided (reuse must not reset to 55min).
    const beMs =
      expiresInSec != null
        ? Math.min(expiresInSec * 1000, CSRF_BE_TTL_SECS * 1000)
        : CSRF_CLIENT_TTL_MS
    // Leave a small buffer before BE hard expiry
    const clientMs = Math.min(
      CSRF_CLIENT_TTL_MS,
      Math.max(30_000, beMs - 30_000),
    )
    sessionStorage.setItem(CSRF_TOKEN_EXPIRES_AT_KEY, String(now + clientMs))
  } else {
    sessionStorage.removeItem(CSRF_TOKEN_KEY)
    sessionStorage.removeItem(CSRF_TOKEN_STORED_AT_KEY)
    sessionStorage.removeItem(CSRF_TOKEN_EXPIRES_AT_KEY)
  }
}

/**
 * 从服务器获取 CSRF Token
 * ✅ 安全修复 P0: Token 由服务器生成并验证，防止伪造
 */
async function fetchCSRFTokenFromServer(): Promise<{
  token: string | null
  expiresInSec: number | null
}> {
  try {
    const response = await fetch(`${API_URL}/api/csrf-token`, {
      method: 'GET',
      credentials: 'include', // 包含认证 Cookie
    })

    if (!response.ok) {
      // Probe should not 401 for guests; non-OK is a real fault.
      console.warn('Failed to fetch CSRF token from server:', response.status)
      return { token: null, expiresInSec: null }
    }

    const data: unknown = await response.json()
    const parsed = parseCsrfTokenResponse(data)
    if (!parsed.token) {
      // HTTP 200 + null = guest / no session — expected, not a failure.
      console.debug('[csrf] no session — CSRF token null (guest)')
    }
    return parsed
  } catch (error) {
    console.error('Error fetching CSRF token:', error)
    return { token: null, expiresInSec: null }
  }
}

/**
 * 获取当前 CSRF Token，如果不存在则从服务器获取新的
 * ✅ 安全修复 P0: 改为从服务器获取而不是客户端生成
 *
 * Guest note: backend returns 200 + csrf_token:null without a session.
 * Callers should only need this for authenticated mutating requests —
 * see `frontend/src/lib/api.ts` (GET skips CSRF).
 *
 * @param forceRefresh - 是否强制从服务器获取新 Token（默认 false）
 */
export async function getCSRFToken(
  forceRefresh: boolean = false,
): Promise<string | null> {
  // 如果不强制刷新，先尝试使用缓存的 Token（含 ~55min 客户端 TTL / BE expires_at）
  if (!forceRefresh) {
    const token = sessionStorage.getItem(CSRF_TOKEN_KEY)
    const storedAtRaw = sessionStorage.getItem(CSRF_TOKEN_STORED_AT_KEY)
    const storedAt = storedAtRaw ? Number(storedAtRaw) : null
    const expiresAtRaw = sessionStorage.getItem(CSRF_TOKEN_EXPIRES_AT_KEY)
    const expiresAt = expiresAtRaw ? Number(expiresAtRaw) : null

    // 验证现有 Token 格式 + TTL
    if (
      token &&
      isValidCSRFToken(token) &&
      isCsrfCacheFresh(storedAt, Date.now(), CSRF_CLIENT_TTL_MS, expiresAt)
    ) {
      return token
    }
    // Stale cache: drop before re-fetch so concurrent callers don't reuse it.
    if (token) {
      sessionStorage.removeItem(CSRF_TOKEN_KEY)
      sessionStorage.removeItem(CSRF_TOKEN_STORED_AT_KEY)
      sessionStorage.removeItem(CSRF_TOKEN_EXPIRES_AT_KEY)
    }
  }

  // forceRefresh: invalidate inflight so a lagging non-force cannot overwrite us
  if (forceRefresh) {
    inflight = null
  }

  const myGen = ++writeGeneration

  // force path: dedicated request; generation guards store
  if (forceRefresh) {
    const result = await fetchCSRFTokenFromServer()
    persistCsrfToken(result.token, result.expiresInSec, myGen)
    return result.token
  }

  // Non-force: coalesce concurrent callers
  if (!inflight) {
    const genAtStart = myGen
    inflight = fetchCSRFTokenFromServer()
      .then((result) => {
        persistCsrfToken(result.token, result.expiresInSec, genAtStart)
        return result.token
      })
      .finally(() => {
        inflight = null
      })
  }

  const token = await inflight
  // If forceRefresh finished while we waited, prefer what force stored
  if (myGen !== writeGeneration) {
    return sessionStorage.getItem(CSRF_TOKEN_KEY)
  }
  return token
}

/**
 * Validate the bounded wire shape of a server-issued stateless token.
 * Cryptographic verification remains server-side; this only rejects malformed
 * or oversized values before caching/sending them.
 */
export function isValidCSRFToken(token: string): boolean {
  if (
    typeof token !== 'string' ||
    token.length === 0 ||
    token.length > CSRF_TOKEN_MAX_LENGTH
  ) {
    return false
  }
  const parts = token.split('.')
  if (parts.length !== 3 || parts[0] !== CSRF_TOKEN_VERSION) return false
  if (!/^[\w-]+$/.test(parts[1])) return false
  if (
    parts[1].length < 16 ||
    parts[1].length > CSRF_TOKEN_MAX_LENGTH ||
    parts[2].length !== CSRF_TOKEN_SIGNATURE_LENGTH ||
    !/^[\w-]+$/.test(parts[2])
  ) {
    return false
  }
  return true
}

/**
 * 清除 CSRF Token（登出 / 403 轮换时调用）。
 * 广播 `csrf-token-cleared` 让 userInfoCache 等内存缓存一并失效，
 * 避免 axios 轮换 sessionStorage 后 Home 仍用旧 getCsrfTokenWithCache 值。
 */
export function clearCSRFToken(): void {
  sessionStorage.removeItem(CSRF_TOKEN_KEY)
  sessionStorage.removeItem(CSRF_TOKEN_STORED_AT_KEY)
  sessionStorage.removeItem(CSRF_TOKEN_EXPIRES_AT_KEY)
  inflight = null
  writeGeneration += 1
  if (typeof window !== 'undefined') {
    window.dispatchEvent(new CustomEvent('csrf-token-cleared'))
  }
}

/**
 * 获取 CSRF Token Header 名称
 */
export function getCSRFHeaderName(): string {
  return CSRF_TOKEN_HEADER
}
