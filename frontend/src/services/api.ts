/**
 * 基础 API 服务
 *
 * 提供统一的 HTTP 请求封装
 */

import { API_URL } from '../config'
import { clearCSRFToken, getCSRFToken } from '../utils/csrf'

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
  ) {
    super(message)
    this.name = 'ApiError'
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
      let errorMessage = `API Error: ${response.status}`
      let errorCode: string | undefined
      let errorDetails: unknown
      let errorBody: { message?: unknown; error?: unknown; code?: string; details?: unknown } | null =
        null

      try {
        errorBody = await response.json()
        errorMessage =
          (typeof errorBody?.message === 'string' && errorBody.message) ||
          (typeof errorBody?.error === 'string' && errorBody.error) ||
          errorMessage
        errorCode = errorBody?.code
        errorDetails = errorBody?.details
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

      throw new ApiError(errorMessage, response.status, errorCode, errorDetails)
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
