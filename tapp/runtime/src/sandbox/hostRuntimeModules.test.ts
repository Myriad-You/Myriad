import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { manifestRequestsRuntimeModule } from './hostRuntimeModules.ts'

describe('host runtime modules', () => {
  it('only matches an explicit three declaration', () => {
    assert.equal(manifestRequestsRuntimeModule(['three'], 'three'), true)
    assert.equal(manifestRequestsRuntimeModule([], 'three'), false)
    assert.equal(manifestRequestsRuntimeModule(undefined, 'three'), false)
    assert.equal(manifestRequestsRuntimeModule(['three'], 'ammo'), false)
  })
})
