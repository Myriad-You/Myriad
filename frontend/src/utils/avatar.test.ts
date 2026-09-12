import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { localFallbackAvatar, resolveAvatar } from './avatar'

function decodeSvg(dataUri: string): string {
  assert.ok(
    dataUri.startsWith('data:image/svg+xml;charset=utf-8,'),
    `unexpected data uri: ${dataUri.slice(0, 40)}`,
  )
  return decodeURIComponent(
    dataUri.slice('data:image/svg+xml;charset=utf-8,'.length),
  )
}

describe('localFallbackAvatar', () => {
  it('生成合法 SVG，且同一个名字恒定同一张脸', () => {
    const a = localFallbackAvatar('染川瞳')
    const b = localFallbackAvatar('染川瞳')
    assert.equal(a, b)

    const svg = decodeSvg(a)
    assert.match(svg, /^<svg xmlns="http:\/\/www\.w3\.org\/2000\/svg"/)
    assert.match(svg, /<\/svg>$/)
    assert.match(svg, /<text[^>]*>染<\/text>/)
  })

  it('不同名字给不同底色', () => {
    const colors = new Set(
      ['alice', 'bob', 'carol', 'dave'].map(
        (n) => decodeSvg(localFallbackAvatar(n)).match(/fill="(#[0-9a-f]{6})"/)?.[1],
      ),
    )
    assert.ok(colors.size > 1)
  })

  it('emoji 取整个字位簇，不会截出半个代理对', () => {
    const svg = decodeSvg(localFallbackAvatar('🐈 cat'))
    const letter = svg.match(/<text[^>]*>([^<]*)<\/text>/)?.[1]
    assert.equal(letter, '🐈')
  })

  it('转义 XML 元字符，否则整张 SVG 不渲染', () => {
    const svg = decodeSvg(localFallbackAvatar('<script>'))
    assert.match(svg, /<text[^>]*>&lt;<\/text>/)
    assert.ok(!svg.includes('<script>'))
  })

  it('空名字也要有脸', () => {
    for (const seed of ['', '   ', null, undefined]) {
      const svg = decodeSvg(localFallbackAvatar(seed))
      assert.match(svg, /<text[^>]*>U<\/text>/)
    }
  })
})

describe('resolveAvatar', () => {
  it('防盗链 CDN 包一层站内代理', () => {
    assert.match(
      resolveAvatar('https://i0.hdslb.com/bfs/face/a.jpg', 'x'),
      /^\/api\/proxy\/image\?url=/,
    )
  })

  it('健康 CDN 原样返回', () => {
    const raw = 'https://avatars.githubusercontent.com/u/1?v=4'
    assert.equal(resolveAvatar(raw, 'x'), raw)
  })

  it('已代理地址不二次包装', () => {
    const once = resolveAvatar('https://i0.hdslb.com/bfs/face/a.jpg', 'x')
    assert.equal(resolveAvatar(once, 'x'), once)
  })

  it('空地址回落本地生成，而不是打 ui-avatars.com', () => {
    for (const empty of [null, undefined, '', '   ']) {
      const out = resolveAvatar(empty, 'Zed')
      assert.ok(out.startsWith('data:image/svg+xml'))
      assert.ok(!out.includes('ui-avatars.com'))
    }
  })
})
