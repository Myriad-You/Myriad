import { currentCopy, formatCurrent } from '../i18n/localeCopy'
import { showToast } from './toastManager'

const recentToastAt = { t: 0 }
const TOAST_COOLDOWN_MS = 4000

/** Retry-After: seconds or HTTP-date. */
export function parseRetryAfterSeconds(response: Response): number | null {
  const raw = response.headers.get('Retry-After')
  if (!raw) return null
  const asInt = Number.parseInt(raw, 10)
  if (Number.isFinite(asInt) && asInt >= 0) {
    return Math.min(asInt, 3600)
  }
  const when = Date.parse(raw)
  if (Number.isFinite(when)) {
    const sec = Math.ceil((when - Date.now()) / 1000)
    return sec > 0 ? Math.min(sec, 3600) : 0
  }
  return null
}

/** Body retry_after (s) if header missing. */
export function retryAfterSecondsFromBody(data: unknown): number | null {
  if (!data || typeof data !== 'object') return null
  const ra = (data as { retry_after?: unknown }).retry_after
  if (typeof ra === 'number' && Number.isFinite(ra) && ra >= 0) {
    return Math.min(Math.ceil(ra), 3600)
  }
  if (typeof ra === 'string') {
    const asInt = Number.parseInt(ra, 10)
    if (Number.isFinite(asInt) && asInt >= 0) return Math.min(asInt, 3600)
  }
  return null
}

/** Do not let Retry-After copy override the UI locale. */
export function formatRateLimitMessage(
  seconds: number,
  serverMessage?: string | null,
): string {
  const sec = Number.isFinite(seconds) && seconds > 0 ? Math.ceil(seconds) : 60
  const msg = serverMessage?.trim() ?? ''
  if (
    msg &&
    !/rate limit exceeded|too many requests|please try again/i.test(msg)
  ) {
    return msg
  }
  return formatCurrent(currentCopy().errors.rateLimitedRetry, { sec })
}

export function notifyHttpRateLimit(
  response: Response,
  body?: unknown,
): boolean {
  if (response.status !== 429) return false
  const seconds =
    parseRetryAfterSeconds(response) ??
    retryAfterSecondsFromBody(body) ??
    60
  const serverMsg =
    body && typeof body === 'object'
      ? String(
          (body as { message?: unknown }).message ??
            '',
        ) || null
      : null
  const now = Date.now()
  if (now - recentToastAt.t < TOAST_COOLDOWN_MS) return true
  recentToastAt.t = now
  showToast({
    message: formatRateLimitMessage(seconds, serverMsg),
    type: 'warning',
    duration: Math.min(Math.max(seconds, 3) * 1000, 12_000),
  })
  return true
}
