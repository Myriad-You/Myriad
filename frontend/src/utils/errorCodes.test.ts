import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import { currentCopy } from '../i18n/localeCopy.ts'
import { inferErrorCode, resolveErrorCode } from './errorCodes.ts'
import { userFacingError } from './userFacingError.ts'

const SPEC = JSON.parse(
  readFileSync(new URL('../../../shared/error_codes.json', import.meta.url), 'utf8'),
) as {
  labels: Record<string, string>
  leftovers: Record<string, string>
  prefixes: Record<string, string>
}

describe('error codes', () => {
  it('maps every shared label and leftover to its code', () => {
    for (const [label, code] of Object.entries(SPEC.labels)) {
      assert.equal(inferErrorCode(label), code, label)
    }
    for (const [label, code] of Object.entries(SPEC.leftovers)) {
      assert.equal(inferErrorCode(label), code, label)
    }
  })

  it('maps leftover prefixes without eating the rest of the dump', () => {
    assert.equal(
      inferErrorCode('分享内容为空：请提供 text，或 title/summary'),
      'share_text_empty',
    )
    assert.equal(
      inferErrorCode('Service in configuration mode — retry later'),
      'configuration_mode',
    )
    assert.equal(
      inferErrorCode('Failed to fetch playlist: timed out'),
      'playlist_fetch_failed',
    )
  })

  it('lets leftover text win over an unmapped code', () => {
    assert.equal(
      resolveErrorCode('unmapped', 'Failed to load configuration'),
      'config_load_failed',
    )
    assert.equal(resolveErrorCode('UNAUTHORIZED', ''), 'unauthorized')
  })

  it('userFacingError prefers the inferred leftover code', () => {
    assert.equal(
      userFacingError('服务器正在配置模式'),
      currentCopy().errors.configurationMode,
    )
    assert.equal(
      userFacingError('Username is required'),
      currentCopy().errors.usernameRequired,
    )
    const src = readFileSync(new URL('./userFacingError.ts', import.meta.url), 'utf8')
    assert.match(src, /resolveErrorCode/)
  })
})
