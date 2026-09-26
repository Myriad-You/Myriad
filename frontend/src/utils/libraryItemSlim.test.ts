import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  preferCardCoverUrl,
  slimLibraryItem,
  slimLibraryMetadata,
} from './libraryItemSlim'

describe('preferCardCoverUrl', () => {
  it('rewrites bangumi large to common', () => {
    assert.equal(
      preferCardCoverUrl('https://lain.bgm.tv/pic/cover/l/ab.jpg'),
      'https://lain.bgm.tv/pic/cover/c/ab.jpg',
    )
  })

  it('leaves bangumi /r/{width}/pic/cover/l/ resize paths alone', () => {
    assert.equal(
      preferCardCoverUrl('https://lain.bgm.tv/r/400/pic/cover/l/ab.jpg'),
      'https://lain.bgm.tv/r/400/pic/cover/l/ab.jpg',
    )
    const proxied = `/api/proxy/image?url=${encodeURIComponent(
      'https://lain.bgm.tv/r/400/pic/cover/l/ab.jpg',
    )}`
    assert.equal(preferCardCoverUrl(proxied), proxied)
  })

  it('adds netease param when missing', () => {
    const out = preferCardCoverUrl('https://p2.music.126.net/xx.jpg')
    assert.ok(out?.includes('param=288y288'))
  })

  it('downsizes oversized netease params and keeps smaller ones', () => {
    assert.equal(
      preferCardCoverUrl('https://p2.music.126.net/xx.jpg?param=400y400'),
      'https://p2.music.126.net/xx.jpg?param=288y288',
    )
    assert.equal(
      preferCardCoverUrl('https://p2.music.126.net/xx.jpg?param=200y200'),
      'https://p2.music.126.net/xx.jpg?param=200y200',
    )
  })

  it('leaves non-netease hosts that merely mention netease untouched', () => {
    for (const url of [
      'https://evil.example/x.jpg?ref=music.126.net',
      'https://music.126.net.evil.example/x.jpg',
      'https://evil.example/music.163.com/x.jpg',
    ]) {
      assert.equal(preferCardCoverUrl(url), url)
    }
  })

  it('adds bilibili card width without cropping', () => {
    assert.equal(
      preferCardCoverUrl('https://i0.hdslb.com/bfs/bangumi/image/x.jpg'),
      'https://i0.hdslb.com/bfs/bangumi/image/x.jpg@528w.webp',
    )
    assert.equal(
      preferCardCoverUrl('https://i2.hdslb.com/bfs/archive/c.jpg?spm=1'),
      'https://i2.hdslb.com/bfs/archive/c.jpg@528w.webp?spm=1',
    )
    const already = 'https://i0.hdslb.com/bfs/archive/c.jpg@672w_378h_1c.webp'
    assert.equal(preferCardCoverUrl(already), already)
    assert.equal(
      preferCardCoverUrl('https://i0.hdslb.com/bfs/face/a.gif'),
      'https://i0.hdslb.com/bfs/face/a.gif',
    )
    assert.equal(
      preferCardCoverUrl('https://hdslb.com.evil.com/bfs/archive/c.jpg'),
      'https://hdslb.com.evil.com/bfs/archive/c.jpg',
    )
    const proxied = `/api/proxy/image?url=${encodeURIComponent(
      'https://i0.hdslb.com/bfs/bangumi/image/x.jpg',
    )}`
    const out = preferCardCoverUrl(proxied)
    assert.ok(out?.includes(encodeURIComponent(
      'https://i0.hdslb.com/bfs/bangumi/image/x.jpg@528w.webp',
    )))
  })

  it('rewrites proxied bangumi large paths', () => {
    const out = preferCardCoverUrl(
      `/api/proxy/image?url=${
        encodeURIComponent('https://lain.bgm.tv/pic/cover/l/ab.jpg')}`,
    )
    assert.ok(out?.includes(encodeURIComponent('https://lain.bgm.tv/pic/cover/c/ab.jpg')))
  })
})

describe('slimLibraryMetadata', () => {
  it('keeps play/progress fields and drops bulk keys', () => {
    const slim = slimLibraryMetadata({
      id: 1,
      fee: 1,
      ar: [{ id: 9, name: 'A', tns: [] }],
      al: { name: 'Album', picUrl: 'https://p2.music.126.net/a.jpg', size: 99 },
      privilege: { fee: 1, maxBr: 999 },
      alias: ['x'],
      comment: 'long',
    })
    assert.equal(slim.fee, 1)
    assert.deepEqual(slim.ar, [{ name: 'A' }])
    assert.equal(slim.alias, undefined)
    assert.equal(slim.comment, undefined)
    assert.deepEqual(slim.privilege, { fee: 1 })
    assert.ok(String((slim.al as { picUrl?: string }).picUrl).includes('param=288y288'))
  })

  it('keeps bangumi progress subject totals', () => {
    const slim = slimLibraryMetadata({
      type: 3,
      ep_status: 5,
      subject: { id: 1, eps: 12, images: { large: 'x' }, summary: 'nope' },
      comment: 'drop',
    })
    assert.equal(slim.ep_status, 5)
    assert.deepEqual(slim.subject, { id: 1, eps: 12 })
    assert.equal(slim.comment, undefined)
  })
})

describe('slimLibraryItem', () => {
  it('compacts cover and metadata together', () => {
    const item = slimLibraryItem({
      id: 'bangumi_subject_1',
      item_type: 'anime',
      title: 'T',
      cover: 'https://lain.bgm.tv/pic/cover/l/1.jpg',
      platform: 'Bangumi',
      metadata: { ep_status: 1, comment: 'x' },
    })
    assert.equal(item.cover, 'https://lain.bgm.tv/pic/cover/c/1.jpg')
    assert.equal((item.metadata as { comment?: string }).comment, undefined)
    assert.equal((item.metadata as { ep_status?: number }).ep_status, 1)
  })
})
