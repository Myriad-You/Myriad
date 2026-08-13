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
      'storage:read',
      'network:fetch',
      'ai:generate',
      'platform:read',
      'ui:theme:read',
      'ui:theme:subscribe',
      'ui:confirm',
      'ui:fullscreen',
      'ui:openUrl',
      'media:read',
    ]
    assert.deepEqual(selectPreviewGrantedPermissions(declared), [
      'storage:read',
      'ui:theme:read',
      'ui:confirm',
      'ui:fullscreen',
      'ui:openUrl',
    ])
    assert.ok(!selectPreviewGrantedPermissions(declared).includes('network:fetch'))
    assert.ok(!selectPreviewGrantedPermissions(declared).includes('ai:generate'))
    // Least privilege: declaring the subscription half does not grant it —
    // preview only ever grants the read half, never subscribe.
    assert.ok(!selectPreviewGrantedPermissions(declared).includes('ui:theme:subscribe'))
  })

  it('is deny-by-default when declarations are empty or missing', () => {
    assert.deepEqual(selectPreviewGrantedPermissions([]), [])
    assert.deepEqual(selectPreviewGrantedPermissions(undefined), [])
    assert.deepEqual(selectPreviewGrantedPermissions(null), [])
  })

  it('does not auto-grant preview allowlist entries that were not declared', () => {
    assert.deepEqual(selectPreviewGrantedPermissions(['ui:theme:read']), [
      'ui:theme:read',
    ])
    assert.deepEqual(selectPreviewGrantedPermissions(['network:fetch']), [])
  })

  it('keeps allowlist stable for host docs and backend parity', () => {
    assert.deepEqual([...PREVIEW_PERMISSIONS], [
      'storage:read',
      'ui:theme:read',
      'ui:confirm',
      'ui:fullscreen',
      'ui:openUrl',
    ])
    for (const permission of PREVIEW_PERMISSIONS) {
      assert.equal(isPreviewPermission(permission), true)
    }
    assert.equal(isPreviewPermission('network:fetch'), false)
    assert.equal(isPreviewPermission('ui:theme:subscribe'), false)
  })
})
