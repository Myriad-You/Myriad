import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  isPreviewPermission,
  PREVIEW_PERMISSIONS,
  selectPreviewGrantedPermissions,
} from './previewGrants.ts'

describe('previewGrants (MYR-024)', () => {
  it('does not treat full manifest permissions as granted', () => {
    const declared = [
      'storage',
      'network:fetch',
      'ai:generate',
      'platform:read',
      'ui:theme',
      'ui:confirm',
      'ui:fullscreen',
      'media:read',
    ]
    assert.deepEqual(selectPreviewGrantedPermissions(declared), [
      'storage',
      'ui:theme',
      'ui:confirm',
      'ui:fullscreen',
    ])
    assert.ok(!selectPreviewGrantedPermissions(declared).includes('network:fetch'))
    assert.ok(!selectPreviewGrantedPermissions(declared).includes('ai:generate'))
  })

  it('is deny-by-default when declarations are empty or missing', () => {
    assert.deepEqual(selectPreviewGrantedPermissions([]), [])
    assert.deepEqual(selectPreviewGrantedPermissions(undefined), [])
    assert.deepEqual(selectPreviewGrantedPermissions(null), [])
  })

  it('does not auto-grant preview allowlist entries that were not declared', () => {
    assert.deepEqual(selectPreviewGrantedPermissions(['ui:theme']), ['ui:theme'])
    assert.deepEqual(selectPreviewGrantedPermissions(['network:fetch']), [])
  })

  it('keeps allowlist stable for host docs and backend parity', () => {
    assert.deepEqual([...PREVIEW_PERMISSIONS], [
      'storage',
      'ui:theme',
      'ui:confirm',
      'ui:fullscreen',
    ])
    for (const permission of PREVIEW_PERMISSIONS) {
      assert.equal(isPreviewPermission(permission), true)
    }
    assert.equal(isPreviewPermission('network:fetch'), false)
  })
})
