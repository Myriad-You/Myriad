import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import {
  needsImageProxy,
  normalizeJsonMediaUrls,
  proxyImageUrl,
} from './proxyImageUrl'

const sharedHosts = JSON.parse(
  readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), '../../../shared/image_proxy_hosts.json'),
    'utf8',
  ),
) as { markers: string[] }

describe('proxyImageUrl', () => {
  it('loads markers from shared/image_proxy_hosts.json', () => {
    assert.ok(sharedHosts.markers.includes('hdslb.com'))
    assert.equal(needsImageProxy('https://i0.hdslb.com/x.jpg'), true)
  })

  it('proxies true hotlink CDNs', () => {
    const cases = [
      'https://i1.hdslb.com/bfs/face/abc.jpg',
      'https://avatars.steamstatic.com/xxx_full.jpg',
      'https://p1.music.126.net/cover.jpg',
      'https://lain.bgm.tv/pic/cover/l/1.jpg',
      'https://pbs.twimg.com/profile_images/1/normal.jpg',
      'https://steamcdn-a.akamaihd.net/steamcommunity/public/images/avatars/a.jpg',
    ]
    for (const raw of cases) {
      const out = proxyImageUrl(raw)
      assert.ok(out, raw)
      assert.match(out!, /\/api\/proxy\/image\?url=/, raw)
      assert.equal(needsImageProxy(raw), true)
    }
  })

  it('does not auto-proxy healthy CDNs', () => {
    for (const raw of [
      'https://avatars.githubusercontent.com/u/1?v=4',
      'https://i.ytimg.com/vi/abc/hqdefault.jpg',
      'https://cdn.discordapp.com/avatars/1/2.png',
      'https://enka.network/ui/UI_AvatarIcon_Ayaka.png',
      'https://ui-avatars.com/api/?name=A',
    ]) {
      assert.equal(proxyImageUrl(raw), raw)
      assert.equal(needsImageProxy(raw), false)
    }
  })

  it('does not double-proxy', () => {
    const once = proxyImageUrl('https://i0.hdslb.com/bfs/face/x.jpg')!
    assert.equal(proxyImageUrl(once), once)
  })

  it('upgrades protocol-relative and http', () => {
    const out = proxyImageUrl('//i0.hdslb.com/bfs/face/x.jpg')!
    assert.match(out, /url=https%3A%2F%2Fi0\.hdslb\.com/)
  })

  it('normalizeJsonMediaUrls rewrites nested trees', () => {
    const out = normalizeJsonMediaUrls({
      avatar: 'https://i0.hdslb.com/bfs/face/a.jpg',
      library_items: [
        {
          cover:
            'https://cdn.cloudflare.steamstatic.com/steam/apps/1/header.jpg',
        },
      ],
      name: 'keep',
    })
    assert.match(String(out.avatar), /\/api\/proxy\/image/)
    assert.match(String(out.library_items[0].cover), /\/api\/proxy\/image/)
    assert.equal(out.name, 'keep')
  })
})
