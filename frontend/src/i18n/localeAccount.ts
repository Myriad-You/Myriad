import type { Locale } from './index'
import { ApiError, apiService } from '../services/api'

let persistHandler: ((locale: Locale) => void) | null = null

export function setLocalePersistHandler(
  handler: ((locale: Locale) => void) | null,
): void {
  persistHandler = handler
}

export function persistLocaleToAccount(locale: Locale): void {
  persistHandler?.(locale)
}

/** A 403 means this account may not keep a preference; the local choice stands. */
export async function putAccountLocale(locale: Locale): Promise<void> {
  try {
    await apiService.put('/auth/me/locale', { locale })
  } catch (error) {
    if (error instanceof ApiError && error.status === 403) return
    throw error
  }
}
