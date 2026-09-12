/** 定向失效：local widgetId + storage:write + 宿主预算，避免当成重挂炮。 */

export const WIDGET_INVALIDATE_TARGET_COOLDOWN_MS = 15_000
export const WIDGET_INVALIDATE_TARGET_TAPP_MAX_PER_MINUTE = 2
export const WIDGET_INVALIDATE_REASON_MAX_LEN = 256
const MAX_LOCAL_WIDGET_ID_LEN = 128
const TAPP_WINDOW_MS = 60_000

export type WidgetInvalidateTargetParse =
  | { ok: true; reason: string; widgetId: string }
  | { ok: false; error: string }

export type WidgetInvalidateTargetAccept =
  | { ok: true }
  | { ok: false; reason: 'cooldown' | 'tapp-budget'; retryAfterMs: number }

const lastAcceptedByWidget = new Map<string, number>()
const acceptedAtByTapp = new Map<string, number[]>()

export function isSafeLocalWidgetId(widgetId: string): boolean {
  if (!widgetId || widgetId.length > MAX_LOCAL_WIDGET_ID_LEN) return false
  if (widgetId === '.' || widgetId === '..' || widgetId.startsWith('.')) {
    return false
  }
  return /^[\w.-]+$/.test(widgetId)
}

export function isLocalWidgetIdOfTapp(
  widgetId: string,
  manifestWidgetIds: readonly string[],
  registeredLocalIds: readonly string[],
): boolean {
  return (
    manifestWidgetIds.includes(widgetId) ||
    registeredLocalIds.includes(widgetId)
  )
}

export function parseWidgetInvalidateTargetArgs(
  args: unknown[],
): WidgetInvalidateTargetParse {
  const [rawReason, rawOptions] = args
  const reason =
    typeof rawReason === 'string'
      ? rawReason.slice(0, WIDGET_INVALIDATE_REASON_MAX_LEN)
      : 'requested'

  if (rawOptions == null) {
    return {
      ok: false,
      error:
        'Page/headless invalidate requires { target: { widgetId } }; omit options only in the Widget sandbox',
    }
  }
  if (
    typeof rawOptions !== 'object' ||
    Array.isArray(rawOptions) ||
    Object.getPrototypeOf(rawOptions) !== Object.prototype
  ) {
    return { ok: false, error: 'invalidate options must be a plain object' }
  }

  const target = (rawOptions as { target?: unknown }).target
  if (target === 'all') {
    return {
      ok: false,
      error:
        'target "all" is not supported; write Tapp.storage to refresh every visible widget',
    }
  }
  if (
    !target ||
    typeof target !== 'object' ||
    Array.isArray(target) ||
    Object.getPrototypeOf(target) !== Object.prototype
  ) {
    return { ok: false, error: 'options.target must be { widgetId }' }
  }

  const widgetId = (target as { widgetId?: unknown }).widgetId
  if (typeof widgetId !== 'string' || !isSafeLocalWidgetId(widgetId)) {
    return {
      ok: false,
      error:
        "target.widgetId must be this Tapp's local widget id (letters, numbers, dots, underscores, hyphens)",
    }
  }

  return { ok: true, reason, widgetId }
}

export function tryAcceptWidgetInvalidateTarget(
  tappId: string,
  widgetId: string,
  now = Date.now(),
): WidgetInvalidateTargetAccept {
  pruneTappWindow(tappId, now)

  const widgetKey = `${tappId}\0${widgetId}`
  const last = lastAcceptedByWidget.get(widgetKey)
  if (last !== undefined) {
    const retryAfterMs = last + WIDGET_INVALIDATE_TARGET_COOLDOWN_MS - now
    if (retryAfterMs > 0) {
      return { ok: false, reason: 'cooldown', retryAfterMs }
    }
  }

  const accepted = acceptedAtByTapp.get(tappId) ?? []
  if (accepted.length >= WIDGET_INVALIDATE_TARGET_TAPP_MAX_PER_MINUTE) {
    const oldest = accepted[0] ?? now
    return {
      ok: false,
      reason: 'tapp-budget',
      retryAfterMs: Math.max(1, oldest + TAPP_WINDOW_MS - now),
    }
  }

  lastAcceptedByWidget.set(widgetKey, now)
  accepted.push(now)
  acceptedAtByTapp.set(tappId, accepted)
  return { ok: true }
}

export function resetWidgetInvalidateTargetRateLimitForTests(): void {
  lastAcceptedByWidget.clear()
  acceptedAtByTapp.clear()
}

function pruneTappWindow(tappId: string, now: number): void {
  const cutoff = now - TAPP_WINDOW_MS
  const stamps = acceptedAtByTapp.get(tappId)
  if (!stamps) return
  const next = stamps.filter((stamp) => stamp > cutoff)
  if (next.length === 0) acceptedAtByTapp.delete(tappId)
  else acceptedAtByTapp.set(tappId, next)
}
