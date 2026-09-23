import { API_URL } from '../config'
import { hostLocaleHeaders } from '../i18n/hostLocaleHeaders'
import { currentCopy } from '../i18n/localeCopy'
import { fetchWithAiConfiguration } from '../utils/aiConfiguration'
import { aiRequestTimeoutMs } from '../utils/aiRequestTimeout.mjs'
import { authSubject } from '../utils/authSubject'
import { awaitAbortable } from '../utils/awaitAbortable'
import { clearCSRFToken, getCSRFToken } from '../utils/csrf'
import { notifyHostSessionFailure } from '../utils/hostSessionFailure'
import {
  notifyHttpRateLimit,
  parseRetryAfterSeconds,
  retryAfterSecondsFromBody,
} from '../utils/httpRateLimitToast'
import { httpStatusMessage } from '../utils/httpStatus'

const API_BASE = `${API_URL}/api`

export interface ApiRequestOptions extends RequestInit {
  requireAuth?: boolean
  /** ms; 0 leaves the request unbounded (AI routes keep their own floor). */
  timeout?: number
  params?: Record<string, string | number | boolean | undefined>
  /** Resolve the success body as a Blob instead of JSON. */
  responseType?: 'json' | 'blob'
}

export class ApiError extends Error {
  constructor(
    message: string,
    public status: number,
    public code?: string,
    public details?: unknown,
    public hint?: string,
    /** Parsed error response body, when the server sent JSON. */
    public body?: unknown,
    /** Seconds the server asked us to wait (429 only). */
    public retryAfter?: number,
  ) {
    super(message)
    this.name = 'ApiError'
  }
}

/** Multipart and binary bodies go out as-is; the browser sets their Content-Type. */
function encodeBody(data: unknown): BodyInit | undefined {
  if (!data) return undefined
  if (data instanceof FormData || data instanceof Blob || data instanceof URLSearchParams) {
    return data
  }
  return JSON.stringify(data)
}

/** Machine codes: snake_case, SCREAMING_SNAKE, or short ALLCAPS. Not English labels. */
const STABLE_ERROR_CODE = /^(?:[A-Za-z][A-Za-z0-9]*_\w+|[A-Z][A-Z0-9]{2,64})$/

function readErrorString(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value.trim() : undefined
}

export function parseApiErrorBody(
  body: unknown,
  status: number,
): { message: string; code?: string; hint?: string; details?: unknown } {
  const fallback = httpStatusMessage(status)
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

/** Relative to the page when API_URL is empty; no dependency on `window`. */
function buildUrl(
  endpoint: string,
  params?: Record<string, string | number | boolean | undefined>,
): string {
  const url = `${API_BASE}${endpoint}`
  if (!params) return url
  const query = new URLSearchParams()
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined) query.append(key, String(value))
  }
  const search = query.toString()
  return search ? `${url}${url.includes('?') ? '&' : '?'}${search}` : url
}

function isCsrfErrorBody(body: {
  message?: unknown
  error?: unknown
}): boolean {
  const haystack =
    `${String(body.error ?? '')} ${String(body.message ?? '')}`.toLowerCase()
  return haystack.includes('csrf')
}

/** Gateway hiccups and dropped connections: safe to repeat only for reads. */
const TRANSIENT_STATUSES = new Set([502, 503, 504])
const TRANSIENT_RETRIES = 3

/** Longest Retry-After a read will quietly wait out before surfacing a 429. */
export const RATE_LIMIT_RETRY_MAX_MS = 8_000

/** How long to wait before repeating a rate-limited read, or null to give up. */
export function rateLimitRetryDelayMs(retryAfterSeconds: number | null): number | null {
  if (retryAfterSeconds == null) return 1_000
  const ms = Math.ceil(retryAfterSeconds * 1000)
  return ms <= RATE_LIMIT_RETRY_MAX_MS ? Math.max(ms, 250) : null
}

function sleep(ms: number, signal: AbortSignal): Promise<void> {
  return awaitAbortable(new Promise<void>(resolve => setTimeout(resolve, ms)), signal)
}

/**
 * Idempotent reads retry transient failures and wait out one short 429 window
 * (no red toast for a blip); everything else is sent once.
 */
async function dispatch(url: string, init: RequestInit, idempotent: boolean): Promise<Response> {
  const signal = init.signal!
  let waitedOutRateLimit = false
  for (let attempt = 0; ; attempt++) {
    const last = !idempotent || attempt >= TRANSIENT_RETRIES
    let response: Response
    try {
      response = await fetchWithAiConfiguration(url, init)
    } catch (error) {
      if (last || !(error instanceof TypeError) || signal.aborted) throw error
      await sleep(150 * (attempt + 1), signal)
      continue
    }
    if (idempotent && response.status === 429 && !waitedOutRateLimit) {
      const wait = rateLimitRetryDelayMs(parseRetryAfterSeconds(response))
      if (wait != null) {
        waitedOutRateLimit = true
        void response.body?.cancel().catch(() => {})
        await sleep(wait, signal)
        continue
      }
    }
    if (last || !TRANSIENT_STATUSES.has(response.status)) return response
    void response.body?.cancel().catch(() => {})
    await sleep(150 * (attempt + 1), signal)
  }
}

