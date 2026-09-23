import type { AxiosError, AxiosRequestConfig, InternalAxiosRequestConfig } from 'axios'
import type { UnresolvedRestoredMedia } from './settingsRestoreMedia'
import axios from 'axios'

import { API_URL } from '../config'
import { hostLocaleHeaders } from '../i18n/hostLocaleHeaders'
import { currentCopy } from '../i18n/localeCopy'
import { parseApiErrorBody } from '../services/api'
import { checkAiConfiguration } from '../utils/aiConfiguration'
import { aiRequestTimeoutMs } from '../utils/aiRequestTimeout.mjs'
import { authSubject } from '../utils/authSubject'
import { clearCSRFToken, getCSRFHeaderName, getCSRFToken } from '../utils/csrf'
import { notifyHostSessionFailure } from '../utils/hostSessionFailure'
import {
  formatRateLimitMessage,
  retryAfterSecondsFromBody,
} from '../utils/httpRateLimitToast'
import { invalidatePermissionConfig } from '../utils/permissionConfig'
import { checkRateLimit, RateLimitError } from '../utils/rateLimiter'
import { isUselessErrorText, userFacingError } from '../utils/userFacingError'
import { parseUnresolvedRestoredMedia } from './settingsRestoreMedia'

const API_BASE_URL =
  API_URL || (typeof window !== 'undefined' ? window.location.origin : '')

function isValidUrl(url: string): boolean {
  // Empty string is a valid same-origin /api base.
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

/** CSRF retry once. */
type CsrfRetryableConfig = InternalAxiosRequestConfig & {
  __csrfRetried?: boolean
  __authSubject?: AbortSignal
}

const MUTATING_METHODS = new Set(['post', 'put', 'patch', 'delete'])

/** CSRF 403 bodies from csrf middleware. */
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
  const raw =
    parsed.message && parsed.message !== 'API Error: 400' ? parsed.message : ''
  return userFacingError(raw, fallback)
}

/** A successful HTTP response can still reject a configuration write. */
function assertConfigWriteSuccess(
  data: unknown,
  fallbackMessage: string,
): void {
  if (
    data &&
    typeof data === 'object' &&
    Object.hasOwn(data, 'success') &&
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
  timeout: 30000,
  withCredentials: true,
})

