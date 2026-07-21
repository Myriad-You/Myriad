/**
 * CSRF (Cross-Site Request Forgery) 防护工具
 * ✅ 安全修复 P0: 从服务器获取 CSRF Token 而不是客户端生成
 */

import { API_URL } from '../config'

const CSRF_TOKEN_KEY = 'csrf_token'
const CSRF_TOKEN_HEADER = 'X-CSRF-Token'

/** 合流并发请求，避免多个调用方各打一次 /api/csrf-token */
let inflight: Promise<string | null> | null = null

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
      if (response.status === 401) {
        // 游客的正常状态，不是故障：后端没有 session 可绑定 token，
        // csrf_middleware 也会跳过游客的写请求。
        console.debug('[csrf] no session — CSRF token not applicable (guest)')
      } else {
        console.warn('Failed to fetch CSRF token from server:', response.status)
      }
      return null
    }

    const data = await response.json()
    const token = data.csrf_token || null

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
 * Guest note: backend requires a session for /api/csrf-token (401 otherwise).
 * Callers should only invoke this for authenticated mutating requests — see
 * `frontend/src/lib/api.ts` (GET skips CSRF) and Home.tsx (CSRF only when logged in).
 * 401 is logged at debug, not error.
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
