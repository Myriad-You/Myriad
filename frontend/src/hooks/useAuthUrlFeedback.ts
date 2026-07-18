/**
 * Surface OAuth account-link outcomes from `/?link=success|error&...` as toasts,
 * then clean the query so refresh does not re-show them.
 */

import { useEffect } from 'react'
import { useI18n } from '../contexts/I18nContext'
import { showError, showSuccess } from '../utils/toastManager'

function messageForLinkReason(
  reason: string | null,
  t: ReturnType<typeof useI18n>['t'],
  format: ReturnType<typeof useI18n>['format'],
): string {
  switch (reason) {
    case 'already_linked':
      return t.auth.linkErrorAlreadyLinked
    case 'user_not_found':
      return t.auth.linkErrorUserNotFound
    default:
      return reason
        ? format(t.auth.linkErrorUnknown, { reason })
        : t.auth.linkErrorGeneric
  }
}

function cleanLinkParams(): void {
  try {
    const params = new URLSearchParams(window.location.search)
    if (!params.has('link')) return
    params.delete('link')
    params.delete('reason')
    params.delete('provider')
    params.delete('username')
    const next = params.toString()
    const path = window.location.pathname
    window.history.replaceState({}, '', next ? `${path}?${next}` : path)
  } catch {
    // ignore
  }
}

/** Call once near the root layout (has ToastContainer + I18n). */
export function useAuthUrlFeedback(): void {
  const { t, format } = useI18n()

  useEffect(() => {
    try {
      const params = new URLSearchParams(window.location.search)
      const link = params.get('link')?.trim()
      if (!link) return

      if (link === 'error') {
        const reason = params.get('reason')?.trim() || null
        showError(messageForLinkReason(reason, t, format))
      } else if (link === 'success') {
        const provider = params.get('provider')?.trim() || ''
        const username = params.get('username')?.trim() || ''
        if (provider && username) {
          showSuccess(
            format(t.auth.linkSuccessDetail, { provider, username }),
          )
        } else if (provider) {
          showSuccess(format(t.auth.linkSuccessProvider, { provider }))
        } else {
          showSuccess(t.auth.linkSuccess)
        }
      }

      cleanLinkParams()
    } catch {
      // ignore (SSR / non-browser)
    }
  }, [t, format])
}
