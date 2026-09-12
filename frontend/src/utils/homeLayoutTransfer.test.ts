import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import { createHomeStickerItem } from './homeLayout'
import {
  buildHomeLayoutExport,
  canonicalStickerImageUrl,
  encodeBase64,
  HOME_LAYOUT_ASSET_MAX_BYTES,
  HOME_LAYOUT_ASSETS_MAX_TOTAL_BYTES,
  HOME_LAYOUT_EXPORT_KIND,
  HOME_LAYOUT_EXPORT_VERSION,
  HOME_LAYOUT_IMPORT_MAX_BYTES,
  HOME_LAYOUT_IMPORT_MAX_TILES,
  homeLayoutExportFilename,
  listStickerImageUrls,
  parseHomeLayoutImport,
  parseHomeLayoutImportText,
  rewriteStickerImageUrls,
  stickerAssetFitsBudget,
} from './homeLayoutTransfer'

const welcome = {
  id: 'w1',
  type: 'welcome',
  size: '4x2' as const,
  position: { x: 0, y: 0 },
  extra: 'drop-me',
}

const weather = {
  id: 'w2',
  type: 'weather',
  size: '2x2' as const,
  position: { x: 4, y: 0 },
  config: { city: 'Tokyo' },
}

const sticker = createHomeStickerItem({
  size: '2x2',
  position: { x: 8, y: 0 },
  imageUrl: '/media/sticker.png',
  prompt: 'cat',
})

describe('home layout export envelope', () => {
  it('writes kind, version, mode, and sanitized tiles', () => {
    const doc = buildHomeLayoutExport(
      {
        standard: [welcome, sticker],
        free: [weather, sticker],
      },
      'free',
    )
    assert.equal(doc.kind, HOME_LAYOUT_EXPORT_KIND)
    assert.equal(doc.v, HOME_LAYOUT_EXPORT_VERSION)
    assert.equal(doc.mode, 'free')
    assert.deepEqual(doc.assets, {})
    assert.deepEqual(
      doc.layouts.standard.map((tile) => tile.type),
      ['welcome'],
    )
    assert.equal(Object.hasOwn(doc.layouts.standard[0]!, 'extra'), false)
    assert.equal(doc.layouts.free.length, 2)
    assert.equal(doc.layouts.free[1]?.kind, 'sticker')
    assert.equal(doc.layouts.free[1]?.config?.imageUrl, '/media/sticker.png')
  })

  it('names the file with an ISO date', () => {
    assert.equal(
      homeLayoutExportFilename(new Date('2026-09-06T12:00:00.000Z')),
      'myriad-home-layout-2026-09-06.json',
    )
  })
})

