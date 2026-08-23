/**
 * 基础 API 服务
 *
 * 提供统一的 HTTP 请求封装
 */

import { API_URL } from '../config'
import { clearCSRFToken, getCSRFToken } from '../utils/csrf'
import { notifyHttpRateLimit } from '../utils/httpRateLimitToast'

const API_BASE = `${API_URL}/api`

export interface ApiRequestOptions extends RequestInit {
  /** 是否需要认证 */
  requireAuth?: boolean
  /** 超时时间（毫秒） */
  timeout?: number
  /** 查询参数 */
  params?: Record<string, string | number | boolean | undefined>
}

export interface ApiResponse<T> {
  data: T
  status: number
  ok: boolean
}

/**
 * API 错误类
 */
export class ApiError extends Error {
  constructor(
    message: string,
    public status: number,
    public code?: string,
    public details?: unknown,
    public hint?: string,
  ) {
    super(message)
    this.name = 'ApiError'
  }
}

const STABLE_ERROR_CODE = /^[A-Za-z][A-Za-z0-9_]{2,64}$/

function readErrorString(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value.trim() : undefined
}

export function parseApiErrorBody(
  body: unknown,
  status: number,
): { message: string; code?: string; hint?: string; details?: unknown } {
  const fallback = `API Error: ${status}`
  if (!body || typeof body !== 'object') {
    return { message: fallback }
  }
  const payload = body as {
    message?: unknown
    error?: unknown
    code?: unknown
    hint?: unknown
    details?: unknown
  }
  const label = readErrorString(payload.error)
  const message = readErrorString(payload.message) || label || fallback
  const explicitCode = readErrorString(payload.code)
  const code =
    explicitCode ||
    (label && STABLE_ERROR_CODE.test(label) ? label : undefined)
  return {
    message,
    code,
    hint: readErrorString(payload.hint),
    details: payload.details ?? body,
  }
}

/**
 * 构建完整 URL
 */
function buildUrl(
  endpoint: string,
  params?: Record<string, string | number | boolean | undefined>,
): string {
  const url = new URL(`${API_BASE}${endpoint}`, window.location.origin)

  if (params) {
    Object.entries(params).forEach(([key, value]) => {
      if (value !== undefined) {
        url.searchParams.append(key, String(value))
      }
    })
  }

  return url.toString()
}

function isCsrfErrorBody(body: {
  message?: unknown
  error?: unknown
}): boolean {
  const haystack =
    `${String(body.error ?? '')} ${String(body.message ?? '')}`.toLowerCase()
  return haystack.includes('csrf')
}

/**
 * 通用 API 请求
 * State-changing calls retry once after CSRF rejection (stale sessionStorage /
 * backend restart / token expiry), matching brewApi / speechApi / brewliaApi.
 */
async function request<T>(
  endpoint: string,
  options: ApiRequestOptions = {},
  retryOnCSRFError: boolean = true,
): Promise<T> {
  const {
    requireAuth: _requireAuth = false,
    timeout = 30000,
    params,
    ...fetchOptions
  } = options

  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(fetchOptions.headers as Record<string, string>),
  }

  // 对于状态变更操作添加 CSRF Token
  const method = fetchOptions.method?.toUpperCase() || 'GET'
  const needsCSRF = ['POST', 'PUT', 'PATCH', 'DELETE'].includes(method)

  if (needsCSRF) {
    const csrfToken = await getCSRFToken()
    if (csrfToken) {
      headers['X-CSRF-Token'] = csrfToken
    }
  }

  const url = buildUrl(endpoint, params)

  // 创建 AbortController 用于超时控制
  const controller = new AbortController()
  const timeoutId = setTimeout(() => controller.abort(), timeout)

  try {
    const response = await fetch(url, {
      ...fetchOptions,
      headers,
      credentials: 'include',
      signal: controller.signal,
    })

    clearTimeout(timeoutId)

    if (!response.ok) {
      notifyHttpRateLimit(response)
      let errorMessage = `API Error: ${response.status}`
      let errorCode: string | undefined
      let errorDetails: unknown
      let errorHint: string | undefined
      let errorBody: { message?: unknown; error?: unknown; code?: string; details?: unknown } | null =
        null

      try {
        errorBody = await response.json()
        const parsed = parseApiErrorBody(errorBody, response.status)
        errorMessage = parsed.message
        errorCode = parsed.code
        errorDetails = parsed.details
        errorHint = parsed.hint
      } catch {
        // 忽略 JSON 解析错误
      }

      // CSRF: clear cache, force-refresh, retry once (settings writers + others)
      if (
        response.status === 403 &&
        needsCSRF &&
        retryOnCSRFError &&
        errorBody &&
        isCsrfErrorBody(errorBody)
      ) {
        console.warn(
          '[apiService] CSRF rejection on',
          method,
          endpoint,
          '— refreshing token and retrying once',
        )
        clearCSRFToken()
        const newToken = await getCSRFToken(true)
        if (newToken) {
          return request<T>(endpoint, options, false)
        }
      }

      throw new ApiError(
        errorMessage,
        response.status,
        errorCode,
        errorDetails,
        errorHint,
      )
    }

    // 处理空响应
    const contentType = response.headers.get('content-type')
    if (contentType?.includes('application/json')) {
      return await response.json()
    }

    return {} as T
  } catch (error) {
    clearTimeout(timeoutId)

    if (error instanceof ApiError) {
      throw error
    }

    if (error instanceof Error && error.name === 'AbortError') {
      throw new ApiError('Request timeout', 408, 'TIMEOUT')
    }

    throw new ApiError(
      error instanceof Error ? error.message : 'Unknown error',
      0,
      'NETWORK_ERROR',
    )
  }
}

