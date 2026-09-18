import { currentCopy, formatCurrent } from '../i18n/localeCopy'

function fill(
  template: string,
  params: Record<string, string | number> = {},
): string {
  return formatCurrent(template, params)
}

export function httpStatusMessage(status: number): string {
  const t = currentCopy().errors
  if (status === 401) return t.unauthorized
  if (status === 403) return t.forbidden
  if (status === 404) return t.notFound
  if (status === 408) return t.timeout
  if (status === 429) return t.rateLimited
  if (status >= 500) {
    return fill(t.serverError, { status })
  }
  if (status > 0) {
    return fill(t.httpStatus, { status })
  }
  return t.networkError
}

export function statusFromErrorText(text: string): number {
  const m =
    text.match(/\bHTTP\s+(\d{3})\b/i) ??
    text.match(/^API Error:\s*(\d{3})$/i) ??
    text.match(/\((?:HTTP\s*)?(\d{3})\)$/)
  const status = m ? Number(m[1]) : 0
  return status >= 400 && status <= 599 ? status : 0
}
