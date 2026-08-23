import type { AxiosError, AxiosRequestConfig, InternalAxiosRequestConfig } from 'axios'
import axios from 'axios'

import { API_URL } from '../config'
import { parseApiErrorBody } from '../services/api'
import { clearCSRFToken, getCSRFHeaderName, getCSRFToken } from '../utils/csrf'
import {
  formatRateLimitMessage,
  retryAfterSecondsFromBody,
} from '../utils/httpRateLimitToast'
import { checkRateLimit, RateLimitError } from '../utils/rateLimiter'
import TokenManager from '../utils/tokenManager'

// 智能 API URL 检测（与 config.ts 保持一致）
// 生产环境使用相对路径（空字符串），开发环境使用 localhost
const API_BASE_URL =
  API_URL || (typeof window !== 'undefined' ? window.location.origin : '')

// 验证 API URL 格式
function isValidUrl(url: string): boolean {
  // 空字符串是有效的（表示使用相对路径）
  if (url === '') {
    return true
  }
  try {
    const parsed = new URL(url)
    return parsed.protocol === 'http:' || parsed.protocol === 'https:'
  } catch {
    return false
  }
}

if (!isValidUrl(API_BASE_URL)) {
  throw new Error('Invalid API_BASE_URL configuration')
}

/** Mark a request as already retried after a CSRF failure (single retry only). */
type CsrfRetryableConfig = InternalAxiosRequestConfig & {
  __csrfRetried?: boolean
}

const MUTATING_METHODS = new Set(['post', 'put', 'patch', 'delete'])

/**
 * Detect CSRF rejection bodies from backend csrf middleware.
 * Matches: "CSRF token missing/expired/invalid/not found" and message text.
 */
export function isCsrfFailure(
  status: number,
  data: unknown,
): boolean {
  if (status !== 403) return false
  if (!data || typeof data !== 'object') return false
  const body = data as { error?: unknown; message?: unknown }
  const haystack = `${String(body.error ?? '')} ${String(body.message ?? '')}`.toLowerCase()
  return haystack.includes('csrf')
}

function extractApiErrorMessage(
  data: unknown,
  fallback: string,
): string {
  const parsed = parseApiErrorBody(data, 400)
  if (parsed.message && parsed.message !== 'API Error: 400') {
    return parsed.message
  }
  return fallback
}

/**
 * Assert a mutating config write succeeded.
 * With validateStatus accepting 4xx, callers must not treat error JSON as OK.
 */
export function assertConfigWriteSuccess(
  status: number,
  data: unknown,
  fallbackMessage: string,
): void {
  if (status >= 400) {
    throw new Error(extractApiErrorMessage(data, fallbackMessage))
  }
  if (
    data &&
    typeof data === 'object' &&
    'success' in data &&
    (data as { success: unknown }).success !== true
  ) {
    throw new Error(extractApiErrorMessage(data, fallbackMessage))
  }
}

const api = axios.create({
  baseURL: API_BASE_URL,
  headers: {
    'Content-Type': 'application/json',
  },
  timeout: 30000, // 30秒超时
  // Keep 4xx as fulfilled responses so callers (and CSRF retry) can inspect
  // body/status. Writers must still fail closed via assertConfigWriteSuccess.
  validateStatus: (status) => status < 500, // 只有5xx才算网络错误
  withCredentials: true, // 自动发送 HttpOnly Cookie
})

