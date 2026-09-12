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
  playgroundUnknownError?: string
}

export interface MapPlaygroundErrorOpts {
  userCancelled?: boolean
  code?: string
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
  const t = text.replaceAll(/\s+/g, ' ').trim()
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

  switch (opts?.code) {
    case 'playground_ai_unconfigured':
      return copy.playgroundAiNotConfiguredHint
    case 'playground_ai_failed':
      return copy.playgroundAiGenerationFailedHint
    case 'playground_validation_failed':
      return compose(
        copy.playgroundValidationFailedHint,
        extractValidationDetail(raw),
        copy.playgroundErrorDetail,
        format,
      )
    case 'playground_payload_too_large':
      return compose(
        copy.playgroundPayloadTooLargeHint,
        raw,
        copy.playgroundErrorDetail,
        format,
      )
    case 'playground_agent_busy':
      return copy.playgroundAgentBusyHint
    case 'playground_cancelled':
      return copy.playgroundCancelled || copy.playgroundTimeoutHint
    case 'playground_auth_required':
      return copy.playgroundAuthRequiredHint
    case 'playground_admin_required':
      return copy.playgroundAdminRequiredHint
    case 'playground_rate_limited':
      return copy.playgroundRateLimitHint
    case 'playground_bad_request':
      return format(copy.playgroundBadRequestHint, {
        detail: truncateDetail(raw.replaceAll(/^HTTP\s*400\s*:?\s*/ig, '').trim() || raw),
      })
    default:
      break
  }

  const lower = raw.toLowerCase()

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

  const isTimeout =
    raw === 'TimeoutError' ||
    lower === 'timeouterror' ||
    /timed?\s*out/i.test(raw) ||
    /aborted due to timeout/i.test(raw) ||
    /signal timed out/i.test(raw) ||
    /backend proxy timeout/i.test(raw) ||
    /\btimeout\b/i.test(raw)

  if (isTimeout) return copy.playgroundTimeoutHint

  if (
    raw === 'AbortError' ||
    lower === 'aborterror' ||
    /the operation was aborted/i.test(raw)
  ) {
    return copy.playgroundTimeoutHint
  }

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

  if (/csrf/i.test(raw)) {
    return copy.playgroundAuthRequiredHint
  }

  if (/\bHTTP\s*429\b/i.test(raw) || /rate\s*limit/i.test(raw) || /too many requests/i.test(raw)) {
    return copy.playgroundRateLimitHint
  }

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

  if (
    /did not pass validation/i.test(raw) ||
    (/validation/i.test(raw) && /after\s+\d+\s+attempts/i.test(raw)) ||
    /\bHTTP\s*422\b/i.test(raw)
  ) {
    const detail = extractValidationDetail(raw)
    const useful =
      detail && !/^HTTP\s*422$/i.test(detail) ? detail : undefined
    return compose(
      copy.playgroundValidationFailedHint,
      useful,
      copy.playgroundErrorDetail,
      format,
    )
  }

  if (
    /agent is shutting down/i.test(raw) ||
    /playground agent is shutting down/i.test(raw)
  ) {
    return copy.playgroundAgentBusyHint
  }

  if (
    /stream ended without a final response/i.test(raw) ||
    /stream body unavailable/i.test(raw) ||
    /failed to serialize stream event/i.test(raw)
  ) {
    return copy.playgroundStreamIncompleteHint
  }

  if (
    /failed to fetch/i.test(raw) ||
    /networkerror/i.test(raw) ||
    /load failed/i.test(raw) ||
    /network request failed/i.test(raw) ||
    /net::err_/i.test(raw)
  ) {
    return copy.playgroundNetworkHint
  }

  if (
    /invalid agent plan/i.test(raw) ||
    /invalid json project/i.test(raw)
  ) {
    return copy.playgroundValidationFailedHint
  }

  if (
    /\bHTTP\s*400\b/i.test(raw) ||
    /instruction must contain/i.test(raw) ||
    /invalid current project/i.test(raw) ||
    /invalid request payload/i.test(raw) ||
    /invalid runtime feedback/i.test(raw) ||
    /history turn/i.test(raw) ||
    /failed history entries/i.test(raw)
  ) {
    const detail = raw.replaceAll(/^HTTP\s*400\s*:?\s*/ig, '').trim()
    return format(copy.playgroundBadRequestHint, {
      detail: truncateDetail(detail || raw),
    })
  }

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

  const looksLocalized =
    /[\u3040-\u30FF\u3400-\u9FFF]/.test(raw) ||
    raw.length > 40

  if (looksLocalized && !/^HTTP\s*\d+/i.test(raw) && !/^[a-z]{2,}Error$/i.test(raw)) {
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

export function mapPlaygroundRuntimeError(
  message: string,
  copy: Pick<
    PlaygroundErrorCopy,
    'playgroundRuntimeError' | 'playgroundUnknownError'
  >,
  format: (template: string, params: Record<string, string | number>) => string = defaultFormat,
): string {
  const raw =
    (message || '').trim() || copy.playgroundUnknownError || ''
  if (!raw) {
    return copy.playgroundRuntimeError.includes('{message}')
      ? format(copy.playgroundRuntimeError, { message: '?' })
      : copy.playgroundRuntimeError
  }
  if (copy.playgroundRuntimeError.includes('{message}')) {
    return format(copy.playgroundRuntimeError, {
      message: truncateDetail(raw, 480),
    })
  }
  return raw
}
