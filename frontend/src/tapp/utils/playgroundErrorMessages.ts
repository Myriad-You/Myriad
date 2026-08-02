/**
 * Map raw Tapp Playground generate / stream errors to localized, actionable copy.
 * Backend messages are English; this layer classifies them and keeps a truncated
 * technical detail when it helps debugging (validation failures, etc.).
 */

export interface PlaygroundErrorCopy {
  playgroundTimeoutHint: string
  playgroundServerErrorHint: string
  playgroundGenerateFailed: string
  playgroundCancelled?: string
  playgroundAiNotConfiguredHint: string
  playgroundAiGenerationFailedHint: string
  playgroundValidationFailedHint: string
  playgroundPayloadTooLargeHint: string
  playgroundAdminRequiredHint: string
  playgroundAuthRequiredHint: string
  playgroundRateLimitHint: string
  playgroundNetworkHint: string
  playgroundStreamIncompleteHint: string
  playgroundAgentBusyHint: string
  playgroundBadRequestHint: string
  playgroundErrorDetail: string
  playgroundRuntimeError: string
}

export interface MapPlaygroundErrorOpts {
  userCancelled?: boolean
  /** Replace `{key}` in copy templates that need a detail fragment. */
  format?: (template: string, params: Record<string, string | number>) => string
}

const DETAIL_MAX = 720

function defaultFormat(
  template: string,
  params: Record<string, string | number>,
): string {
  return Object.entries(params).reduce(
    (s, [k, v]) => s.replaceAll(`{${k}}`, String(v)),
    template,
  )
}

function truncateDetail(text: string, max = DETAIL_MAX): string {
  const t = text.replace(/\s+/g, ' ').trim()
  if (t.length <= max) return t
  return `${t.slice(0, max - 1)}…`
}

function compose(
  primary: string,
  detail: string | undefined,
  detailTemplate: string,
  format: (template: string, params: Record<string, string | number>) => string,
): string {
  const d = detail ? truncateDetail(detail) : ''
  if (!d || d === primary || primary.includes(d)) return primary
  if (detailTemplate.includes('{detail}')) {
    return `${primary}\n${format(detailTemplate, { detail: d })}`
  }
  return `${primary}\n${d}`
}

/** Pull the human-useful tail after "after N attempts:" etc. */
function extractValidationDetail(raw: string): string {
  const m = raw.match(
    /did not pass validation after \d+ attempts?: (.+)$/i,
  )
  if (m?.[1]) return m[1].trim()
  const colon = raw.indexOf(': ')
  if (colon > 0 && /validation/i.test(raw.slice(0, colon))) {
    return raw.slice(colon + 2).trim()
  }
  return raw
}

/**
 * Classify a raw generate error into a friendly message for the status band.
 */
