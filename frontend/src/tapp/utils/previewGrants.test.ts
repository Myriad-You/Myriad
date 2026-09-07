import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  isPlaygroundPreviewExpectedError,
  isPreviewPermission,
  PREVIEW_PERMISSIONS,
  PREVIEW_UNAVAILABLE_CODE,
  previewUnavailableMessage,
  selectPreviewGrantedPermissions,
} from './previewGrants.ts'

describe('previewGrants (MYR-024)', () => {
  it('does not treat full manifest permissions as granted', () => {
    const declared = [
      'storage',
      'storage:read',
      'storage:write',
      'network:fetch',
      'ai:generate',
      'platform:read',
      'ui:theme',
      'ui:confirm',
      'ui:fullscreen',
      'ui:openUrl',
      'media:read',
    ]
    assert.deepEqual(selectPreviewGrantedPermissions(declared), [
      'storage:read',
      'storage:write',
      'ui:theme',
      'ui:confirm',
      'ui:fullscreen',
      'ui:openUrl',
    ])
    assert.ok(!selectPreviewGrantedPermissions(declared).includes('network:fetch'))
    assert.ok(!selectPreviewGrantedPermissions(declared).includes('ai:generate'))
    // The retired coarse `storage` name is not a preview grant either.
    assert.ok(!selectPreviewGrantedPermissions(declared).includes('storage'))
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
      'storage:read',
      'storage:write',
      'ui:theme',
      'ui:confirm',
      'ui:fullscreen',
      'ui:openUrl',
    ])
    for (const permission of PREVIEW_PERMISSIONS) {
      assert.equal(isPreviewPermission(permission), true)
    }
    assert.equal(isPreviewPermission('network:fetch'), false)
    assert.equal(isPreviewPermission('storage'), false)
  })

  it('does not treat preview-disabled host APIs as repairable runtime bugs', () => {
    assert.equal(PREVIEW_UNAVAILABLE_CODE, 'PREVIEW_UNAVAILABLE')
    assert.match(
      previewUnavailableMessage('ai.tasks.create'),
      /ai\.tasks\.create is unavailable in temporary preview/,
    )
    assert.equal(
      isPlaygroundPreviewExpectedError(
        previewUnavailableMessage('ai.tasks.create'),
      ),
      true,
    )
    assert.equal(
      isPlaygroundPreviewExpectedError('Unknown action: ai.tasks.create'),
      true,
    )
    assert.equal(
      isPlaygroundPreviewExpectedError(
        'Permission denied: Missing permission: ai:image',
      ),
      true,
    )
    assert.equal(
      isPlaygroundPreviewExpectedError(
        'Permission denied: Missing permission: ai:generate, ai:analyze, ai:chat, ai:image, ai:search',
      ),
      true,
    )
    assert.equal(
      isPlaygroundPreviewExpectedError(
        'Permission denied: Missing permission: storage:read',
      ),
      false,
    )
    assert.equal(
      isPlaygroundPreviewExpectedError('ReferenceError: foo is not defined'),
      false,
    )
  })
})
