import { currentCopy } from '../i18n/localeCopy'
import { ApiError } from '../services/api'

export function httpStatusMessage(status: number): string {
  const t = currentCopy().errors
  if (status === 401) return t.unauthorized
  if (status === 403) return t.forbidden
  if (status === 404) return t.notFound
  if (status === 408) return t.timeout
  if (status === 429) return t.rateLimited
  if (status >= 500) {
    return t.serverError.replace('{status}', String(status))
  }
  if (status > 0) {
    return t.httpStatus.replace('{status}', String(status))
  }
  return t.networkError
}

export function statusFromErrorText(text: string): number {
  const m =
    text.match(/\bHTTP\s+(\d{3})\b/i) ||
    text.match(/^API Error:\s*(\d{3})$/i) ||
    text.match(/\((?:HTTP\s*)?(\d{3})\)$/)
  const status = m ? Number(m[1]) : 0
  return status >= 400 && status <= 599 ? status : 0
}

export function isUselessErrorText(text: string): boolean {
  const detail = text.replace(/\s+/g, ' ').trim()
  if (!detail) return true
  if (/^API Error:\s*\d+$/i.test(detail)) return true
  if (/^HTTP(\s+error!)?(\s*status:?)?\s*\d+(\s*:.*)?$/i.test(detail)) {
    return true
  }
  if (/^failed to [a-z ]+:\s*\d+$/i.test(detail)) return true
  if (/install failed(:\s*\d+)?$/i.test(detail)) return true
  if (/csrf token (unavailable|refresh failed)/i.test(detail)) return true
  if (/^\{[\s\S]*\}$/.test(detail)) return true
  if (/failed to fetch|networkerror|load failed/i.test(detail)) return true
  if (/^unknown error$/i.test(detail)) return true
  if (/^request timeout$/i.test(detail)) return true
  if (/^internal (server )?error$/i.test(detail)) return true
  if (/^operation failed$/i.test(detail)) return true
  if (/^failed$/i.test(detail)) return true
  if (/^failed to (save|load|get|publish|rotate|compose) /i.test(detail)) {
    return true
  }
  if (/^no library data available$/i.test(detail)) return true
  if (/^action failed$/i.test(detail)) return true
  if (/^discovery failed$|^import failed$|^failed to add$/i.test(detail)) {
    return true
  }
  if (/^database error$/i.test(detail)) return true
  if (/\((?:HTTP\s*)?\d{3}\)$/i.test(detail)) {
    const inner = detail.replace(/\s*\((?:HTTP\s*)?\d{3}\)\s*$/i, '').trim()
    if (
      !inner ||
      /^could not [a-z ]+$/i.test(inner) ||
      /^failed to [a-z ]+$/i.test(inner)
    ) {
      return true
    }
  }
  return false
}

function readHint(reason: unknown): string {
  if (
    reason &&
    typeof reason === 'object' &&
    'hint' in reason &&
    typeof (reason as { hint: unknown }).hint === 'string'
  ) {
    return (reason as { hint: string }).hint.trim()
  }
  return ''
}

function readStatus(reason: unknown): number {
  if (
    reason &&
    typeof reason === 'object' &&
    'status' in reason &&
    typeof (reason as { status: unknown }).status === 'number'
  ) {
    return (reason as { status: number }).status
  }
  return 0
}

function readCode(reason: unknown): string {
  if (
    reason &&
    typeof reason === 'object' &&
    'code' in reason &&
    typeof (reason as { code: unknown }).code === 'string'
  ) {
    return (reason as { code: string }).code.trim()
  }
  return ''
}

function joinParts(...parts: string[]): string {
  return parts.filter(Boolean).join(' · ')
}

function clip(text: string): string {
  return text.length > 180 ? `${text.slice(0, 179)}…` : text
}

/** Localized, diagnosable copy for anything that can land in the UI. */
export function userFacingError(reason: unknown, fallback?: string): string {
  const t = currentCopy().errors
  const fallbackText = fallback?.trim() || t.unknown
  const raw =
    reason instanceof Error
      ? reason.message.trim()
      : typeof reason === 'string'
        ? reason.trim()
        : ''
  const status = readStatus(reason) || statusFromErrorText(raw)
  const code = readCode(reason)
  const hint = readHint(reason)

  if (code === 'TIMEOUT' || status === 408) {
    return joinParts(t.timeout, usefulExtra(hint, t.timeout))
  }
  if (code === 'NETWORK_ERROR' || (status === 0 && reason instanceof ApiError)) {
    return joinParts(t.networkError, usefulExtra(hint, t.networkError))
  }
  if (code === 'CSRF' || /csrf token/i.test(raw)) {
    return joinParts(t.csrfUnavailable, usefulExtra(hint, t.csrfUnavailable))
  }

  const byStatus = status > 0 ? httpStatusMessage(status) : ''
  const useful = isUselessErrorText(raw) ? '' : clip(raw)
  const extraHint = usefulExtra(hint, byStatus, useful, fallbackText)

  if (byStatus && useful && useful !== byStatus) {
    return joinParts(byStatus, useful, extraHint)
  }
  if (useful) return joinParts(useful, extraHint)
  if (byStatus) return joinParts(byStatus, extraHint)
  return joinParts(fallbackText, extraHint)
}

function usefulExtra(text: string, ...known: string[]): string {
  if (!text || isUselessErrorText(text)) return ''
  if (known.some((item) => item && text === item)) return ''
  return clip(text)
}