export function mapPlaygroundGenerateError(
  message: string,
  copy: PlaygroundErrorCopy,
  opts?: MapPlaygroundErrorOpts,
): string {
  const format = opts?.format ?? defaultFormat

  if (opts?.userCancelled && copy.playgroundCancelled) {
    return copy.playgroundCancelled
  }

  const raw = (message || '').trim()
  if (!raw) return copy.playgroundGenerateFailed

  const lower = raw.toLowerCase()

  // --- Model / agent failures first (before timeout) ---
  // Backend 502 exact text is "Pro AI agent generation failed"; never treat as timeout.
  if (
    /pro ai agent generation failed/i.test(raw) ||
    /agent generation failed/i.test(raw) ||
    /generation failed \(\s*502\s*\)/i.test(raw)
  ) {
    return copy.playgroundAiGenerationFailedHint
  }

  if (
    /pro ai model is not enabled/i.test(raw) ||
    /model is not enabled or configured/i.test(raw) ||
    /ai model is not enabled/i.test(raw)
  ) {
    return copy.playgroundAiNotConfiguredHint
  }

  // --- Timeouts (client hard timeout or explicit timeout wording) ---
  const isTimeout =
    raw === 'TimeoutError' ||
    lower === 'timeouterror' ||
    /timed?\s*out/i.test(raw) ||
    /aborted due to timeout/i.test(raw) ||
    /signal timed out/i.test(raw) ||
    /backend proxy timeout/i.test(raw) ||
    /\btimeout\b/i.test(raw)

  if (isTimeout) return copy.playgroundTimeoutHint

  // Non-user abort → soft interrupt (treat like timeout recovery path)
  if (
    raw === 'AbortError' ||
    lower === 'aborterror' ||
    /the operation was aborted/i.test(raw)
  ) {
    return copy.playgroundTimeoutHint
  }

  // --- Auth / admin (playground is admin-only) ---
  if (
    /\bHTTP\s*401\b/i.test(raw) ||
    /please login/i.test(raw) ||
    /unauthorized/i.test(raw) ||
    /invalid user id in authorization/i.test(raw)
  ) {
    return copy.playgroundAuthRequiredHint
  }

  if (
    /\bHTTP\s*403\b/i.test(raw) ||
    /administrator access required/i.test(raw) ||
    /only current admin/i.test(raw) ||
    (/forbidden/i.test(raw) && /admin/i.test(raw))
  ) {
    return copy.playgroundAdminRequiredHint
  }

  // CSRF is retried once in the service; if it still surfaces, treat as auth.
  if (/csrf/i.test(raw)) {
    return copy.playgroundAuthRequiredHint
  }

  // --- Rate limit ---
  if (/\bHTTP\s*429\b/i.test(raw) || /rate\s*limit/i.test(raw) || /too many requests/i.test(raw)) {
    return copy.playgroundRateLimitHint
  }

  // --- Payload / history too large ---
  if (
    /\bHTTP\s*413\b/i.test(raw) ||
    /payload too large/i.test(raw) ||
    /request body exceeds/i.test(raw) ||
    /current project is too large/i.test(raw) ||
    /project exceeds/i.test(raw) ||
    /history.*too large/i.test(raw) ||
    /history accepts at most/i.test(raw)
  ) {
    return compose(
      copy.playgroundPayloadTooLargeHint,
      raw,
      copy.playgroundErrorDetail,
      format,
    )
  }

  // --- Validation exhausted (422) ---
  if (
    /did not pass validation/i.test(raw) ||
    (/validation/i.test(raw) && /after\s+\d+\s+attempts/i.test(raw)) ||
    /\bHTTP\s*422\b/i.test(raw)
  ) {
    const detail = extractValidationDetail(raw)
    // HTTP 422 alone without detail → generic validation hint
    const useful =
      detail && !/^HTTP\s*422$/i.test(detail) ? detail : undefined
    return compose(
      copy.playgroundValidationFailedHint,
      useful,
      copy.playgroundErrorDetail,
      format,
    )
  }

  // --- Agent busy / shutting down ---
  if (
    /agent is shutting down/i.test(raw) ||
    /playground agent is shutting down/i.test(raw)
  ) {
    return copy.playgroundAgentBusyHint
  }

  // --- Stream incomplete ---
  if (
    /stream ended without a final response/i.test(raw) ||
    /stream body unavailable/i.test(raw) ||
    /failed to serialize stream event/i.test(raw)
  ) {
    return copy.playgroundStreamIncompleteHint
  }

  // --- Network / offline ---
  if (
    /failed to fetch/i.test(raw) ||
    /networkerror/i.test(raw) ||
    /load failed/i.test(raw) ||
    /network request failed/i.test(raw) ||
    /net::err_/i.test(raw)
  ) {
    return copy.playgroundNetworkHint
  }

  // --- Bad request with concrete server message ---
  if (
    /\bHTTP\s*400\b/i.test(raw) ||
    /instruction must contain/i.test(raw) ||
    /invalid current project/i.test(raw) ||
    /invalid request payload/i.test(raw) ||
    /invalid runtime feedback/i.test(raw) ||
    /history turn/i.test(raw) ||
    /failed history entries/i.test(raw)
  ) {
    const detail = raw.replace(/^HTTP\s*400\s*:?\s*/i, '').trim()
    return format(copy.playgroundBadRequestHint, {
      detail: truncateDetail(detail || raw),
    })
  }

  // --- Generic 5xx / gateway ---
  const isServer =
    /\bHTTP\s*50[0234]\b/i.test(raw) ||
    /bad gateway/i.test(raw) ||
    /gateway timeout/i.test(raw) ||
    /service unavailable/i.test(raw) ||
    /internal server error/i.test(raw)

  if (isServer) {
    return compose(
      copy.playgroundServerErrorHint,
      raw.startsWith('HTTP') ? raw : undefined,
      copy.playgroundErrorDetail,
      format,
    )
  }

  // Unknown: keep raw if it looks user-authored / already localized;
  // otherwise wrap with generic primary + technical detail.
  const looksLocalized =
    /[\u3040-\u30FF\u3400-\u9FFF]/.test(raw) || // CJK
    raw.length > 40

  if (looksLocalized && !/^HTTP\s*\d+/i.test(raw) && !/^[a-z]{2,}Error$/i.test(raw)) {
    // Prefer showing the server message when it's already descriptive.
    // Still prefix with generate-failed if it's a short English identifier.
    if (/^[\w .:/-]{1,48}$/.test(raw) && !/\s{2,}/.test(raw) && raw.split(' ').length <= 4) {
      return compose(
        copy.playgroundGenerateFailed,
        raw,
        copy.playgroundErrorDetail,
        format,
      )
    }
    return raw
  }

  return compose(
    copy.playgroundGenerateFailed,
    raw,
    copy.playgroundErrorDetail,
    format,
  )
}

/** Prefix sandbox / widget runtime messages for the floating warning card. */
export function mapPlaygroundRuntimeError(
  message: string,
  copy: Pick<PlaygroundErrorCopy, 'playgroundRuntimeError'>,
  format: (template: string, params: Record<string, string | number>) => string = defaultFormat,
): string {
  const raw = (message || '').trim() || 'Unknown runtime error'
  if (copy.playgroundRuntimeError.includes('{message}')) {
    return format(copy.playgroundRuntimeError, {
      message: truncateDetail(raw, 480),
    })
  }
  return raw
}
