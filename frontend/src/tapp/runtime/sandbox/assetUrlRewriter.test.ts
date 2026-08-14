import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  isSandboxedFetchUrl,
  normalizeDeclaredAssetPath,
  resolveDeclaredAssetPath,
  rewriteAssetUrl,
} from './assetUrlRewriter.ts'

describe('normalizeDeclaredAssetPath', () => {
  it('keeps declared assets/ paths and strips query or wrappers', () => {
    assert.equal(normalizeDeclaredAssetPath('assets/check.png'), 'assets/check.png')
    assert.equal(
      normalizeDeclaredAssetPath('./assets/models/cube.glb?v=1#mesh'),
      'assets/models/cube.glb',
    )
    assert.equal(
      normalizeDeclaredAssetPath('/install/root/assets/check.png'),
      'assets/check.png',
    )
  })

  it('rejects network URLs, traversal, and non-assets paths', () => {
    assert.equal(normalizeDeclaredAssetPath('https://unpkg.com/three'), '')
    assert.equal(normalizeDeclaredAssetPath('//cdn.example/a.png'), '')
    assert.equal(normalizeDeclaredAssetPath('assets/../secret.bin'), '')
    assert.equal(normalizeDeclaredAssetPath('textures/check.png'), '')
    assert.equal(normalizeDeclaredAssetPath('blob:https://local/1'), '')
  })
})

describe('rewriteAssetUrl', () => {
  const urls = {
    'assets/check.png': 'blob:opaque/check',
    'assets/cube.glb': 'blob:opaque/cube',
  }

  it('maps declared paths and unique basenames', () => {
    assert.equal(rewriteAssetUrl('assets/check.png', urls), 'blob:opaque/check')
    assert.equal(rewriteAssetUrl('cube.glb', urls), 'blob:opaque/cube')
    assert.equal(rewriteAssetUrl('blob:opaque/other', urls), 'blob:opaque/other')
  })

  it('does not guess when a basename is ambiguous or remote', () => {
    const ambiguous = {
      ...urls,
      'assets/alt/check.png': 'blob:opaque/alt',
    }
    assert.equal(rewriteAssetUrl('check.png', ambiguous), '')
    assert.equal(rewriteAssetUrl('https://cdn.example/cube.glb', urls), '')
  })
})

describe('resolveDeclaredAssetPath', () => {
  const urls = {
    'assets/check.png': 'blob:opaque/check',
    'assets/cube.glb': 'blob:opaque/cube',
  }

  it('returns declared paths and unique basenames', () => {
    assert.equal(resolveDeclaredAssetPath('assets/check.png', urls), 'assets/check.png')
    assert.equal(resolveDeclaredAssetPath('cube.glb', urls), 'assets/cube.glb')
  })

  it('does not resolve remote or ambiguous names', () => {
    assert.equal(resolveDeclaredAssetPath('https://cdn.example/cube.glb', urls), '')
    assert.equal(
      resolveDeclaredAssetPath('check.png', {
        ...urls,
        'assets/alt/check.png': 'blob:opaque/alt',
      }),
      '',
    )
  })
})

describe('isSandboxedFetchUrl', () => {
  it('allows only blob and data', () => {
    assert.equal(isSandboxedFetchUrl('blob:https://opaque/1'), true)
    assert.equal(isSandboxedFetchUrl('data:application/octet-stream;base64,AA'), true)
    assert.equal(isSandboxedFetchUrl('https://example.com/a'), false)
    assert.equal(isSandboxedFetchUrl('/assets/check.png'), false)
  })
})
