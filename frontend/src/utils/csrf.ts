/** Guest CSRF probe is 200 with csrf_token: null; never 401. */

import { API_URL } from '../config'

const CSRF_TOKEN_KEY = 'csrf_token'
const CSRF_TOKEN_STORED_AT_KEY = 'csrf_token_stored_at'
const CSRF_TOKEN_EXPIRES_AT_KEY = 'csrf_token_expires_at'
const CSRF_TOKEN_HEADER = 'X-CSRF-Token'
export const CSRF_TOKEN_VERSION = 'v1'
/** Max length 1024. */
export const CSRF_TOKEN_MAX_LENGTH = 1024
const CSRF_TOKEN_SIGNATURE_LENGTH = 43 // 32-byte HMAC-SHA256, base64url

/** Client TTL 55 min; BE 3600s. */
export const CSRF_CLIENT_TTL_MS = 55 * 60 * 1000
/** BE TTL 3600s. */
export const CSRF_BE_TTL_SECS = 3600

/** Coalesce concurrent CSRF fetches. */
let inflight: Promise<string | null> | null = null

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

/** expires_in (s) anchors client TTL. */
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
): void {
  if (token) {
    const now = Date.now()
    sessionStorage.setItem(CSRF_TOKEN_KEY, token)
    sessionStorage.setItem(CSRF_TOKEN_STORED_AT_KEY, String(now))
    // Reuse must not reset TTL to 55 min when BE expires_in is present.
    const beMs =
      expiresInSec != null
        ? Math.min(expiresInSec * 1000, CSRF_BE_TTL_SECS * 1000)
        : CSRF_CLIENT_TTL_MS
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

async function fetchCSRFTokenFromServer(): Promise<{
  token: string | null
  expiresInSec: number | null
}> {
  try {
    const response = await fetch(`${API_URL}/api/csrf-token`, {
      method: 'GET',
      credentials: 'include',
    })

    if (!response.ok) {
      console.warn('Failed to fetch CSRF token from server:', response.status)
      return { token: null, expiresInSec: null }
    }

    const data: unknown = await response.json()
    const parsed = parseCsrfTokenResponse(data)
    if (!parsed.token) {
      console.debug('[csrf] no session — CSRF token null (guest)')
    }
    return parsed
  } catch (error) {
    console.error('Error fetching CSRF token:', error)
    return { token: null, expiresInSec: null }
  }
}

export async function getCSRFToken(
  forceRefresh: boolean = false,
): Promise<string | null> {
  if (!forceRefresh) {
    const token = sessionStorage.getItem(CSRF_TOKEN_KEY)
    const storedAtRaw = sessionStorage.getItem(CSRF_TOKEN_STORED_AT_KEY)
    const storedAt = storedAtRaw ? Number(storedAtRaw) : null
    const expiresAtRaw = sessionStorage.getItem(CSRF_TOKEN_EXPIRES_AT_KEY)
    const expiresAt = expiresAtRaw ? Number(expiresAtRaw) : null

    if (
      token &&
      isValidCSRFToken(token) &&
      isCsrfCacheFresh(storedAt, Date.now(), CSRF_CLIENT_TTL_MS, expiresAt)
    ) {
      return token
    }
    // Drop stale cache before re-fetch.
    if (token) {
      sessionStorage.removeItem(CSRF_TOKEN_KEY)
      sessionStorage.removeItem(CSRF_TOKEN_STORED_AT_KEY)
      sessionStorage.removeItem(CSRF_TOKEN_EXPIRES_AT_KEY)
    }
  }

  if (forceRefresh || !inflight) {
    const request = fetchCSRFTokenFromServer()
      .then((result) => {
        // Clearing or replacing the request revokes its right to persist.
        if (inflight !== request) return sessionStorage.getItem(CSRF_TOKEN_KEY)
        persistCsrfToken(result.token, result.expiresInSec)
        return result.token
      })
      .finally(() => {
        if (inflight === request) inflight = null
      })
    inflight = request
  }

  return inflight
}

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

export function clearCSRFToken(): void {
  sessionStorage.removeItem(CSRF_TOKEN_KEY)
  sessionStorage.removeItem(CSRF_TOKEN_STORED_AT_KEY)
  sessionStorage.removeItem(CSRF_TOKEN_EXPIRES_AT_KEY)
  inflight = null
}

export function getCSRFHeaderName(): string {
  return CSRF_TOKEN_HEADER
}