describe('home layout import', () => {
  it('round-trips its own export', () => {
    const doc = buildHomeLayoutExport(
      { standard: [welcome], free: [weather, sticker] },
      'free',
    )
    const parsed = parseHomeLayoutImport(doc)
    assert.equal(parsed.ok, true)
    if (!parsed.ok) return
    assert.equal(parsed.mode, 'free')
    assert.deepEqual(parsed.assets, {})
    assert.equal(parsed.layouts.standard[0]?.type, 'welcome')
    assert.equal(parsed.layouts.free[1]?.kind, 'sticker')
  })

  it('accepts stored v2 dashboard_layout and a legacy array', () => {
    const stored = parseHomeLayoutImport({
      v: 2,
      standard: [welcome],
      free: [weather],
    })
    assert.equal(stored.ok, true)
    if (stored.ok) {
      assert.equal(stored.mode, null)
      assert.equal(stored.layouts.free[0]?.config?.city, 'Tokyo')
    }

    const legacy = parseHomeLayoutImport([welcome, weather])
    assert.equal(legacy.ok, true)
    if (legacy.ok) {
      assert.equal(legacy.layouts.standard.length, 2)
      assert.equal(legacy.layouts.free.length, 2)
    }
  })

  it('accepts a dashboard_layout extract and rejects settings backups', () => {
    const extract = parseHomeLayoutImport({
      dashboard_layout: JSON.stringify({ v: 2, standard: [welcome], free: [] }),
      dashboard_layout_mode: 'free',
    })
    assert.equal(extract.ok, true)
    if (extract.ok) assert.equal(extract.mode, 'free')

    const backup = parseHomeLayoutImport({
      format: 'myriad-settings-backup',
      version: 2,
      configurations: [],
      effective_config: {},
      user_preferences: {},
      dashboard_layout: { v: 2, standard: [welcome], free: [] },
    })
    assert.deepEqual(backup, { ok: false, reason: 'settings-backup' })

    const legacyDump = parseHomeLayoutImport({
      platforms: {},
      ai_config: {},
      ui_config: {},
      standard: [welcome],
      free: [],
    })
    assert.deepEqual(legacyDump, { ok: false, reason: 'settings-backup' })
  })

  it('clamps tiles, drops junk, and uniquifies ids', () => {
    const parsed = parseHomeLayoutImport({
      standard: [
        { id: 'a', type: 'welcome', size: '4x2', position: { x: 20, y: 9 } },
        {
          id: 'a',
          type: 'weather',
          size: '2x2',
          position: { x: 0, y: 0 },
        },
        { id: 'bad', type: 'welcome', size: '9x9', position: { x: 0, y: 0 } },
        sticker,
      ],
      free: [
        {
          id: 's',
          type: 'sticker',
          size: '4x3',
          position: { x: 0, y: 10 },
          kind: 'sticker',
          config: { imageUrl: '/s.png' },
        },
      ],
    })
    assert.equal(parsed.ok, true)
    if (!parsed.ok) return
    assert.deepEqual(parsed.layouts.standard[0]?.position, { x: 12, y: 2 })
    assert.equal(parsed.layouts.standard[1]?.id, 'a__2')
    assert.equal(
      parsed.layouts.standard.some((tile) => tile.size === '9x9'),
      false,
    )
    assert.equal(
      parsed.layouts.standard.some((tile) => tile.kind === 'sticker'),
      false,
    )
    assert.equal(parsed.layouts.free[0]?.position.y, 5)
  })

  it('rejects garbage, empty-invalid files, oversized text, and oversized lists', () => {
    assert.deepEqual(parseHomeLayoutImport(null), {
      ok: false,
      reason: 'invalid',
    })
    assert.deepEqual(parseHomeLayoutImport({ kind: 'other', layouts: {} }), {
      ok: false,
      reason: 'invalid',
    })
    assert.deepEqual(
      parseHomeLayoutImport({
        standard: [{ nope: true }],
        free: [{ nope: true }],
      }),
      { ok: false, reason: 'invalid' },
    )
    assert.deepEqual(parseHomeLayoutImportText('not-json'), {
      ok: false,
      reason: 'invalid',
    })
    assert.deepEqual(
      parseHomeLayoutImportText('[]', HOME_LAYOUT_IMPORT_MAX_BYTES + 1),
      { ok: false, reason: 'too-large' },
    )

    const tooMany = Array.from({ length: HOME_LAYOUT_IMPORT_MAX_TILES + 1 }, (_, i) => ({
      id: `w${i}`,
      type: 'welcome',
      size: '2x2',
      position: { x: 0, y: 0 },
    }))
    assert.deepEqual(parseHomeLayoutImport({ standard: tooMany, free: [] }), {
      ok: false,
      reason: 'too-many',
    })
  })

  it('accepts an explicit empty layout and a BOM-prefixed export', () => {
    const empty = parseHomeLayoutImport({ standard: [], free: [] })
    assert.equal(empty.ok, true)
    if (empty.ok) {
      assert.deepEqual(empty.layouts, { standard: [], free: [] })
    }
    const doc = buildHomeLayoutExport({ standard: [welcome], free: [] }, 'standard')
    const withBom = parseHomeLayoutImportText(`\uFEFF${JSON.stringify(doc)}`)
    assert.equal(withBom.ok, true)
  })
})

const PNG_1X1 = Uint8Array.from([
  0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
  0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
  0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00,
  0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
  0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
  0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
])
const STICKER_HASH = `ab${'a'.repeat(62)}`
const STICKER_PATH = `/api/brew/image-cache/ab/${STICKER_HASH}.png`
const PNG_DATA = encodeBase64(PNG_1X1)

