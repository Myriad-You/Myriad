import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { pickLocaleEntry, resolveManifestText } from './manifestLocale.ts'

const source = {
  name: '我的应用',
  description: '中文描述',
  locales: {
    'en-US': { name: 'My App', description: 'English description' },
    'ja-JP': { description: '日本語の説明' },
  },
}

describe('resolveManifestText', () => {
  it('returns exact locale overrides', () => {
    assert.deepEqual(resolveManifestText(source, 'en-US'), {
      name: 'My App',
      description: 'English description',
    })
  })

  it('falls back per-field to top-level text', () => {
    assert.deepEqual(resolveManifestText(source, 'ja-JP'), {
      name: '我的应用',
      description: '日本語の説明',
    })
  })

  it('falls back entirely when locale is missing', () => {
    assert.deepEqual(resolveManifestText(source, 'zh-CN'), {
      name: '我的应用',
      description: '中文描述',
    })
  })

  it('matches case-insensitively and by language prefix', () => {
    assert.equal(resolveManifestText(source, 'en-us').name, 'My App')
    assert.equal(resolveManifestText(source, 'en').name, 'My App')
    assert.equal(resolveManifestText(source, 'en-GB').name, 'My App')
  })

  it('ignores empty override strings', () => {
    const blank = {
      name: 'Base',
      locales: { 'en-US': { name: '   ' } },
    }
    assert.equal(resolveManifestText(blank, 'en-US').name, 'Base')
  })

  it('handles missing locales and undefined locale', () => {
    assert.deepEqual(resolveManifestText({ name: 'Base' }, undefined), {
      name: 'Base',
      description: undefined,
    })
  })

  it('does not let zh-TW prefix-match zh-CN', () => {
    const locales = {
      'zh-CN': { name: '简体' },
      'en-US': { name: 'English' },
    }
    assert.equal(pickLocaleEntry(locales, 'zh-TW'), undefined)
    assert.equal(pickLocaleEntry(locales, 'zh-HK')?.name, undefined)
    assert.equal(pickLocaleEntry(locales, 'zh-CN')?.name, '简体')
  })

  it('matches Traditional Chinese variants to zh-TW', () => {
    const locales = {
      'zh-TW': { name: '繁體' },
      'zh-CN': { name: '简体' },
    }
    assert.equal(pickLocaleEntry(locales, 'zh-HK')?.name, '繁體')
    assert.equal(pickLocaleEntry(locales, 'zh-Hant')?.name, '繁體')
    assert.equal(pickLocaleEntry(locales, 'zh-Hans')?.name, '简体')
  })
})
