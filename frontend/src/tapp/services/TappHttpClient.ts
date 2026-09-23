import { API_URL } from '../../config'
import { hostLocaleHeaders } from '../../i18n/hostLocaleHeaders'
import { currentCopy } from '../../i18n/localeCopy'
import { ApiError, apiService, parseApiErrorBody } from '../../services/api'
import { authSubject } from '../../utils/authSubject'
import { notifyHostSessionFailure } from '../../utils/hostSessionFailure'
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

  static from(error: ApiError): TappHttpError {
    return new TappHttpError(error.message, error.status, {
      retryAfter: error.retryAfter,
      body: error.body,
      code: error.code,
    })
  }
}

/**
 * TAPP endpoints over the shared client: CSRF, session-failure and rate-limit
 * handling are the host's. This layer adds only the runtime grant (with its
 * one-shot recovery) and no time budget — installs and store downloads are
 * long-running. Resolves the raw body; see `apiRequest` for the envelope.
 */
export async function tappRequest<T>(
  endpoint: string,
  options: ApiRequestOptions = {},
  retryOnRuntimeGrant: boolean = true,
): Promise<T> {
  const { runtimeGrant, headers, ...init } = options
  try {
    return await apiService.request<T>(endpoint.replace(/^\/api(?=\/)/, ''), {
      ...init,
      timeout: 0,
      headers: {
        ...(headers as Record<string, string> | undefined),
        ...(runtimeGrant ? { 'X-Tapp-Runtime-Grant': runtimeGrant } : {}),
      },
    })
  } catch (error) {
    if (!(error instanceof ApiError)) throw error
    if (
      error.status === 401 &&
      retryOnRuntimeGrant &&
      runtimeGrant &&
      error.code === 'INVALID_RUNTIME_GRANT'
    ) {
      const { TappRuntimeGrant } = await import('../runtime/TappRuntimeGrant')
      const replacement =
        await TappRuntimeGrant.recoverRejectedToken(runtimeGrant)
      if (replacement) {
        return tappRequest(endpoint, { ...options, runtimeGrant: replacement }, false)
      }
    }
    throw TappHttpError.from(error)
  }
}

/** `tappRequest` plus the standard `{ success, data }` envelope. */
export async function apiRequest<T>(
  endpoint: string,
  options: ApiRequestOptions = {},
): Promise<T> {
  const result = await tappRequest<unknown>(endpoint, options)
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
  const requestSubject = authSubject.signal
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
      error.code === 'INVALID_RUNTIME_GRANT'
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
    notifyHostSessionFailure(response.status, error, requestSubject)
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
