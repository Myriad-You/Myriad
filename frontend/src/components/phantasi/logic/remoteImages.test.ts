import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  htmlImageSources,
  isExternalImageUrl,
  keepMarkdownImages,
  keepRemoteImages,
  markdownImageSources,
  rememberSaves,
  replaceMarkdownImageSrc,
} from './remoteImages.ts'

const SITE = ['https://my.blog']

describe('isExternalImageUrl', () => {
  it('只有别的网站的 http(s) 图片要存', () => {
    assert.ok(isExternalImageUrl('https://old.example/wp-content/uploads/a.jpg', SITE))
    assert.ok(isExternalImageUrl('http://cdn.example/a.png?x=1', SITE))
    for (const src of [
      'https://my.blog/uploads/a.png',
      'data:image/png;base64,aaaa',
      '/media/assets/3f2a1b4c-5d6e-7f80-91a2-b3c4d5e6f708/a.png',
      // 本站媒体挂着旧域名，也是本站的。
      'https://old-name.example/media/assets/3f2a1b4c-5d6e-7f80-91a2-b3c4d5e6f708/a.png',
      'https://old-name.example/api/phantasi/image-cache/aa/bb.png',
      'blob:https://my.blog/1',
      'ftp://old.example/a.png',
      'https://user:pw@old.example/a.png',
      'not a url',
    ]) {
      assert.equal(isExternalImageUrl(src, SITE), false, src)
    }
  })
})

describe('markdown 图片地址', () => {
  it('认行内图片、尖括号地址和图片引用的定义行，代码里的不算', () => {
    const md = [
      '![a](https://x.example/a.png "标题") 和 ![b](<https://x.example/b c.png>)',
      '',
      '[链接](https://x.example/page.png) 不是图片',
      '',
      '![c][ref] 和 ![Shortcut]',
      '',
      '`![d](https://x.example/d.png)`',
      '',
      '```',
      '![e](https://x.example/e.png)',
      '```',
      '',
      '[ref]: https://x.example/c.png',
      '[shortcut]: <https://x.example/s.png> "t"',
      '[unused]: https://x.example/u.png',
    ].join('\n')
    assert.deepEqual(markdownImageSources(md), [
      'https://x.example/a.png',
      'https://x.example/b c.png',
      'https://x.example/c.png',
      'https://x.example/s.png',
    ])
  })

  it('只换图片的地址，同样地址的链接和代码不动', () => {
    const md = [
      '[![a](https://x.example/a.png)](https://x.example/a.png)',
      '![again](https://x.example/a.png)',
      '`![code](https://x.example/a.png)`',
      'https://x.example/a.png',
    ].join('\n')
    assert.equal(
      replaceMarkdownImageSrc(md, 'https://x.example/a.png', '/media/assets/1/a.png'),
      [
        '[![a](/media/assets/1/a.png)](https://x.example/a.png)',
        '![again](/media/assets/1/a.png)',
        '`![code](https://x.example/a.png)`',
        'https://x.example/a.png',
      ].join('\n'),
    )
  })

  it('换引用图片的定义行', () => {
    const md = '![c][1]\n\n[1]: https://x.example/c.png'
    assert.equal(
      replaceMarkdownImageSrc(md, 'https://x.example/c.png', '/media/assets/1/c.png'),
      '![c][1]\n\n[1]: /media/assets/1/c.png',
    )
  })
})

describe('htmlImageSources', () => {
  it('取 img 的原地址并解开实体', () => {
    const html = '<p><img src="https://x.example/a.png?a=1&amp;b=2" alt=""><img src="/shown.png" data-src="https://x.example/b.png"></p>'
    assert.deepEqual(htmlImageSources(html), [
      'https://x.example/a.png?a=1&b=2',
      'https://x.example/b.png',
    ])
  })
})

