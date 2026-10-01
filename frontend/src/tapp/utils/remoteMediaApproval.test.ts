import type { TappManifest } from '../types'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { declaredRemoteMedia, pendingRemoteMedia } from './remoteMediaApproval.ts'

function manifest(permissions: string[], remoteMedia?: string[]): TappManifest {
  return {
    id: 'com.example.media',
    name: 'Media',
    version: '1.0.0',
    core: { entry: 'core.js' },
    category: 'utility',
    permissions,
    remoteMedia,
  } as TappManifest
}

describe('remoteMediaApproval', () => {
  it('ignores hosts without the media:remote declaration', () => {
    assert.deepEqual(declaredRemoteMedia({ manifest: manifest([], ['a.example.com']) }), [])
  })

  it('lists declared hosts the approver has not approved yet', () => {
    const m = manifest(['media:remote'], ['a.example.com', '*.b.example.com'])
    assert.deepEqual(
      pendingRemoteMedia({ manifest: m, approvedRemoteMedia: ['a.example.com'] }),
      ['*.b.example.com'],
    )
    assert.deepEqual(
      pendingRemoteMedia({ manifest: m, approvedRemoteMedia: ['a.example.com', '*.b.example.com'] }),
      [],
    )
  })

  it('reports nothing to viewers who cannot see approvals', () => {
    const m = manifest(['media:remote'], ['a.example.com'])
    assert.deepEqual(pendingRemoteMedia({ manifest: m }), [])
  })
})
