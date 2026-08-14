/**
 * cd frontend && node --experimental-strip-types --test src/tapp/utils/storeLocale.test.ts
 */

import type { StorePreviewDescriptor } from './storePreview.ts'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  parseStoreLocales,
  resolveStoreMerchandising,
} from './storeLocale.ts'

function preview(html: string): StorePreviewDescriptor {
  return {
    version: 1,
    type: 'snapshot',
    html,
    styles: ['preview.css'],
    viewport: { width: 1280, height: 720 },
    fit: 'cover',
    focus: { x: 0.5, y: 0.5 },
    theme: 'light',
  }
}

const app = {
  name: '朝夕',
  description: '记录重要的日子',
  long_description: '用朝夕记录生日和纪念日。',
  preview: preview('preview.html'),
  locales: {
    'en-US': {
      name: 'Days',
      description: 'Keep meaningful dates close.',
      long_description: 'Use Days to remember birthdays and anniversaries.',
      preview: preview('preview.en-US.html'),
    },
    'ja-JP': {
      name: '日々',
      description: '大切な日を記録します。',
      long_description: '日々で誕生日や記念日を記録できます。',
      preview: preview('preview.ja-JP.html'),
    },
  },
}

describe('resolveStoreMerchandising', () => {
  it('selects exact long description and preview', () => {
    const en = resolveStoreMerchandising(app, 'en-US')
    assert.equal(en.name, 'Days')
    assert.equal(
      en.longDescription,
      'Use Days to remember birthdays and anniversaries.',
    )
    assert.equal(en.preview?.html, 'preview.en-US.html')

    const ja = resolveStoreMerchandising(app, 'ja-JP')
    assert.equal(ja.name, '日々')
    assert.equal(ja.preview?.html, 'preview.ja-JP.html')
  })

  it('matches language prefixes and falls back per field', () => {
    const ja = resolveStoreMerchandising(app, 'ja')
    assert.equal(ja.name, '日々')
    assert.equal(ja.preview?.html, 'preview.ja-JP.html')

    const zh = resolveStoreMerchandising(app, 'zh-CN')
    assert.equal(zh.name, '朝夕')
    assert.equal(zh.longDescription, '用朝夕记录生日和纪念日。')
    assert.equal(zh.preview?.html, 'preview.html')
  })

  it('falls back to default preview when a locale only overrides copy', () => {
    const localized = resolveStoreMerchandising(
      {
        ...app,
        locales: {
          'en-US': {
            name: 'Days',
            long_description: 'English only copy.',
          },
        },
      },
      'en-US',
    )
    assert.equal(localized.longDescription, 'English only copy.')
    assert.equal(localized.preview?.html, 'preview.html')
  })

  it('falls back to localized short description when long copy is absent', () => {
    const localized = resolveStoreMerchandising(
      {
        name: '朝夕',
        description: '中文短描述',
        locales: {
          'en-US': { name: 'Days', description: 'English short copy.' },
        },
      },
      'en-US',
    )
    assert.equal(localized.longDescription, 'English short copy.')
  })

  it('keeps legacy catalogs without locales unchanged', () => {
    const localized = resolveStoreMerchandising(
      {
        name: 'Notes',
        description: 'Short',
        long_description: 'Long',
        preview: preview('preview.html'),
      },
      'en-US',
    )
    assert.deepEqual(localized, {
      name: 'Notes',
      description: 'Short',
      longDescription: 'Long',
      preview: preview('preview.html'),
    })
  })
})

describe('parseStoreLocales', () => {
  it('keeps name overlays and parses localized preview snapshots', () => {
    const locales = parseStoreLocales({
      'en-US': {
        name: 'Days',
        preview: {
          version: 1,
          type: 'snapshot',
          html: 'preview.en-US.html',
          styles: ['preview.css'],
        },
      },
      junk: 'nope',
    })
    assert.equal(locales?.['en-US']?.name, 'Days')
    assert.equal(locales?.['en-US']?.preview?.html, 'preview.en-US.html')
    assert.equal(locales?.junk, undefined)
  })

  it('drops invalid localized preview declarations', () => {
    const locales = parseStoreLocales({
      'en-US': {
        long_description: 'Hello',
        preview: { html: '<div>inline</div>' },
      },
    })
    assert.equal(locales?.['en-US']?.long_description, 'Hello')
    assert.equal(locales?.['en-US']?.preview, undefined)
  })
})