// Add request interceptor to include auth token
api.interceptors.request.use(
  async (config) => {
    // 异步获取 CSRF Token（从服务器）
    // 只有状态变更请求需要：后端 csrf_middleware 仅校验 POST/PUT/PATCH/DELETE。
    // Guest contract: GET /api/csrf-token returns 200 + csrf_token:null (not 401).
    // Still skip on GET to avoid wasted probe traffic on every read.
    if (MUTATING_METHODS.has(config.method?.toLowerCase() ?? 'get')) {
      const csrfToken = await getCSRFToken()
      if (csrfToken) {
        config.headers[getCSRFHeaderName()] = csrfToken
      }
    }

    // HttpOnly Cookie 用于身份验证（自动发送，无需手动添加）
    // TokenManager.getToken() 返回 null（HttpOnly Cookie 无法被 JS 读取）
    // Axios 通过 withCredentials: true 自动发送 Cookie

    // Rate Limiting 检查（仅针对修改操作）
    if (
      config.method &&
      ['post', 'put', 'patch', 'delete'].includes(config.method.toLowerCase())
    ) {
      const endpoint = config.url || ''

      if (endpoint.includes('/auth/login')) {
        if (!checkRateLimit(endpoint, 'login')) {
          return Promise.reject(
            new RateLimitError('登录尝试过于频繁，请稍后再试', 300000),
          )
        }
      } else if (endpoint.includes('/fetch')) {
        if (!checkRateLimit(endpoint, 'fetch')) {
          return Promise.reject(
            new RateLimitError('数据获取请求过于频繁，请稍后再试', 60000),
          )
        }
      } else if (endpoint.includes('/analysis')) {
        if (!checkRateLimit(endpoint, 'analysis')) {
          return Promise.reject(
            new RateLimitError('分析请求过于频繁，请稍后再试', 60000),
          )
        }
      } else {
        if (!checkRateLimit(endpoint, 'api')) {
          return Promise.reject(
            new RateLimitError('请求过于频繁，请稍后再试', 60000),
          )
        }
      }
    }

    return config
  },
  (error) => {
    return Promise.reject(error)
  },
)

/** Parse HTTP Retry-After (seconds, or HTTP-date) into milliseconds for RateLimitError. */
function retryAfterHeaderToMs(header: unknown, fallbackSeconds = 60): number {
  if (header == null || header === '') {
    return fallbackSeconds * 1000
  }
  const raw = String(header).trim()
  const asInt = Number.parseInt(raw, 10)
  // Numeric Retry-After is seconds (RFC 9110); treat reasonable values as seconds.
  if (Number.isFinite(asInt) && String(asInt) === raw) {
    return Math.max(1, asInt) * 1000
  }
  const when = Date.parse(raw)
  if (Number.isFinite(when)) {
    return Math.max(1000, when - Date.now())
  }
  return fallbackSeconds * 1000
}

/** Prefer Retry-After header, then body.retry_after, then 60s. */
function waitMsFrom429(
  headers: Record<string, unknown> | undefined,
  data: unknown,
  fallbackSeconds = 60,
): number {
  const header = headers?.['retry-after']
  if (header != null && header !== '') {
    return retryAfterHeaderToMs(header, fallbackSeconds)
  }
  const bodySec = retryAfterSecondsFromBody(data)
  if (bodySec != null) {
    return Math.max(1, bodySec) * 1000
  }
  return fallbackSeconds * 1000
}

function rateLimitErrorFromAxios(headers: unknown, data: unknown): RateLimitError {
  const hdrs = (headers || {}) as Record<string, unknown>
  const waitMs = waitMsFrom429(hdrs, data, 60)
  const waitSec = Math.ceil(waitMs / 1000)
  const serverMsg =
    data && typeof data === 'object' && typeof (data as { message?: unknown }).message === 'string'
      ? String((data as { message: string }).message)
      : null
  return new RateLimitError(formatRateLimitMessage(waitSec, serverMsg), waitMs)
}