describe('keepRemoteImages', () => {
  it('并行有上限、超出上限的和失败的保留原地址，一张失败不影响别的', async () => {
    let running = 0
    let peak = 0
    const saved: string[] = []
    const result = await keepRemoteImages(
      ['u1', 'u2', 'bad', 'u3', 'u1', 'u4'],
      async (url) => {
        running += 1
        peak = Math.max(peak, running)
        await new Promise((resolve) => setTimeout(resolve, 5))
        running -= 1
        if (url === 'bad') throw new Error('nope')
        return `/media/${url}`
      },
      { limit: 4, concurrency: 2, onSaved: (from) => saved.push(from) },
    )
    assert.equal(peak, 2)
    assert.deepEqual([...result.saved.keys()].sort(), ['u1', 'u2', 'u3'])
    assert.deepEqual(saved.sort(), ['u1', 'u2', 'u3'])
    assert.deepEqual(result.kept.sort(), ['bad', 'u4'])
  })

  it('取消后不再开始新的', async () => {
    const controller = new AbortController()
    const asked: string[] = []
    const result = await keepRemoteImages(
      ['a', 'b', 'c'],
      async (url) => {
        asked.push(url)
        controller.abort()
        return `/media/${url}`
      },
      { concurrency: 1, signal: controller.signal },
    )
    assert.deepEqual(asked, ['a'])
    assert.deepEqual(result.kept, ['b', 'c'])
  })
})

describe('rememberSaves', () => {
  it('同一个地址只存一次，失败的下次再试', async () => {
    const calls: string[] = []
    let fail = true
    const save = rememberSaves(async (url) => {
      calls.push(url)
      if (url === 'flaky' && fail) throw new Error('once')
      return `/media/${url}`
    })
    const [a, b] = await Promise.all([save('x'), save('x')])
    assert.equal(a, b)
    await assert.rejects(save('flaky'))
    fail = false
    assert.equal(await save('flaky'), '/media/flaky')
    assert.deepEqual(calls, ['x', 'flaky', 'flaky'])
  })

  it('一个外站一张都存不成、接连失败够了次数，余下的不再去等', async () => {
    const calls: string[] = []
    const save = rememberSaves(async (url) => {
      calls.push(url)
      if (url.startsWith('https://dead.example/')) throw new Error('timeout')
      return '/media/ok'
    }, { giveUpAfter: 2 })
    for (const name of ['a', 'b', 'c', 'd']) {
      await assert.rejects(save(`https://dead.example/${name}.png`))
    }
    assert.equal(await save('https://alive.example/a.png'), '/media/ok')
    assert.deepEqual(calls, [
      'https://dead.example/a.png',
      'https://dead.example/b.png',
      'https://alive.example/a.png',
    ])
  })
})

describe('keepMarkdownImages', () => {
  it('导入的一篇：外站图片换成本站地址，本站的不动，存不下的留原样', async () => {
    const md = [
      '![a](https://old.example/wp-content/uploads/a.jpg)',
      '![mine](https://my.blog/media/assets/x/y.png)',
      '![gone](https://old.example/wp-content/uploads/gone.jpg)',
    ].join('\n\n')
    const asked: string[] = []
    const result = await keepMarkdownImages(md, SITE, async (url) => {
      asked.push(url)
      if (url.endsWith('gone.jpg')) throw new Error('404')
      return '/media/assets/1/a.jpg'
    })
    assert.deepEqual(asked.sort(), [
      'https://old.example/wp-content/uploads/a.jpg',
      'https://old.example/wp-content/uploads/gone.jpg',
    ])
    assert.equal(result.saved, 1)
    assert.deepEqual(result.kept, ['https://old.example/wp-content/uploads/gone.jpg'])
    assert.equal(
      result.markdown,
      [
        '![a](/media/assets/1/a.jpg)',
        '![mine](https://my.blog/media/assets/x/y.png)',
        '![gone](https://old.example/wp-content/uploads/gone.jpg)',
      ].join('\n\n'),
    )
  })

  it('没有外站图片就不发请求', async () => {
    const result = await keepMarkdownImages('纯文字\n\n![](data:image/png;base64,aa)', SITE, async () => {
      throw new Error('should not be called')
    })
    assert.equal(result.saved, 0)
    assert.deepEqual(result.kept, [])
  })
})