api.interceptors.request.use(
  async (config) => {
    (config as CsrfRetryableConfig).__authSubject ??= authSubject.signal
    const blocked = await checkAiConfiguration(config.url || '', {
      method: config.method,
      body: typeof config.data === 'string' ? config.data : JSON.stringify(config.data),
      signal: config.signal as AbortSignal | undefined,
    })
    if (blocked) {
      const data = blocked.headers.get('content-type')?.includes('application/json')
        ? await blocked.json() : { error: await blocked.text() }
      throw new axios.AxiosError(data.error, 'ERR_BAD_REQUEST', config, undefined, {
        data, status: blocked.status, statusText: blocked.statusText, headers: {}, config,
      })
    }
    const aiTimeoutMs = aiRequestTimeoutMs(config.url || '')
    if (aiTimeoutMs) {
      config.timeout = Math.max(config.timeout ?? 0, aiTimeoutMs)
    }
    for (const [key, value] of Object.entries(hostLocaleHeaders())) {
      config.headers.set(key, value)
    }
    // CSRF only on POST/PUT/PATCH/DELETE. GET /api/csrf-token is 200 + csrf_token:null (not 401).
    if (MUTATING_METHODS.has(config.method?.toLowerCase() ?? 'get')) {
      const csrfToken = await getCSRFToken()
      if (csrfToken) {
        config.headers[getCSRFHeaderName()] = csrfToken
      }
    }

    if (
      config.method &&
      ['post', 'put', 'patch', 'delete'].includes(config.method.toLowerCase())
    ) {
      const endpoint = config.url || ''

      if (endpoint.includes('/auth/login')) {
        if (!checkRateLimit(endpoint, 'login')) {
          return Promise.reject(
            new RateLimitError(currentCopy().errors.rateLimitedLogin, 300000),
          )
        }
      } else if (endpoint.includes('/fetch')) {
        if (!checkRateLimit(endpoint, 'fetch')) {
          return Promise.reject(
            new RateLimitError(currentCopy().errors.rateLimitedFetch, 60000),
          )
        }
      } else if (endpoint.includes('/analysis')) {
        if (!checkRateLimit(endpoint, 'analysis')) {
          return Promise.reject(
            new RateLimitError(currentCopy().errors.rateLimitedAnalysis, 60000),
          )
        }
      } else {
        if (!checkRateLimit(endpoint, 'api')) {
          return Promise.reject(
            new RateLimitError(currentCopy().errors.rateLimited, 60000),
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

function retryAfterHeaderToMs(header: unknown, fallbackSeconds = 60): number {
  if (header == null || header === '') {
    return fallbackSeconds * 1000
  }
  const raw = String(header).trim()
  const asInt = Number.parseInt(raw, 10)
  // Numeric Retry-After is seconds (RFC 9110).
  if (Number.isFinite(asInt) && String(asInt) === raw) {
    return Math.max(1, asInt) * 1000
  }
  const when = Date.parse(raw)
  if (Number.isFinite(when)) {
    return Math.max(1000, when - Date.now())
  }
  return fallbackSeconds * 1000
}

/** Retry-After header, then body.retry_after, else 60s. */
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

api.interceptors.response.use(
  (response) => response,
  async (error: AxiosError | RateLimitError) => {
    if (error instanceof RateLimitError) throw error

    const response = error.response
    const config = error.config as CsrfRetryableConfig | undefined
    const method = config?.method?.toLowerCase() ?? 'get'
    if (
      response &&
      config &&
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

    if (response && config?.__authSubject) {
      notifyHostSessionFailure(response.status, response.data, config.__authSubject)
    }

    if (error.response?.status === 429) {
      return Promise.reject(
        rateLimitErrorFromAxios(
          error.response.headers as Record<string, unknown>,
          error.response.data,
        ),
      )
    }

    if (response) {
      error.message = parseApiErrorBody(response.data, response.status).message
    }
    throw error
  },
)

// SetupWizard fetches setup itself; not this module.
export async function fetchConfig() {
  const response = await api.get('/api/config')
  return response.data
}

export async function updateConfig(config: any) {
  const response = await api.post('/api/config', config)
  assertConfigWriteSuccess(
    response.data,
    currentCopy().errors.configSaveFailed,
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
  if (!response.data?.preview) {
    const previewError =
      typeof response.data?.error === 'string' ? response.data.error : ''
    throw new Error(
      previewError && !isUselessErrorText(previewError)
        ? previewError
        : currentCopy().errors.settingsBackupPreviewFailed,
    )
  }
  return response.data.preview as SettingsRestorePreview
}

export interface SettingsRestoreResult {
  success: true
  message?: string
  requires_reload?: boolean
  preview?: SettingsRestorePreview
  /** Local media the backup cites that does not exist here; left unbound. */
  unresolved_media: UnresolvedRestoredMedia[]
}

export async function restoreSettingsBackup(
  backup: unknown,
): Promise<SettingsRestoreResult> {
  const response = await api.post('/api/config/settings-backup', backup)
  if (response.data?.success !== true) {
    const restoreError =
      typeof response.data?.error === 'string' ? response.data.error : ''
    throw new Error(
      restoreError && !isUselessErrorText(restoreError)
        ? restoreError
        : currentCopy().errors.settingsBackupRestoreFailed,
    )
  }
  invalidatePermissionConfig()
  return {
    ...response.data,
    unresolved_media: parseUnresolvedRestoredMedia(
      response.data.unresolved_media,
    ),
  } as SettingsRestoreResult
}

export async function fetchPermissionsConfig() {
  const response = await api.get('/api/config/permissions')
  return response.data
}

export async function updatePermissionsConfig(
  permissions: Record<string, boolean | number>,
) {
  const response = await api.post('/api/config/permissions', permissions)
  assertConfigWriteSuccess(
    response.data,
    currentCopy().errors.configSaveFailed,
  )
  invalidatePermissionConfig()
  return response.data
}

export async function reloadSystemConfig() {
  const response = await api.post('/api/system/reload-config')
  assertConfigWriteSuccess(
    response.data,
    currentCopy().errors.configReloadFailed,
  )
  invalidatePermissionConfig()
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
