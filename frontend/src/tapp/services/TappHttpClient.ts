import { API_URL } from '../../config'
import { hostLocaleHeaders } from '../../i18n/hostLocaleHeaders'
import { currentCopy } from '../../i18n/localeCopy'
import { parseApiErrorBody } from '../../services/api'
import { getCSRFToken } from '../../utils/csrf'
import {
  notifyHttpRateLimit,
  parseRetryAfterSeconds,
  retryAfterSecondsFromBody,
} from '../../utils/httpRateLimitToast'
import { userFacingError } from '../../utils/userFacingError'

export interface ApiRequestOptions extends RequestInit {
  /** 仅宿主运行时身份；不进沙箱。 */
  runtimeGrant?: string
}

export class TappHttpError extends Error {
  readonly status: number
  readonly code?: string
  readonly retryAfter?: number
  readonly body?: unknown

  constructor(
    message: string,
    status: number,
    opts?: { retryAfter?: number; body?: unknown; code?: string },
  ) {
    super(message)
    this.name = 'TappHttpError'
    this.status = status
    this.code = opts?.code
    this.retryAfter = opts?.retryAfter
    this.body = opts?.body
  }
}

export async function apiRequest<T>(
  endpoint: string,
  options: ApiRequestOptions = {},
  retryOnCsrf: boolean = true,
  retryOnRuntimeGrant: boolean = true,
): Promise<T> {
  const { runtimeGrant, ...fetchOptions } = options
  const method = (options.method || 'GET').toUpperCase()
  const needsCsrf =
    method !== 'GET' && method !== 'HEAD' && method !== 'OPTIONS'
  const csrfToken = needsCsrf ? (await getCSRFToken()) || '' : ''

  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...hostLocaleHeaders(),
    ...(options.headers as Record<string, string>),
  }

  if (runtimeGrant) headers['X-Tapp-Runtime-Grant'] = runtimeGrant
  if (needsCsrf && csrfToken) headers['X-CSRF-Token'] = csrfToken

  const response = await fetch(`${API_URL}${endpoint}`, {
    ...fetchOptions,
    headers,
    credentials: 'include',
  })

  if (!response.ok) {
    const errorData = await response.json().catch(() => ({}))
    if (response.status === 429) {
      notifyHttpRateLimit(response, errorData)
    }
    if (
      response.status === 403 &&
      retryOnCsrf &&
      (errorData.error?.includes('CSRF') || errorData.error?.includes('csrf'))
    ) {
      await getCSRFToken(true)
      return apiRequest(endpoint, options, false, retryOnRuntimeGrant)
    }

    if (
      response.status === 401 &&
      retryOnRuntimeGrant &&
      runtimeGrant &&
      (errorData.code === 'INVALID_RUNTIME_GRANT' ||
        errorData.code === 'RUNTIME_GRANT_SUBJECT_MISMATCH')
    ) {
      const { TappRuntimeGrant } = await import('../runtime/TappRuntimeGrant')
      const replacement =
        await TappRuntimeGrant.recoverRejectedToken(runtimeGrant)
      if (replacement) {
        return apiRequest(
          endpoint,
          { ...options, runtimeGrant: replacement },
          retryOnCsrf,
          false,
        )
      }
    }

    const parsed = parseApiErrorBody(errorData, response.status)
    const retryAfter =
      response.status === 429
        ? (parseRetryAfterSeconds(response) ??
          retryAfterSecondsFromBody(errorData) ??
          undefined)
        : undefined
    throw new TappHttpError(parsed.message, response.status, {
      retryAfter,
      body: errorData,
      code: parsed.code,
    })
  }

  const result = await response.json()
  if (
    typeof result === 'object' &&
    result !== null &&
    Object.hasOwn(result, 'success')
  ) {
    const payload = result as { success: unknown; error?: unknown; data?: T }
    if (!payload.success) {
      throw new Error(
        (typeof payload.error === 'string' && payload.error.trim()) ||
          currentCopy().errors.requestFailed,
      )
    }
    if (Object.hasOwn(payload, 'data')) return payload.data as T
    return result as T
  }
  return result as T
}

export async function streamRuntimeEvents(
  endpoint: string,
  runtimeGrant: string,
  onEvent: (event: string, data: unknown) => void,
  signal?: AbortSignal,
  retryOnRuntimeGrant: boolean = true,
): Promise<void> {
  const response = await fetch(`${API_URL}${endpoint}`, {
    headers: {
      Accept: 'text/event-stream',
      'X-Tapp-Runtime-Grant': runtimeGrant,
      ...hostLocaleHeaders(),
    },
    credentials: 'include',
    signal,
  })
  if (!response.ok) {
    const error = await response.json().catch(() => ({}))
    if (
      response.status === 401 &&
      retryOnRuntimeGrant &&
      (error.code === 'INVALID_RUNTIME_GRANT' ||
        error.code === 'RUNTIME_GRANT_SUBJECT_MISMATCH')
    ) {
      const { TappRuntimeGrant } = await import('../runtime/TappRuntimeGrant')
      const replacement =
        await TappRuntimeGrant.recoverRejectedToken(runtimeGrant)
      if (replacement) {
        return streamRuntimeEvents(
          endpoint,
          replacement,
          onEvent,
          signal,
          false,
        )
      }
    }
    const parsed = parseApiErrorBody(error, response.status)
    throw new TappHttpError(
      userFacingError(
        parsed.message === `API Error: ${response.status}`
          ? `HTTP ${response.status}`
          : parsed.message,
        currentCopy().errors.streamUnreadable,
      ),
      response.status,
      { body: error, code: parsed.code },
    )
  }
  if (!response.body)
    throw new Error(currentCopy().errors.streamUnreadable)

  const reader = response.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''
  while (true) {
    const { done, value } = await reader.read()
    buffer += decoder.decode(value, { stream: !done }).replaceAll('\r\n', '\n')
    let boundary = buffer.indexOf('\n\n')
    while (boundary >= 0) {
      const block = buffer.slice(0, boundary)
      buffer = buffer.slice(boundary + 2)
      let eventName = 'message'
      const data: string[] = []
      for (const line of block.split('\n')) {
        if (line.startsWith('event:')) eventName = line.slice(6).trim()
        if (line.startsWith('data:')) data.push(line.slice(5).trimStart())
      }
      if (data.length > 0) {
        const raw = data.join('\n')
        let parsed: unknown = raw
        try {
          parsed = JSON.parse(raw)
        } catch {
        }
        onEvent(eventName, parsed)
      }
      boundary = buffer.indexOf('\n\n')
    }
    if (done) break
  }
}