// Response interceptor: CSRF + 429 on fulfilled path (validateStatus: status < 500),
// 401/network on rejected path.
api.interceptors.response.use(
  async (response) => {
    const config = response.config as CsrfRetryableConfig
    const method = config.method?.toLowerCase() ?? 'get'

    // 429 is "success" under validateStatus — handle here, not only in error branch.
    if (response.status === 429) {
      return Promise.reject(
        rateLimitErrorFromAxios(response.headers as Record<string, unknown>, response.data),
      )
    }

    if (
      MUTATING_METHODS.has(method) &&
      isCsrfFailure(response.status, response.data) &&
      !config.__csrfRetried
    ) {
      console.warn(
        '[api] CSRF rejection on',
        method.toUpperCase(),
        config.url,
        '— refreshing token and retrying once',
      )
      clearCSRFToken()
      const newToken = await getCSRFToken(true)
      if (newToken) {
        const retryConfig: CsrfRetryableConfig = {
          ...config,
          __csrfRetried: true,
        }
        retryConfig.headers = retryConfig.headers ?? {}
        retryConfig.headers[getCSRFHeaderName()] = newToken
        return api.request(retryConfig as AxiosRequestConfig)
      }
    }

    return response
  },
  (error: AxiosError | RateLimitError) => {
    // 处理 Rate Limit 错误
    if (error instanceof RateLimitError) {
      return Promise.reject(error)
    }

    // 处理 Axios 错误
    if (error.response?.status === 401) {
      // Token expired or invalid, clear it and redirect to login
      TokenManager.removeToken()
      clearCSRFToken()
      window.dispatchEvent(
        new CustomEvent('auth-state-changed', {
          detail: { isAuthenticated: false },
        }),
      )
    }

    // 429 can still land here if validateStatus is overridden on a call.
    if (error.response?.status === 429) {
      return Promise.reject(
        rateLimitErrorFromAxios(
          error.response.headers as Record<string, unknown>,
          error.response.data,
        ),
      )
    }

    return Promise.reject(error)
  },
)

// Configuration
// Setup 流程由 SetupWizard 直接 fetch，不经本模块。
export async function fetchConfig() {
  const response = await api.get('/api/config')
  return response.data
}

export async function updateConfig(config: any) {
  const response = await api.post('/api/config', config)
  assertConfigWriteSuccess(
    response.status,
    response.data,
    'Failed to save configuration',
  )
  return response.data
}

export async function fetchSettingsBackup() {
  const response = await api.get('/api/config/settings-backup')
  return response.data
}

export interface SettingsRestorePreview {
  backup_version: number
  current_version: number
  restore_count: number
  preserve_count: number
  ignored_count: number
  migrated_count: number
  invalid_count: number
  ignored_keys: string[]
  invalid_keys: string[]
}

export async function previewSettingsBackup(backup: unknown) {
  const response = await api.post('/api/config/settings-backup/preview', backup)
  if (response.status >= 400 || !response.data?.preview) {
    throw new Error(response.data?.error || 'Failed to preview settings backup')
  }
  return response.data.preview as SettingsRestorePreview
}

export async function restoreSettingsBackup(backup: unknown) {
  const response = await api.post('/api/config/settings-backup', backup)
  if (response.status >= 400 || response.data?.success !== true) {
    throw new Error(response.data?.error || 'Failed to restore settings backup')
  }
  return response.data
}

// Config Permissions
export async function fetchPermissionsConfig() {
  const response = await api.get('/api/config/permissions')
  return response.data
}

export async function updatePermissionsConfig(
  permissions: Record<string, boolean | number>,
) {
  const response = await api.post('/api/config/permissions', permissions)
  assertConfigWriteSuccess(
    response.status,
    response.data,
    'Failed to save permissions',
  )
  return response.data
}

// System
export async function reloadSystemConfig() {
  const response = await api.post('/api/system/reload-config')
  assertConfigWriteSuccess(
    response.status,
    response.data,
    'Failed to reload configuration',
  )
  return response.data
}

// Speech
export async function checkSpeechStatus() {
  const response = await api.get('/api/speech/status')
  return response.data
}

export interface SpeechTestResponse {
  success: boolean
  provider?: string
  tts_ok?: boolean
  asr_ok?: boolean
  tts_skipped?: boolean
  audio?: string
  transcript?: string
  error?: string
}

export async function testSpeechService(): Promise<SpeechTestResponse> {
  const response = await api.post('/api/speech/test')
  return response.data
}

export default api
