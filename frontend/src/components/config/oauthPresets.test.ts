import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  entryFromPreset,
  findPreset,
  hasOAuthCredential,
  OAUTH_PRESETS,
} from './oauthPresets'

describe('OAuth presets', () => {
  it('lists the picker presets', () => {
    assert.deepEqual(
      OAUTH_PRESETS.map((item) => item.id),
      [
        'github',
        'google',
        'microsoft',
        'gitlab',
        'discord',
        'authentik',
        'keycloak',
        'auth0',
        'custom',
      ],
    )
  })

  it('treats a masked secret as configured', () => {
    assert.equal(
      hasOAuthCredential({ client_id: 'abc', client_secret: '***' }),
      true,
    )
    assert.equal(
      hasOAuthCredential({ client_id: 'abc', client_secret: '' }),
      false,
    )
    assert.equal(
      hasOAuthCredential({ client_id: '', client_secret: 'secret' }),
      false,
    )
  })

  it('stamps a unique slug onto a second copy', () => {
    const google = findPreset('google')
    assert.ok(google)
    const first = entryFromPreset(google, [])
    assert.equal(first.slug, 'google')
    assert.equal(first.display_name, 'Google')
    assert.equal(first.kind, 'oidc')
    assert.ok(first.discovery_url?.includes('accounts.google.com'))
    const second = entryFromPreset(google, [first])
    assert.equal(second.slug, 'google-2')
    assert.equal(second.display_name, 'Google 2')
  })

  it('falls back to the preset id when the default slug is empty', () => {
    const custom = findPreset('custom')
    assert.ok(custom)
    const entry = entryFromPreset(custom, [])
    assert.equal(entry.slug, 'custom')
    assert.equal(entry.discovery_url, '')
  })
})