describe('home layout sticker originals', () => {
  it('canonicalizes image-cache sticker URLs and lists them once', () => {
    const tile = createHomeStickerItem({
      size: '2x2',
      position: { x: 0, y: 0 },
      imageUrl: `https://example.com${STICKER_PATH}?x=1`,
      prompt: 'cat',
    })
    const doc = buildHomeLayoutExport({ standard: [], free: [tile] }, 'free')
    assert.equal(doc.layouts.free[0]?.config?.imageUrl, STICKER_PATH)
    assert.deepEqual(listStickerImageUrls(doc.layouts), [STICKER_PATH])
    assert.equal(
      canonicalStickerImageUrl(`https://host${STICKER_PATH}#frag`),
      STICKER_PATH,
    )
    const webpPath = `/api/brew/image-cache/ab/${STICKER_HASH}.webp`
    assert.equal(canonicalStickerImageUrl(webpPath), webpPath)
  })

  it('keeps matching PNG assets and drops data URLs from saved tiles', () => {
    const tile = createHomeStickerItem({
      size: '2x2',
      position: { x: 0, y: 0 },
      imageUrl: STICKER_PATH,
      prompt: 'cat',
    })
    const doc = buildHomeLayoutExport({ standard: [], free: [tile] }, 'free', {
      [STICKER_PATH]: { mime: 'image/png', data: PNG_DATA },
      '/api/brew/image-cache/ff/not-used.png': {
        mime: 'image/png',
        data: PNG_DATA,
      },
    })
    assert.deepEqual(Object.keys(doc.assets), [STICKER_PATH])
    assert.equal(doc.assets[STICKER_PATH]?.mime, 'image/png')

    const inline = parseHomeLayoutImport({
      kind: HOME_LAYOUT_EXPORT_KIND,
      v: 2,
      mode: 'free',
      layouts: {
        standard: [],
        free: [
          createHomeStickerItem({
            size: '2x2',
            position: { x: 0, y: 0 },
            imageUrl: `data:image/png;base64,${PNG_DATA}`,
            prompt: 'cat',
          }),
        ],
      },
    })
    assert.equal(inline.ok, true)
    if (!inline.ok) return
    const imageUrl = inline.layouts.free[0]?.config?.imageUrl
    assert.equal(typeof imageUrl, 'string')
    assert.equal(String(imageUrl).startsWith('data:'), false)
    assert.equal(String(imageUrl).startsWith('inline:'), true)
    assert.equal(inline.assets[String(imageUrl)]?.mime, 'image/png')

    const jpeg = Uint8Array.from([0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10])
    const jpegData = encodeBase64(jpeg)
    const sameId = parseHomeLayoutImport({
      standard: [],
      free: [
        {
          id: 'dup',
          type: 'sticker',
          kind: 'sticker',
          size: '2x2',
          position: { x: 0, y: 0 },
          config: { imageUrl: `data:image/png;base64,${PNG_DATA}` },
        },
        {
          id: 'dup',
          type: 'sticker',
          kind: 'sticker',
          size: '2x2',
          position: { x: 2, y: 0 },
          config: { imageUrl: `data:image/jpeg;base64,${jpegData}` },
        },
      ],
    })
    assert.equal(sameId.ok, true)
    if (sameId.ok) {
      assert.equal(sameId.layouts.free[0]?.config?.imageUrl, 'inline:0')
      assert.equal(sameId.layouts.free[1]?.config?.imageUrl, 'inline:1')
      assert.equal(sameId.assets['inline:0']?.mime, 'image/png')
      assert.equal(sameId.assets['inline:1']?.mime, 'image/jpeg')
    }

    const webp = Uint8Array.from([
      0x52, 0x49, 0x46, 0x46, 0x08, 0x00, 0x00, 0x00, 0x57, 0x45, 0x42, 0x50,
    ])
    const webpData = encodeBase64(webp)
    const webpImport = parseHomeLayoutImport({
      standard: [],
      free: [
        {
          id: 'webp',
          type: 'sticker',
          kind: 'sticker',
          size: '2x2',
          position: { x: 0, y: 0 },
          config: { imageUrl: `data:image/webp;base64,${webpData}` },
        },
      ],
    })
    assert.equal(webpImport.ok, true)
    if (webpImport.ok) {
      assert.equal(webpImport.layouts.free[0]?.config?.imageUrl, 'inline:0')
      assert.equal(webpImport.assets['inline:0']?.mime, 'image/webp')
    }
  })

  it('caps bundled originals at the shared budget', () => {
    assert.equal(stickerAssetFitsBudget(0, 1), true)
    assert.equal(stickerAssetFitsBudget(HOME_LAYOUT_ASSETS_MAX_TOTAL_BYTES, 1), false)
    assert.equal(stickerAssetFitsBudget(0, HOME_LAYOUT_ASSET_MAX_BYTES + 1), false)
  })

  it('rewrites sticker URLs after restore and accepts a v1 file without assets', () => {
    const tile = createHomeStickerItem({
      size: '2x2',
      position: { x: 0, y: 0 },
      imageUrl: STICKER_PATH,
      prompt: 'cat',
    })
    const rewritten = rewriteStickerImageUrls(
      { standard: [], free: [tile] },
      (url) => (url === STICKER_PATH ? '/api/brew/image-cache/cd/new.png' : url),
    )
    assert.equal(
      rewritten.free[0]?.config?.imageUrl,
      '/api/brew/image-cache/cd/new.png',
    )

    const legacy = parseHomeLayoutImport({
      kind: HOME_LAYOUT_EXPORT_KIND,
      v: 1,
      mode: 'standard',
      layouts: { standard: [welcome], free: [] },
    })
    assert.equal(legacy.ok, true)
    if (legacy.ok) assert.deepEqual(legacy.assets, {})
  })
})

describe('home layout transfer wiring', () => {
  it('keeps Home on the transfer buttons', () => {
    const home = readFileSync(new URL('../views/Home.tsx', import.meta.url), 'utf8')
    const transfer = readFileSync(
      new URL('../components/home/HomeLayoutTransfer.tsx', import.meta.url),
      'utf8',
    )
    assert.equal(home.includes('HomeLayoutTransferButtons'), true)
    assert.equal(home.includes('showLabel={false}'), false)
    assert.equal(home.includes('restoreStickerAssets'), true)
    assert.equal(home.includes('layoutImportInFlightRef'), true)
    assert.equal(transfer.includes('t.home.importLayout'), true)
    assert.equal(transfer.includes('t.home.exportLayout'), true)
    assert.equal(transfer.includes('parseHomeLayoutImportText'), true)
    assert.equal(transfer.includes('buildHomeLayoutExport'), true)
    assert.equal(transfer.includes('fetchStickerAssets'), true)
  })
})
