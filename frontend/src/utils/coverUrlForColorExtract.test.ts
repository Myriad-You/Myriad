import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { coverUrlForColorExtract } from './coverUrlForColorExtract.ts'

describe('coverUrlForColorExtract', () => {
  it('shrinks netease cover via param=', () => {
    const full = 'https://p1.music.126.net/abc/cover.jpg'
    assert.equal(
      coverUrlForColorExtract(full),
      'https://p1.music.126.net/abc/cover.jpg?param=150y150',
    )
  })

  it('rewrites existing netease param to 150y150', () => {
    const full = 'https://music.163.com/cover.jpg?param=400y400'
    assert.equal(
      coverUrlForColorExtract(full),
      'https://music.163.com/cover.jpg?param=150y150',
    )
  })

  it('appends param when netease URL already has query', () => {
    const full = 'https://p2.music.126.net/x.jpg?foo=1'
    assert.equal(
      coverUrlForColorExtract(full),
      'https://p2.music.126.net/x.jpg?foo=1&param=150y150',
    )
  })

  it('shrinks QQ album art dimensions', () => {
    const full =
      'https://y.gtimg.cn/music/photo_new/T002R300x300M000abc.jpg'
    assert.equal(
      coverUrlForColorExtract(full),
      'https://y.gtimg.cn/music/photo_new/T002R150x150M000abc.jpg',
    )
  })

  it('leaves unknown hosts unchanged', () => {
    const full = 'https://cdn.example.com/art.png'
    assert.equal(coverUrlForColorExtract(full), full)
  })

  it('handles empty string', () => {
    assert.equal(coverUrlForColorExtract(''), '')
  })

  it('rewrites size inside image proxy query', () => {
    const full =
      `/api/proxy/image?url=${
      encodeURIComponent('https://p1.music.126.net/abc/cover.jpg')}`
    const out = coverUrlForColorExtract(full)
    assert.match(out, /^\/api\/proxy\/image\?url=/)
    const decoded = decodeURIComponent(out.split('url=')[1] || '')
    assert.equal(decoded, 'https://p1.music.126.net/abc/cover.jpg?param=150y150')
  })
})
