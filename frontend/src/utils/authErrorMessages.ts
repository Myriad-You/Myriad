/**
 * Map backend auth / OAuth / admin error tokens to localized user-facing copy.
 */

import type { useI18n } from '../contexts/I18nContext'

type T = ReturnType<typeof useI18n>['t']
type Format = ReturnType<typeof useI18n>['format']

/** Safe provider `desc` for display: strip controls, truncate, reject junk. */
export function sanitizeOAuthDesc(raw: string | null | undefined): string | null {
  if (!raw) return null
  let s = raw.trim()
  // URLSearchParams already decodes; still normalize whitespace / controls
  s = s.replace(/[\u0000-\u001F\u007F]/g, ' ').replace(/\s+/g, ' ').trim()
  s = s.replace(/[<>`]/g, '')
  if (!s) return null
  // Avoid echoing long opaque blobs or pure URLs as the primary message
  if (s.length > 180) s = `${s.slice(0, 180)}…`
  return s
}

/** Known OAuth callback `oauth_error` codes from backend + common IdP errors. */
export function messageForOAuthError(
  code: string,
  desc: string | null | undefined,
  t: T,
  format: Format,
): string {
  const safeDesc = sanitizeOAuthDesc(desc)
  let base: string

  switch (code) {
    case 'state_missing':
      base = t.auth.oauthErrorStateMissing
      break
    case 'state_expired':
      base = t.auth.oauthErrorStateExpired
      break
    case 'state_replay':
      base = t.auth.oauthErrorStateReplay
      break
    case 'state_slug_mismatch':
      base = t.auth.oauthErrorStateSlugMismatch
      break
    case 'missing_code':
      base = t.auth.oauthErrorMissingCode
      break
    case 'missing_state':
      base = t.auth.oauthErrorMissingState
      break
    case 'access_denied':
      base = t.auth.oauthErrorAccessDenied
      break
    case 'temporarily_unavailable':
      base = t.auth.oauthErrorTemporarilyUnavailable
      break
    case 'server_error':
      base = t.auth.oauthErrorServerError
      break
    case 'invalid_request':
      base = t.auth.oauthErrorInvalidRequest
      break
    case 'unauthorized_client':
      base = t.auth.oauthErrorUnauthorizedClient
      break
    case 'unsupported_response_type':
      base = t.auth.oauthErrorUnsupportedResponseType
      break
    case 'invalid_scope':
      base = t.auth.oauthErrorInvalidScope
      break
    case 'token_exchange_failed':
      base = t.auth.oauthErrorTokenExchange
      break
    case 'profile_fetch_failed':
      base = t.auth.oauthErrorProfileFetch
      break
    case 'provider_unavailable':
      base = t.auth.oauthErrorProviderUnavailable
      break
    case 'login_failed':
      base = t.auth.oauthErrorLoginFailed
      break
    default:
      base = format(t.auth.oauthError, { code: code || 'unknown' })
  }

  // Append sanitized provider description when it adds actionable detail
  if (
    safeDesc &&
    !base.toLowerCase().includes(safeDesc.toLowerCase()) &&
    // Prefer our guidance for session/state errors; desc is usually empty anyway
    !code.startsWith('state_') &&
    code !== 'missing_code' &&
    code !== 'missing_state'
  ) {
    return format(t.auth.oauthErrorWithDesc, { message: base, desc: safeDesc })
  }

  return base
}

/** Local username/password login failures from auth_local + rate limit middleware. */
export function messageForLocalLoginError(
  err: unknown,
  t: T,
  format: Format,
): string {
  const msg =
    err instanceof Error && err.message ? err.message : String(err ?? '')

  if (/invalid credentials|username or password is incorrect/i.test(msg)) {
    return t.auth.invalidCredentials
  }
  if (/local login disabled/i.test(msg)) {
    return t.auth.localLoginDisabled
  }
  // Backend: "Rate limit exceeded. Please try again in N seconds."
  const retryMatch = msg.match(/try again in (\d+)\s*seconds?/i)
  if (retryMatch || /too many requests|rate limit exceeded/i.test(msg)) {
    const seconds = retryMatch ? Number.parseInt(retryMatch[1], 10) : 60
    return format(t.auth.rateLimitError, {
      seconds: Number.isFinite(seconds) ? seconds : 60,
    })
  }

  return msg || t.auth.loginFailed
}

/** Admin user-management API English error bodies → i18n. */
export function messageForAdminUserError(
  err: unknown,
  t: T,
  fallback: string,
): string {
  const msg =
    err instanceof Error && err.message ? err.message : String(err ?? '')
  if (!msg) return fallback

  if (
    /cannot unlink the user's only sign-in method/i.test(msg) ||
    /cannot unlink last identity/i.test(msg)
  ) {
    return t.config.usersErrorUnlinkLast
  }
  if (/cannot delete your own account/i.test(msg)) {
    return t.config.usersErrorDeleteSelf
  }
  if (/cannot delete the last administrator/i.test(msg)) {
    return t.config.usersErrorLastAdmin
  }
  if (/cannot demote the last administrator/i.test(msg)) {
    return t.config.usersErrorLastAdminDemote
  }
  if (/cannot revoke your own admin/i.test(msg)) {
    return t.config.usersErrorRevokeSelf
  }
  if (
    /only the primary administrator.*delete administrators/i.test(msg) ||
    /only the primary administrator \(id=1\) can delete/i.test(msg) ||
    /only the site owner can delete administrators/i.test(msg)
  ) {
    return t.config.usersErrorPrimaryAdminDelete
  }
  if (
    /only the primary administrator.*admin roles/i.test(msg) ||
    /only the site owner can change admin roles/i.test(msg)
  ) {
    return t.config.usersPrimaryAdminOnly
  }
  if (/cannot delete the site owner/i.test(msg)) {
    return t.config.usersErrorCannotDeleteOwner
  }
  if (/cannot demote the site owner/i.test(msg)) {
    return t.config.usersErrorCannotDemoteOwner
  }
  if (/no linked oauth identity/i.test(msg)) {
    return t.config.usersLocalLoginRequiresOAuth
  }

  // Prefer localized fallback over raw English when it looks like our generic API prefix
  if (/^API Error:\s*\d+/i.test(msg)) return fallback
  return msg
}
