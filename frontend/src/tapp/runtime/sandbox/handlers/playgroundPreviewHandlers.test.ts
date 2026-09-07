import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  previewAssetFromPackage,
  previewContextApp,
  previewContextPlayer,
  previewContextSystem,
  previewContextUser,
} from './playgroundPreviewHandlers.ts'

describe('previewAssetFromPackage', () => {
  it('decodes data URLs and raw base64 under assets/', () => {
    const png =
      'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=='
    const fromDataUrl = previewAssetFromPackage(
      'assets/icon.png',
      `data:image/png;base64,${png}`,
    )
    assert.ok(fromDataUrl)
    assert.equal(fromDataUrl.mimeType, 'image/png')
    assert.equal(fromDataUrl.base64, png)
    assert.ok(fromDataUrl.size > 0)

    const fromRaw = previewAssetFromPackage('assets/mesh.glb', png)
    assert.ok(fromRaw)
    assert.equal(fromRaw.mimeType, 'model/gltf-binary')
    assert.equal(fromRaw.base64, png)
  })

  it('rejects path traversal and non-assets paths', () => {
    assert.equal(previewAssetFromPackage('../secret.png', 'AAAA'), null)
    assert.equal(previewAssetFromPackage('page.html', 'AAAA'), null)
  })
})

describe('preview context stubs', () => {
  it('matches host getApp/getSystem fields instead of inventing mode:page', () => {
    const app = previewContextApp({
      id: 'com.example.preview',
      manifest: {
        id: 'com.example.preview',
        name: 'Preview',
        version: '1.0.0',
        core: { entry: 'core.js' },
        permissions: [],
        category: 'utility',
      },
      status: 'running',
      installedAt: '2026-01-01T00:00:00Z',
      grantedPermissions: [],
      userRole: 'admin',
    })
    assert.equal('mode' in app, false)
    assert.equal(app.version, '1.0.0')
    assert.ok(app.features)
    const system = previewContextSystem()
    assert.equal(system.preview, true)
    assert.equal(system.runtime, 'tapp-playground')
    assert.equal(system.online, true)
    const player = previewContextPlayer()
    assert.equal(player.isPlaying, false)
    assert.equal(player.currentTrack, null)
    const user = previewContextUser({
      id: 'com.example.preview',
      manifest: {
        id: 'com.example.preview',
        name: 'Preview',
        version: '1.0.0',
        core: { entry: 'core.js' },
        permissions: [],
        category: 'utility',
      },
      status: 'running',
      installedAt: '2026-01-01T00:00:00Z',
      grantedPermissions: [],
      userRole: 'admin',
    })
    assert.equal(user.id, 'user_preview')
    assert.equal(user.isAdmin, true)
    assert.equal(user.authenticated, true)
    assert.deepEqual(user.connectedPlatforms, [])
    assert.equal(user.preferences.language, 'en-US')
  })
})
