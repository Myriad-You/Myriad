/**
 *   pnpm exec tsx --test src/services/agent/capabilityActions.test.ts
 */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { normalizeCapabilityActions } from './agentApi.ts'

describe('normalizeCapabilityActions', () => {
  it('keeps snake_case string actions from BE IntentAction', () => {
    assert.deepEqual(normalizeCapabilityActions(['query', 'analyze']), [
      'query',
      'analyze',
    ])
  })

  it('coerces newtype object variants', () => {
    assert.deepEqual(normalizeCapabilityActions([{ unknown: 'foo' }]), [
      'unknown:foo',
    ])
  })

  it('ignores empty / non-array input', () => {
    assert.deepEqual(normalizeCapabilityActions(null), [])
    assert.deepEqual(normalizeCapabilityActions(undefined), [])
    assert.deepEqual(normalizeCapabilityActions('query'), [])
    assert.deepEqual(normalizeCapabilityActions(['', '  ', 1, null]), [])
  })
})
