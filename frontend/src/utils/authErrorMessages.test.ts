/* eslint-disable test/no-import-node-test -- node:test is the repository test runner */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  messageForAdminUserError,
  messageForLocalLoginError,
  messageForOAuthError,
  sanitizeOAuthDesc,
} from './authErrorMessages'

const t = {
  auth: {
    oauthError: 'Failed ({code}). Retry.',
    oauthErrorWithDesc: '{message} Details: {desc}',
    oauthErrorStateMissing: 'state missing msg',
    oauthErrorStateExpired: 'state expired msg',
    oauthErrorStateReplay: 'state replay msg',
    oauthErrorStateSlugMismatch: 'slug mismatch msg',
    oauthErrorMissingCode: 'missing code msg',
    oauthErrorMissingState: 'missing state msg',
    oauthErrorAccessDenied: 'access denied msg',
    oauthErrorTemporarilyUnavailable: 'temp unavail msg',
    oauthErrorServerError: 'server error msg',
    oauthErrorInvalidRequest: 'invalid request msg',
    oauthErrorUnauthorizedClient: 'unauthorized client msg',
    oauthErrorUnsupportedResponseType: 'unsupported rt msg',
    oauthErrorInvalidScope: 'invalid scope msg',
    oauthErrorTokenExchange: 'token exchange msg',
    oauthErrorProfileFetch: 'profile fetch msg',
    oauthErrorProviderUnavailable: 'provider unavail msg',
    oauthErrorLoginFailed: 'login failed msg',
    invalidCredentials: 'bad credentials',
    localLoginDisabled: 'local disabled',
    rateLimitError: 'retry in {seconds}s',
    loginFailed: 'login failed',
  },
  config: {
    usersErrorUnlinkLast: 'unlink last',
    usersErrorDeleteSelf: 'delete self',
    usersErrorLastAdmin: 'last admin',
    usersErrorLastAdminDemote: 'last admin demote',
    usersErrorRevokeSelf: 'revoke self',
    usersErrorPrimaryAdminDelete: 'primary delete',
    usersPrimaryAdminOnly: 'primary roles',
    usersErrorCannotDeleteOwner: 'cannot delete owner',
    usersErrorCannotDemoteOwner: 'cannot demote owner',
    usersLocalLoginRequiresOAuth: 'needs oauth',
  },
} as any

function format(
  template: string,
  params: Record<string, string | number>,
): string {
  return Object.entries(params).reduce(
    (s, [k, v]) => s.replaceAll(`{${k}}`, String(v)),
    template,
  )
}

describe('sanitizeOAuthDesc', () => {
  it('strips controls and truncates', () => {
    assert.equal(sanitizeOAuthDesc('  hello\x00world  '), 'hello world')
    assert.equal(sanitizeOAuthDesc(null), null)
    assert.ok((sanitizeOAuthDesc('a'.repeat(200)) ?? '').endsWith('…'))
  })
})

describe('messageForOAuthError', () => {
  it('maps known codes', () => {
    assert.equal(
      messageForOAuthError('state_expired', null, t, format),
      'state expired msg',
    )
    assert.equal(
      messageForOAuthError('state_replay', null, t, format),
      'state replay msg',
    )
    assert.equal(
      messageForOAuthError('access_denied', null, t, format),
      'access denied msg',
    )
    assert.equal(
      messageForOAuthError('token_exchange_failed', null, t, format),
      'token exchange msg',
    )
  })

  it('appends safe desc for provider errors', () => {
    const msg = messageForOAuthError(
      'access_denied',
      'User cancelled',
      t,
      format,
    )
    assert.match(msg, /access denied msg/)
    assert.match(msg, /User cancelled/)
  })

  it('does not append desc for state errors', () => {
    const msg = messageForOAuthError(
      'state_expired',
      'should not appear',
      t,
      format,
    )
    assert.equal(msg, 'state expired msg')
  })

  it('falls back for unknown codes', () => {
    assert.match(
      messageForOAuthError('weird_code', null, t, format),
      /weird_code/,
    )
  })
})

describe('messageForLocalLoginError', () => {
  it('maps credentials and local_login_disabled', () => {
    assert.equal(
      messageForLocalLoginError(
        new Error('Username or password is incorrect'),
        t,
        format,
      ),
      'bad credentials',
    )
    assert.equal(
      messageForLocalLoginError(new Error('Local login disabled'), t, format),
      'local disabled',
    )
  })

  it('maps rate limit retry seconds', () => {
    assert.equal(
      messageForLocalLoginError(
        new Error('Rate limit exceeded. Please try again in 300 seconds.'),
        t,
        format,
      ),
      'retry in 300s',
    )
  })
})

describe('messageForAdminUserError', () => {
  it('maps known admin English errors', () => {
    assert.equal(
      messageForAdminUserError(
        new Error("Cannot unlink the user's only sign-in method"),
        t,
        'fallback',
      ),
      'unlink last',
    )
    assert.equal(
      messageForAdminUserError(
        new Error('Cannot delete your own account'),
        t,
        'fallback',
      ),
      'delete self',
    )
    assert.equal(
      messageForAdminUserError(
        new Error(
          'Only the primary administrator (id=1) can change admin roles',
        ),
        t,
        'fallback',
      ),
      'primary roles',
    )
    assert.equal(
      messageForAdminUserError(
        new Error('Only the site owner can change admin roles'),
        t,
        'fallback',
      ),
      'primary roles',
    )
    assert.equal(
      messageForAdminUserError(
        new Error('Cannot delete the site owner'),
        t,
        'fallback',
      ),
      'cannot delete owner',
    )
    assert.equal(
      messageForAdminUserError(
        new Error('Cannot demote the site owner'),
        t,
        'fallback',
      ),
      'cannot demote owner',
    )
  })
})
