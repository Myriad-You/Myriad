/**
 * Run from frontend/:
 *   pnpm test:unit -- src/utils/codeSplitting.test.ts
 */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { CRITICAL_PRELOAD_ROUTES } from './codeSplitting.ts'

describe('CRITICAL_PRELOAD_ROUTES', () => {
  it('does not prefetch Config or instance-specific Tapp pages', () => {
    assert.deepEqual([...CRITICAL_PRELOAD_ROUTES], [
      'library',
      'tapp',
      'tappStore',
    ])
    assert.ok(
      !(CRITICAL_PRELOAD_ROUTES as readonly string[]).includes('config'),
    )
  })
})
