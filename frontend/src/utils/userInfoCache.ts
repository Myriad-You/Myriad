/** CSRF token is memory-only; not localStorage. */

/** Memory CSRF cache must clear with sessionStorage. */
let csrfTokenCache: { token: string; timestamp: number } | null = null
const CSRF_CACHE_DURATION = 10 * 60 * 1000 // 10 min

if (typeof window !== 'undefined') {
  window.addEventListener('csrf-token-cleared', () => {
    csrfTokenCache = null
  })
}

export async function getCsrfTokenWithCache(
  forceRefresh = false,
): Promise<string> {
  // Prefer sessionStorage when it matches memory.
  let sessionToken: string | null = null
  try {
    sessionToken = sessionStorage.getItem('csrf_token')
  } catch {
    /* ignore */
  }

  if (
    !forceRefresh &&
    csrfTokenCache &&
    Date.now() - csrfTokenCache.timestamp < CSRF_CACHE_DURATION
  ) {
    // Stale when sessionStorage differs.
    if (!sessionToken || sessionToken === csrfTokenCache.token) {
      if (sessionToken) return csrfTokenCache.token
    }
    csrfTokenCache = null
  }

  try {
    // Single source: getCSRFToken.
    const { getCSRFToken } = await import('./csrf')
    const token = await getCSRFToken(forceRefresh || !sessionToken)
    if (token) {
      csrfTokenCache = { token, timestamp: Date.now() }
      return token
    }
    csrfTokenCache = null
    return ''
  } catch (e) {
    console.warn('获取 CSRF Token 失败:', e)
  }

  return ''
}

export function invalidateCsrfCache(): void {
  csrfTokenCache = null
}

export function clearAllUserCache(): void {
  invalidateCsrfCache()
}