/** Binary GET (file download). Returns blob + optional Content-Disposition filename. */
async function requestBlob(
  endpoint: string,
  options: ApiRequestOptions = {},
): Promise<{ blob: Blob; filename?: string; contentType?: string }> {
  const {
    requireAuth: _requireAuth = false,
    timeout = 120000,
    params,
    ...fetchOptions
  } = options

  const headers: Record<string, string> = {
    ...(fetchOptions.headers as Record<string, string>),
  }
  // Do not force application/json Accept for binary bodies
  delete headers['Content-Type']

  const url = buildUrl(endpoint, params)
  const controller = new AbortController()
  const timeoutId = setTimeout(() => controller.abort(), timeout)

  try {
    const response = await fetch(url, {
      ...fetchOptions,
      method: 'GET',
      headers,
      credentials: 'include',
      signal: controller.signal,
    })
    clearTimeout(timeoutId)

    if (!response.ok) {
      let errorMessage = `API Error: ${response.status}`
      let errorCode: string | undefined
      let errorDetails: unknown
      let errorHint: string | undefined
      try {
        const parsed = parseApiErrorBody(await response.json(), response.status)
        errorMessage = parsed.message
        errorCode = parsed.code
        errorDetails = parsed.details
        errorHint = parsed.hint
      } catch {
        /* ignore */
      }
      throw new ApiError(
        errorMessage,
        response.status,
        errorCode,
        errorDetails,
        errorHint,
      )
    }

    const disposition = response.headers.get('content-disposition') || ''
    let filename: string | undefined
    const utf8Match = /filename\*=UTF-8''([^;]+)/i.exec(disposition)
    const plainMatch = /filename="([^"]+)"/i.exec(disposition)
    if (utf8Match?.[1]) {
      try {
        filename = decodeURIComponent(utf8Match[1])
      } catch {
        filename = utf8Match[1]
      }
    } else if (plainMatch?.[1]) {
      filename = plainMatch[1]
    }

    const blob = await response.blob()
    return {
      blob,
      filename,
      contentType: response.headers.get('content-type') || undefined,
    }
  } catch (error) {
    clearTimeout(timeoutId)
    if (error instanceof ApiError) throw error
    if (error instanceof Error && error.name === 'AbortError') {
      throw new ApiError('Request timeout', 408, 'TIMEOUT')
    }
    throw new ApiError(
      error instanceof Error ? error.message : 'Unknown error',
      0,
      'NETWORK_ERROR',
    )
  }
}

/**
 * API 服务实例
 */
export const apiService = {
  /**
   * GET 请求
   */
  get<T>(endpoint: string, options?: ApiRequestOptions): Promise<T> {
    return request<T>(endpoint, { ...options, method: 'GET' })
  },

  /**
   * GET binary body (downloads)
   */
  getBlob(
    endpoint: string,
    options?: ApiRequestOptions,
  ): Promise<{ blob: Blob; filename?: string; contentType?: string }> {
    return requestBlob(endpoint, options)
  },

  /**
   * POST 请求
   */
  post<T>(
    endpoint: string,
    data?: unknown,
    options?: ApiRequestOptions,
  ): Promise<T> {
    return request<T>(endpoint, {
      ...options,
      method: 'POST',
      body: data ? JSON.stringify(data) : undefined,
    })
  },

  /**
   * PUT 请求
   */
  put<T>(
    endpoint: string,
    data?: unknown,
    options?: ApiRequestOptions,
  ): Promise<T> {
    return request<T>(endpoint, {
      ...options,
      method: 'PUT',
      body: data ? JSON.stringify(data) : undefined,
    })
  },

  /**
   * PATCH 请求
   */
  patch<T>(
    endpoint: string,
    data?: unknown,
    options?: ApiRequestOptions,
  ): Promise<T> {
    return request<T>(endpoint, {
      ...options,
      method: 'PATCH',
      body: data ? JSON.stringify(data) : undefined,
    })
  },

  /**
   * DELETE 请求
   */
  delete<T>(endpoint: string, options?: ApiRequestOptions): Promise<T> {
    return request<T>(endpoint, { ...options, method: 'DELETE' })
  },
}

export default apiService
