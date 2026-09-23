import { hostLocaleHeaders } from '../i18n/hostLocaleHeaders'
import { currentCopy, formatCurrent } from '../i18n/localeCopy'
import { ApiError, parseApiErrorBody } from '../services/api'
import { fetchWithAiConfiguration } from './aiConfiguration'
import { withAiTimeoutSignal } from './aiRequestTimeout.mjs'
import { parseRetryAfterSeconds, retryAfterSecondsFromBody } from './httpRateLimitToast'
import { httpStatusMessage } from './httpStatus'
import { isUselessErrorText } from './uselessErrorText'

function isResponseCancellation(error: unknown): boolean {
  return error instanceof Error && (error.name === 'AbortError' || error.name === 'TimeoutError')
}

export async function parseJsonResponse(response: Response): Promise<any> {
  const contentType = response.headers.get('content-type')
  const hasJson = contentType && contentType.includes('application/json')

  if (!hasJson) {
    const text = await response.text()
    throw new Error(
      text || response.statusText || httpStatusMessage(response.status),
    )
  }

  try {
    return await response.json()
  } catch (error) {
    if (isResponseCancellation(error)) throw error
    throw new Error(
      formatCurrent(currentCopy().errors.invalidResponse, {
        status: response.status,
      }),
    )
  }
}

export async function handleErrorResponse(
  response: Response,
  defaultMessage: string = currentCopy().errors.requestFailed,
): Promise<never> {
  const contentType = response.headers.get('content-type')
  const hasJson = contentType && contentType.includes('application/json')

  if (hasJson) {
    try {
      const body = await response.json()
      const parsed = parseApiErrorBody(body, response.status)
      const message =
        !isUselessErrorText(parsed.message)
          ? parsed.message
          : defaultMessage
      throw new ApiError(
        message,
        response.status,
        parsed.code,
        parsed.details,
        parsed.hint,
        body,
        response.status === 429
          ? (parseRetryAfterSeconds(response) ?? retryAfterSecondsFromBody(body) ?? undefined)
          : undefined,
      )
    } catch (error) {
      if (error instanceof ApiError || isResponseCancellation(error)) throw error
      throw new ApiError(
        `${defaultMessage} (${response.status})`,
        response.status,
      )
    }
  }

  const text = await response.text()
  throw new ApiError(
    text || response.statusText || httpStatusMessage(response.status),
    response.status,
  )
}

/** Parse JSON after a successful response; keep server message/code on failure. */
export async function readJsonOk(
  response: Response,
  defaultMessage: string = currentCopy().errors.requestFailed,
): Promise<any> {
  if (!response.ok) {
    await handleErrorResponse(response, defaultMessage)
  }
  return parseJsonResponse(response)
}

const TRANSIENT_STATUSES = new Set([502, 503, 504])
const MAX_RETRIES = 3

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

function isIdempotent(method?: string): boolean {
  const m = (method ?? 'GET').toUpperCase()
  return m === 'GET' || m === 'HEAD'
}

/**
 * Pre-session auth flows only (login, registration). Everything else uses
 * `apiService`, whose 401 handling asks the host to re-validate its session —
 * the wrong reaction to a mistyped password.
 */
export async function fetchJson<T = any>(
  url: string,
  options?: RequestInit,
  errorMessage: string = currentCopy().errors.requestFailed,
): Promise<T> {
  const retryable = isIdempotent(options?.method)

  for (let attempt = 0; ; attempt++) {
    try {
      const response = await fetchWithAiConfiguration(
        url,
        withAiTimeoutSignal(url, {
          credentials: 'include',
          ...options,
          headers: {
            ...hostLocaleHeaders(),
            ...(options?.headers as Record<string, string> | undefined),
          },
        }),
      )

      if (
        retryable &&
        TRANSIENT_STATUSES.has(response.status) &&
        attempt < MAX_RETRIES
      ) {
        await sleep(150 * (attempt + 1))
        continue
      }

      if (!response.ok) {
        await handleErrorResponse(response, errorMessage)
      }

      return await parseJsonResponse(response)
    } catch (error) {
      const isNetworkError = error instanceof TypeError
      if (retryable && isNetworkError && attempt < MAX_RETRIES) {
        await sleep(150 * (attempt + 1))
        continue
      }
      if (error instanceof Error) {
        throw error
      }
      throw new Error(errorMessage)
    }
  }
}
