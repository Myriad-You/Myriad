/**
 * Surface OAuth outcomes from URL query params as toasts, then clean the query
 * so refresh does not re-show them.
 *
 * Handles:
 * - `/?link=success|error&...` (account link while already signed in)
 * - `?oauth_error=&desc=` on any route (including `/login` while GuestOnly
 *   shows the auth spinner for already-signed-in users — toast fires before redirect)
 *
 * LoginForm no longer renders oauth_error as an inline banner, so guests and
 * authed users share this single toast surface (no double-toast).
 */

import { useEffect, useRef } from 'react'
import { useI18n } from '../contexts/I18nContext'
import { messageForOAuthError } from '../utils/authErrorMessages'
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

function cleanAuthFeedbackParams(): void {
  try {
    const params = new URLSearchParams(window.location.search)
    let changed = false
    if (params.has('link')) {
      params.delete('link')
      params.delete('reason')
      params.delete('provider')
      params.delete('username')
      changed = true
    }
    if (params.has('oauth_error')) {
      params.delete('oauth_error')
      params.delete('desc')
      changed = true
    }
    if (!changed) return
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
  // Deduplicate within a mount lifecycle (t/format changes, strict mode, etc.)
  const handledKeyRef = useRef<string | null>(null)

  useEffect(() => {
    try {
      const params = new URLSearchParams(window.location.search)
      const link = params.get('link')?.trim()
      const oauthError = params.get('oauth_error')?.trim()
      if (!oauthError && !link) return

      const key = `${oauthError ?? ''}|${params.get('desc') ?? ''}|${link ?? ''}|${params.get('reason') ?? ''}`
      if (handledKeyRef.current === key) {
        cleanAuthFeedbackParams()
        return
      }
      handledKeyRef.current = key

      if (oauthError) {
        const desc = params.get('desc')
        showError(messageForOAuthError(oauthError, desc, t, format))
      }

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

      cleanAuthFeedbackParams()
    } catch {
      // ignore (SSR / non-browser)
    }
  }, [t, format])
}
