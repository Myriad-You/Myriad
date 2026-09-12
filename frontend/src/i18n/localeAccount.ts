import type { Locale } from './index'
import { API_URL } from '../config'
import { getCSRFToken } from '../utils/csrf'
import { hostLocaleHeaders } from './hostLocaleHeaders'

let persistHandler: ((locale: Locale) => void) | null = null

export function setLocalePersistHandler(
  handler: ((locale: Locale) => void) | null,
): void {
  persistHandler = handler
}

export function persistLocaleToAccount(locale: Locale): void {
  persistHandler?.(locale)
}

export async function putAccountLocale(locale: Locale): Promise<void> {
  const csrfToken = await getCSRFToken()
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...hostLocaleHeaders(),
  }
  if (csrfToken) headers['X-CSRF-Token'] = csrfToken
  const response = await fetch(`${API_URL}/api/auth/me/locale`, {
    method: 'PUT',
    credentials: 'include',
    headers,
    body: JSON.stringify({ locale }),
  })
  if (!response.ok && response.status !== 403) {
    throw new Error(`locale persist failed (${response.status})`)
  }
}
