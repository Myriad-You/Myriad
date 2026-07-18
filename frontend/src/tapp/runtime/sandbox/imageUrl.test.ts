import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { toSandboxImageUrl } from './imageUrl'

describe('toSandboxImageUrl', () => {
  const origin = 'http://localhost:1102'

  it('routes remote HTTP images through the host image proxy', () => {
    assert.equal(
      toSandboxImageUrl(
        'https://p1.music.126.net/cover/image.jpg?size=300',
        origin,
      ),
      '/api/proxy/image?url=https%3A%2F%2Fp1.music.126.net%2Fcover%2Fimage.jpg%3Fsize%3D300',
    )
  })

  it('preserves CSP-compatible image URLs', () => {
    assert.equal(
      toSandboxImageUrl('/api/proxy/image?url=x', origin),
      '/api/proxy/image?url=x',
    )
    assert.equal(
      toSandboxImageUrl('data:image/png;base64,AA==', origin),
      'data:image/png;base64,AA==',
    )
    assert.equal(
      toSandboxImageUrl('blob:http://localhost:1102/id', origin),
      'blob:http://localhost:1102/id',
    )
    assert.equal(
      toSandboxImageUrl('http://localhost:1102/assets/icon.png', origin),
      'http://localhost:1102/assets/icon.png',
    )
  })

  it('does not turn missing or non-string values into fetchable URLs', () => {
    assert.equal(toSandboxImageUrl(null, origin), '')
    assert.equal(toSandboxImageUrl(undefined, origin), '')
    assert.equal(toSandboxImageUrl(42, origin), '')
  })
})