/** CSRF: retry once. */
async function request<T>(
  endpoint: string,
  options: ApiRequestOptions = {},
  retryOnCSRFError: boolean = true,
): Promise<T> {
  const requestSubject = authSubject.signal
  options.signal?.throwIfAborted()
  const {
    requireAuth: _requireAuth = false,
    timeout: timeoutOpt = 30000,
    params,
    responseType = 'json',
    ...fetchOptions
  } = options

  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...hostLocaleHeaders(),
    ...(fetchOptions.headers as Record<string, string>),
  }
  if (fetchOptions.body != null && typeof fetchOptions.body !== 'string') {
    delete headers['Content-Type']
  }

  const method = fetchOptions.method?.toUpperCase() || 'GET'
  const needsCSRF = ['POST', 'PUT', 'PATCH', 'DELETE'].includes(method)

  const url = buildUrl(endpoint, params)
  const timeout = Math.max(timeoutOpt, aiRequestTimeoutMs(url) ?? 0)

  const controller = new AbortController()
  const timeoutId = timeout > 0 ? setTimeout(() => controller.abort(), timeout) : undefined

  const signal = options.signal
    ? AbortSignal.any([options.signal, controller.signal])
    : controller.signal

  try {
    if (needsCSRF) {
      const csrfToken = await awaitAbortable(getCSRFToken(), signal)
      signal.throwIfAborted()
      if (csrfToken) headers['X-CSRF-Token'] = csrfToken
    }
    const response = await dispatch(url, {
      ...fetchOptions,
      headers,
      credentials: 'include',
      signal,
    }, method === 'GET' || method === 'HEAD')

    options.signal?.throwIfAborted()

    if (!response.ok) {
      let errorMessage = httpStatusMessage(response.status)
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
        controller.signal.throwIfAborted()
        options.signal?.throwIfAborted()
      }

      notifyHttpRateLimit(response, errorBody)
      notifyHostSessionFailure(response.status, errorBody, requestSubject)

      // CSRF: force-refresh and retry once.
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
        const newToken = await awaitAbortable(getCSRFToken(true), signal)
        signal.throwIfAborted()
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
        errorBody ?? undefined,
        response.status === 429
          ? (parseRetryAfterSeconds(response) ?? retryAfterSecondsFromBody(errorBody) ?? undefined)
          : undefined,
      )
    }

    if (responseType === 'blob') return (await response.blob()) as T

    const contentType = response.headers.get('content-type')
    if (contentType?.includes('application/json')) {
      return await response.json()
    }

    return {} as T
  } catch (error) {
    clearTimeout(timeoutId)
    options.signal?.throwIfAborted()

    if (error instanceof ApiError) {
      throw error
    }

    if (error instanceof Error && error.name === 'AbortError') {
      throw new ApiError(currentCopy().errors.timeout, 408, 'TIMEOUT')
    }

    throw new ApiError(currentCopy().errors.networkError, 0, 'NETWORK_ERROR')
  } finally {
    clearTimeout(timeoutId)
  }
}

async function requestBlob(
  endpoint: string,
  options: ApiRequestOptions = {},
): Promise<{ blob: Blob; filename?: string; contentType?: string }> {
  options.signal?.throwIfAborted()
  const {
    requireAuth: _requireAuth = false,
    timeout = 120000,
    params,
    ...fetchOptions
  } = options

  const headers: Record<string, string> = {
    ...hostLocaleHeaders(),
    ...(fetchOptions.headers as Record<string, string>),
  }
  // Do not force Accept: application/json on binary GET.
  delete headers['Content-Type']

  const url = buildUrl(endpoint, params)
  const controller = new AbortController()
  const timeoutId = setTimeout(() => controller.abort(), timeout)

  try {
    const response = await fetchWithAiConfiguration(url, {
      ...fetchOptions,
      method: 'GET',
      headers,
      credentials: 'include',
      signal: options.signal
        ? AbortSignal.any([options.signal, controller.signal])
        : controller.signal,
    })
    options.signal?.throwIfAborted()

    if (!response.ok) {
      let errorMessage = httpStatusMessage(response.status)
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
        controller.signal.throwIfAborted()
        options.signal?.throwIfAborted()
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
    options.signal?.throwIfAborted()
    if (error instanceof ApiError) throw error
    if (error instanceof Error && error.name === 'AbortError') {
      throw new ApiError(currentCopy().errors.timeout, 408, 'TIMEOUT')
    }
    throw new ApiError(currentCopy().errors.networkError, 0, 'NETWORK_ERROR')
  } finally {
    clearTimeout(timeoutId)
  }
}

export const apiService = {
  /** Escape hatch for callers that build their own RequestInit (method, raw body). */
  request<T>(endpoint: string, options?: ApiRequestOptions): Promise<T> {
    return request<T>(endpoint, options)
  },

  get<T>(endpoint: string, options?: ApiRequestOptions): Promise<T> {
    return request<T>(endpoint, { ...options, method: 'GET' })
  },

  getBlob(
    endpoint: string,
    options?: ApiRequestOptions,
  ): Promise<{ blob: Blob; filename?: string; contentType?: string }> {
    return requestBlob(endpoint, options)
  },

  post<T>(
    endpoint: string,
    data?: unknown,
    options?: ApiRequestOptions,
  ): Promise<T> {
    return request<T>(endpoint, {
      ...options,
      method: 'POST',
      body: encodeBody(data),
    })
  },

  put<T>(
    endpoint: string,
    data?: unknown,
    options?: ApiRequestOptions,
  ): Promise<T> {
    return request<T>(endpoint, {
      ...options,
      method: 'PUT',
      body: encodeBody(data),
    })
  },

  patch<T>(
    endpoint: string,
    data?: unknown,
    options?: ApiRequestOptions,
  ): Promise<T> {
    return request<T>(endpoint, {
      ...options,
      method: 'PATCH',
      body: encodeBody(data),
    })
  },

  delete<T>(endpoint: string, options?: ApiRequestOptions): Promise<T> {
    return request<T>(endpoint, { ...options, method: 'DELETE' })
  },
}

export default apiService
