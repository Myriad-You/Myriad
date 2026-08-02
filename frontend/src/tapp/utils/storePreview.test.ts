/**
 * cd frontend && node --experimental-strip-types --test src/tapp/utils/storePreview.test.ts
 */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  parseStorePreview,
  STORE_PREVIEW_MAX_HEIGHT,
  STORE_PREVIEW_MAX_WIDTH,
  STORE_PREVIEW_MIN_HEIGHT,
  STORE_PREVIEW_MIN_WIDTH,
} from './storePreview.ts'

describe('parseStorePreview', () => {
  it('parses a complete static snapshot declaration', () => {
    assert.deepEqual(
      parseStorePreview({
        version: 1,
        type: 'snapshot',
        html: 'apps/com.example/preview.html',
        styles: ['apps/com.example/page.css', 'apps/com.example/page.css'],
        viewport: { width: 1440, height: 900 },
        fit: 'contain',
        focus: { x: 0.4, y: 0.25 },
        theme: 'dark',
      }),
      {
        version: 1,
        type: 'snapshot',
        html: 'apps/com.example/preview.html',
        styles: ['apps/com.example/page.css'],
        viewport: { width: 1440, height: 900 },
        fit: 'contain',
        focus: { x: 0.4, y: 0.25 },
        theme: 'dark',
      },
    )
  })

  it('ignores invalid declarations and bounds rendering parameters', () => {
    assert.equal(parseStorePreview(null), undefined)
    assert.equal(parseStorePreview({ version: 2, type: 'snapshot' }), undefined)
    assert.equal(
      parseStorePreview({ version: 1, type: 'snapshot', html: '  ' }),
      undefined,
    )
    // Markup must never be accepted as a resource path (blank-preview regression).
    assert.equal(
      parseStorePreview({
        version: 1,
        type: 'snapshot',
        html: '<main>inline markup is not a path</main>',
      }),
      undefined,
    )
    assert.deepEqual(
      parseStorePreview({
        version: 1,
        type: 'snapshot',
        html: 'apps/ok/preview.html',
        styles: ['apps/ok/a.css', 'data:text/css,body{}', 'bad path.css'],
      })?.styles,
      ['apps/ok/a.css'],
    )

    const low = parseStorePreview({
      version: 1,
      type: 'snapshot',
      html: 'preview.html',
      viewport: { width: 320, height: 200 },
      focus: { x: -2, y: 3 },
    })
    assert.deepEqual(low?.viewport, {
      width: STORE_PREVIEW_MIN_WIDTH,
      height: STORE_PREVIEW_MIN_HEIGHT,
    })
    assert.deepEqual(low?.focus, { x: 0, y: 1 })

    const high = parseStorePreview({
      version: 1,
      type: 'snapshot',
      html: 'preview.html',
      viewport: { width: 99999, height: 99999 },
    })
    assert.deepEqual(high?.viewport, {
      width: STORE_PREVIEW_MAX_WIDTH,
      height: STORE_PREVIEW_MAX_HEIGHT,
    })
  })
})
