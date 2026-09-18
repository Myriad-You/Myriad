import type { TranslationKeys, useI18n } from '../contexts/I18nContext'
import { userFacingError } from './userFacingError'

export {
  messageForOAuthError,
  sanitizeOAuthDesc,
} from './oauthErrorMessages'

type T = ReturnType<typeof useI18n>['t']
type Format = ReturnType<typeof useI18n>['format']

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
  const retryMatch = msg.match(/try again in (\d+)\s*seconds?/i)
  if (retryMatch || /too many requests|rate limit exceeded/i.test(msg)) {
    const seconds = retryMatch ? Number.parseInt(retryMatch[1], 10) : 60
    return format(t.auth.rateLimitError, {
      seconds: Number.isFinite(seconds) ? seconds : 60,
    })
  }

  return userFacingError(err, t.auth.loginFailed)
}

export function messageForAdminUserError(
  err: unknown,
  t: TranslationKeys,
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
  if (/cannot disable tapp install for the site owner/i.test(msg)) {
    return t.config.usersErrorCannotRestrictOwnerInstall
  }
  if (/tapp installation is disabled for this account/i.test(msg)) {
    return t.config.usersTappInstallDisabled
  }
  if (/not found/i.test(msg) && /tapp/i.test(msg)) {
    return t.config.usersErrorTappNotFound
  }

  return userFacingError(err, fallback)
}

/** Public local registration failures from /api/auth/register. */
export function messageForRegisterError(err: unknown, t: T): string {
  const msg =
    err instanceof Error && err.message ? err.message : String(err ?? '')
  if (/registration disabled|public registration is disabled/i.test(msg)) {
    return t.auth.registerDisabled
  }
  if (/setup_required|finish the setup wizard/i.test(msg)) {
    return t.auth.registerSetupRequired
  }
  if (/username taken|already in use|already exists/i.test(msg)) {
    return t.auth.usernameTaken
  }
  return userFacingError(err, t.auth.registerFailed)
}
