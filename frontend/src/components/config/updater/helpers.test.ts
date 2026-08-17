/**
 * Unit tests for updater infra-outcome matching.
 *
 * Run from frontend/:
 *   pnpm test:unit -- src/components/config/updater/helpers.test.ts
 */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { isFreshInfraOutcome } from './helpers.ts'

describe('isFreshInfraOutcome', () => {
  const last = {
    status: 'succeeded' as const,
    target_tag: 'v0.3.32',
    at: '2026-08-17T02:00:00Z',
  }

  it('ignores a missing or unchanged record', () => {
    assert.equal(isFreshInfraOutcome(null, last.at, ''), null)
    assert.equal(isFreshInfraOutcome(last, last.at, ''), null)
  })

  it('accepts a new terminal outcome when no schedule tag is known', () => {
    assert.equal(isFreshInfraOutcome(last, '2026-08-17T01:00:00Z', ''), 'succeeded')
    assert.equal(
      isFreshInfraOutcome(
        { ...last, status: 'failed' },
        '2026-08-17T01:00:00Z',
        '',
      ),
      'failed',
    )
  })

  it('does not treat the app tip as the updater target', () => {
    assert.equal(
      isFreshInfraOutcome(last, '2026-08-17T01:00:00Z', 'v0.4.0'),
      null,
    )
    assert.equal(
      isFreshInfraOutcome(last, '2026-08-17T01:00:00Z', 'v0.3.32'),
      'succeeded',
    )
  })

  it('keeps polling while the helper is still pending', () => {
    assert.equal(
      isFreshInfraOutcome(
        { ...last, status: 'pending' },
        '2026-08-17T01:00:00Z',
        '',
      ),
      null,
    )
  })
})
