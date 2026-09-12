/** @vitest-environment node */

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import {
  absolutizeManifestUrls,
  clampPwaLogoScale,
  computeContainedLogoRect,
  DEFAULT_PWA_LOGO_SCALE,
  PWA_ICON_BACKGROUND,
  PWA_LOGO_SCALE_MAX,
  PWA_LOGO_SCALE_MIN,
  pwaIconIsCanvasReadable,
  resolveManifestUrl,
  resolvePwaIconSourceUrl,
} from './pwa'

const TINY_PNG_DATA_URL =
  'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=='

const here = dirname(fileURLToPath(import.meta.url))

describe('PWA assets', () => {
  it('ships an install-ready web app manifest with 192/512 PNG icons', () => {
    const manifestPath = resolve(here, '../../public/manifest.webmanifest')
    const manifest = JSON.parse(readFileSync(manifestPath, 'utf8')) as {
      name: string
      short_name: string
      start_url: string
      display: string
      icons: Array<{ src: string; sizes: string; type: string }>
    }

    assert.ok(manifest.name)
    assert.ok(manifest.short_name)
    assert.equal(manifest.start_url, '/')
    assert.equal(manifest.display, 'standalone')

    const sizes = new Set(manifest.icons.map((i) => i.sizes))
    assert.ok(sizes.has('192x192'), 'needs 192x192 icon for installability')
    assert.ok(sizes.has('512x512'), 'needs 512x512 icon for installability')

    for (const size of ['192x192', '512x512'] as const) {
      const icon = manifest.icons.find((i) => i.sizes === size)
      assert.ok(icon)
      assert.match(icon!.type, /png/i)
      const iconFile = resolve(
        here,
        '../../public',
        icon!.src.replaceAll(/^\//g, ''),
      )
      assert.ok(
        readFileSync(iconFile).length > 100,
        `${icon!.src} should exist and be non-empty`,
      )
    }
  })
})

describe('blob manifest URL absolutization', () => {
  const origin = 'https://kiseki.blog'

  it('resolves relative paths against site origin', () => {
    assert.equal(resolveManifestUrl('/', origin), 'https://kiseki.blog/')
    assert.equal(
      resolveManifestUrl('/icons/pwa/icon-192.png', origin),
      'https://kiseki.blog/icons/pwa/icon-192.png',
    )
    assert.equal(
      resolveManifestUrl('https://cdn.example/icon.png', origin),
      'https://cdn.example/icon.png',
    )
    assert.equal(resolveManifestUrl('', origin), undefined)
    assert.equal(resolveManifestUrl(null, origin), undefined)
  })

  it('preserves data: and blob: icon URLs (installable composed icons)', () => {
    assert.equal(resolveManifestUrl(TINY_PNG_DATA_URL, origin), TINY_PNG_DATA_URL)
    assert.equal(
      resolveManifestUrl('blob:https://kiseki.blog/uuid-here', origin),
      'blob:https://kiseki.blog/uuid-here',
    )
  })

  it('rewrites start_url, scope, id, and icon src for blob-served manifests', () => {
    const manifestPath = resolve(here, '../../public/manifest.webmanifest')
    const base = JSON.parse(readFileSync(manifestPath, 'utf8')) as Record<
      string,
      unknown
    >
    const next = absolutizeManifestUrls(
      { ...base, name: 'Kiseki', short_name: 'Kiseki' },
      origin,
    )

    assert.equal(next.start_url, 'https://kiseki.blog/')
    assert.equal(next.scope, 'https://kiseki.blog/')
    assert.equal(next.id, 'https://kiseki.blog/')
    assert.equal(next.name, 'Kiseki')

    const icons = next.icons as Array<{ src: string }>
    assert.ok(icons.length >= 2)
    for (const icon of icons) {
      assert.match(icon.src, /^https:\/\/kiseki\.blog\//)
    }
  })

  it('keeps composed data: PNG icons while absolutizing other fields', () => {
    const next = absolutizeManifestUrls(
      {
        name: 'Love on the page',
        short_name: 'Love on the',
        start_url: '/',
        scope: '/',
        id: '/',
        display: 'standalone',
        icons: [
          {
            src: TINY_PNG_DATA_URL,
            sizes: '192x192',
            type: 'image/png',
            purpose: 'any',
          },
          {
            src: TINY_PNG_DATA_URL,
            sizes: '512x512',
            type: 'image/png',
            purpose: 'any',
          },
        ],
      },
      origin,
    )

    assert.equal(next.start_url, 'https://kiseki.blog/')
    assert.equal(next.scope, 'https://kiseki.blog/')
    const icons = next.icons as Array<{ src: string; sizes: string }>
    assert.equal(icons.length, 2)
    for (const icon of icons) {
      assert.equal(icon.src, TINY_PNG_DATA_URL)
      assert.match(icon.src, /^data:image\/png/)
    }
  })
})

describe('PWA logo compositing geometry', () => {
  it('uses white as the default icon background for transparent logos', () => {
    assert.equal(PWA_ICON_BACKGROUND, '#ffffff')
  })

  it('clamps logo scale into a safe range', () => {
    assert.equal(clampPwaLogoScale(undefined), DEFAULT_PWA_LOGO_SCALE)
    assert.equal(clampPwaLogoScale('nope'), DEFAULT_PWA_LOGO_SCALE)
    assert.equal(clampPwaLogoScale(0), PWA_LOGO_SCALE_MIN)
    assert.equal(clampPwaLogoScale(2), PWA_LOGO_SCALE_MAX)
    assert.equal(clampPwaLogoScale(0.5), 0.5)
  })

  it('contain-fits a wide logo centered with padding from logoScale', () => {
    const rect = computeContainedLogoRect(200, 100, 512, 0.8)
    assert.ok(Math.abs(rect.width - 512 * 0.8) < 0.001)
    assert.ok(Math.abs(rect.height - (512 * 0.8) / 2) < 0.001)
    assert.ok(Math.abs(rect.x - (512 - rect.width) / 2) < 0.001)
    assert.ok(Math.abs(rect.y - (512 - rect.height) / 2) < 0.001)
  })

  it('contain-fits a tall logo without overflowing the canvas', () => {
    const rect = computeContainedLogoRect(100, 200, 192, 1)
    assert.ok(rect.height <= 192)
    assert.ok(rect.width <= 192)
    assert.equal(rect.height, 192)
    assert.ok(Math.abs(rect.width - 96) < 0.001)
  })

  it('dual-path: same-origin raw; hotlink CDNs proxy; other hosts stay original', () => {
    const origin = 'https://kiseki.blog'
    assert.equal(
      resolvePwaIconSourceUrl('/favicon.webp', origin),
      'https://kiseki.blog/favicon.webp',
    )
    assert.equal(
      resolvePwaIconSourceUrl('data:image/png;base64,abc', origin),
      'data:image/png;base64,abc',
    )
    assert.equal(
      resolvePwaIconSourceUrl('https://cdn.example/logo.png', origin, ''),
      'https://cdn.example/logo.png',
    )
    const bilibili = 'https://i0.hdslb.com/bfs/face/x.jpg'
    assert.equal(
      resolvePwaIconSourceUrl(bilibili, origin, ''),
      `/api/proxy/image?url=${encodeURIComponent(bilibili)}`,
    )
    assert.equal(
      resolvePwaIconSourceUrl(bilibili, origin, 'https://api.example'),
      `https://api.example/api/proxy/image?url=${encodeURIComponent(bilibili)}`,
    )
  })

  it('canvas readback only for same-origin, data, or proxied URLs', () => {
    const origin = 'https://kiseki.blog'
    assert.equal(pwaIconIsCanvasReadable('/favicon.webp', origin), true)
    assert.equal(
      pwaIconIsCanvasReadable('https://kiseki.blog/siteicon.ico', origin),
      true,
    )
    assert.equal(pwaIconIsCanvasReadable(TINY_PNG_DATA_URL, origin), true)
    assert.equal(
      pwaIconIsCanvasReadable(
        '/api/proxy/image?url=https%3A%2F%2Fi0.hdslb.com%2Fx.jpg',
        origin,
      ),
      true,
    )
    assert.equal(
      pwaIconIsCanvasReadable(
        'https://api.fuukei.org/myriad/frontend/public/siteicon.ico',
        origin,
      ),
      false,
    )
    assert.equal(
      pwaIconIsCanvasReadable('https://cdn.example/logo.png', origin),
      false,
    )
  })
})
