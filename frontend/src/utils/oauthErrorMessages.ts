import type { useI18n } from '../contexts/I18nContext'

type T = ReturnType<typeof useI18n>['t']
type Format = ReturnType<typeof useI18n>['format']

export function sanitizeOAuthDesc(raw: string | null | undefined): string | null {
  if (!raw) return null
  let s = raw.trim()
  s = s.replaceAll(/[\u0000-\u001F\u007F]/g, ' ').replaceAll(/\s+/g, ' ').trim()
  s = s.replaceAll(/[<>`]/g, '')
  if (!s) return null
  if (s.length > 180) s = `${s.slice(0, 180)}…`
  return s
}

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
    case 'email_already_registered':
      base = t.auth.oauthErrorEmailAlreadyRegistered
      break
    case 'browser_tx_mismatch':
      base = t.auth.oauthErrorBrowserTxMismatch
      break
    case 'link_failed':
      base = t.auth.oauthErrorLinkFailed
      break
    default:
      base = format(t.auth.oauthError, { code: code || 'unknown' })
  }

  if (
    safeDesc &&
    !base.toLowerCase().includes(safeDesc.toLowerCase()) &&
    !code.startsWith('state_') &&
    code !== 'missing_code' &&
    code !== 'missing_state'
  ) {
    return format(t.auth.oauthErrorWithDesc, { message: base, desc: safeDesc })
  }

  return base
}
