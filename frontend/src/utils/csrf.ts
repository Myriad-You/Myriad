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
const CSRF_TOKEN_HEADER = 'X-CSRF-Token'

/** 合流并发请求，避免多个调用方各打一次 /api/csrf-token */
let inflight: Promise<string | null> | null = null

/**
 * Parse CSRF probe body. Accepts string token or null/missing for guests.
 */
export function parseCsrfTokenResponse(data: unknown): string | null {
  if (!data || typeof data !== 'object') return null
  const token = (data as { csrf_token?: unknown }).csrf_token
  if (typeof token !== 'string') return null
  const trimmed = token.trim()
  if (!trimmed || trimmed.length !== 32 || !/^[a-z0-9]{32}$/i.test(trimmed)) {
    return null
  }
  return trimmed
}

/**
 * 从服务器获取 CSRF Token
 * ✅ 安全修复 P0: Token 由服务器生成并验证，防止伪造
 */
async function fetchCSRFTokenFromServer(): Promise<string | null> {
  try {
    const response = await fetch(`${API_URL}/api/csrf-token`, {
      method: 'GET',
      credentials: 'include', // 包含认证 Cookie
    })

    if (!response.ok) {
      // Probe should not 401 for guests; non-OK is a real fault.
      console.warn('Failed to fetch CSRF token from server:', response.status)
      return null
    }

    const data: unknown = await response.json()
    const token = parseCsrfTokenResponse(data)
    if (!token) {
      // HTTP 200 + null = guest / no session — expected, not a failure.
      console.debug('[csrf] no session — CSRF token null (guest)')
    }
    return token
  } catch (error) {
    console.error('Error fetching CSRF token:', error)
    return null
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
  // 如果不强制刷新，先尝试使用缓存的 Token
  if (!forceRefresh) {
    const token = sessionStorage.getItem(CSRF_TOKEN_KEY)

    // 验证现有 Token 格式
    if (token && token.length === 32 && /^[a-z0-9]{32}$/i.test(token)) {
      return token
    }
  }

  // 从服务器获取新 Token。
  // 非强制请求之间合流，避免多个调用方各打一次；forceRefresh 必须自己发一次——
  // 复用可能是会话变化之前发出的 inflight，正好违背 force 的语义
  // （403 重试、登录后刷新都靠它）。
  let request: Promise<string | null>
  if (forceRefresh) {
    request = fetchCSRFTokenFromServer()
  } else {
    inflight ||= fetchCSRFTokenFromServer().finally(() => {
      inflight = null
    })
    request = inflight
  }

  const token = await request
  if (token) {
    sessionStorage.setItem(CSRF_TOKEN_KEY, token)
  } else {
    // Drop stale cached token when server says guest / null.
    sessionStorage.removeItem(CSRF_TOKEN_KEY)
  }

  return token
}

/**
 * 验证 CSRF Token 格式（服务器生成的格式：32字符字母数字）
 */
export function isValidCSRFToken(token: string): boolean {
  return (
    typeof token === 'string' &&
    token.length === 32 &&
    /^[a-z0-9]{32}$/i.test(token)
  )
}

/**
 * 清除 CSRF Token（登出时调用）
 */
export function clearCSRFToken(): void {
  sessionStorage.removeItem(CSRF_TOKEN_KEY)
}

/**
 * 获取 CSRF Token Header 名称
 */
export function getCSRFHeaderName(): string {
  return CSRF_TOKEN_HEADER
}

/**
 * 为请求添加 CSRF Token
 * ✅ 安全修复 P0: 使用异步版本
 */
export async function addCSRFToken(
  headers: Record<string, string> = {},
): Promise<Record<string, string>> {
  const token = await getCSRFToken()
  if (token) {
    return {
      ...headers,
      [CSRF_TOKEN_HEADER]: token,
    }
  }
  return headers
}
