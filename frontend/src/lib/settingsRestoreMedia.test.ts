import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import deDE from '../i18n/config.de-DE.json' with { type: 'json' }
import enUS from '../i18n/config.en-US.json' with { type: 'json' }
import frFR from '../i18n/config.fr-FR.json' with { type: 'json' }
import jaJP from '../i18n/config.ja-JP.json' with { type: 'json' }
import koKR from '../i18n/config.ko-KR.json' with { type: 'json' }
import zhCN from '../i18n/config.zh-CN.json' with { type: 'json' }
import zhTW from '../i18n/config.zh-TW.json' with { type: 'json' }
import { formatMessage } from '../i18n/formatMessage'
import {
  clearRestoreMediaNotice,
  formatUnresolvedMediaNotice,
  parseUnresolvedRestoredMedia,
  readRestoreMediaNotice,
  RESTORE_MEDIA_NOTICE_KEY,
  stashRestoreMediaNotice,
} from './settingsRestoreMedia'

function memoryStorage() {
  const map = new Map<string, string>()
  return {
    map,
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => void map.set(key, value),
    removeItem: (key: string) => void map.delete(key),
  }
}

const enCopy = {
  title: enUS.restoreUnresolvedMediaTitle,
  more: enUS.restoreUnresolvedMediaMore,
  wallpaper: enUS.restoreUnresolvedMediaWallpaper,
  sticker: enUS.restoreUnresolvedMediaSticker,
}
function format(template: string, params: Record<string, string | number>) {
  return formatMessage('en-US', template, params)
}

describe('settings restore unresolved media', () => {
  it('parses the server field and drops malformed entries', () => {
    assert.deepEqual(parseUnresolvedRestoredMedia(undefined), [])
    assert.deepEqual(parseUnresolvedRestoredMedia({}), [])
    assert.deepEqual(
      parseUnresolvedRestoredMedia([
        { setting: 'ui_wallpaper_url', url: '/media/assets/a/w.png' },
        { setting: 'dashboard_layout' },
        { setting: 'dashboard_layout', url: '' },
        null,
        'x',
      ]),
      [{ setting: 'ui_wallpaper_url', url: '/media/assets/a/w.png' }],
    )
  })

  it('labels settings, pluralizes and truncates long lists', () => {
    const items = [
      { setting: 'ui_wallpaper_url', url: '/media/federation/1/w.png' },
      ...Array.from({ length: 6 }, (_, i) => ({
        setting: 'dashboard_layout',
        url: `/api/media/${i + 1}/content`,
      })),
    ]
    const notice = formatUnresolvedMediaNotice(items, enCopy, format)
    assert.match(notice.title, /^7 media items /)
    assert.equal(notice.lines.length, 5)
    assert.equal(notice.lines[0], 'Wallpaper: /media/federation/1/w.png')
    assert.equal(notice.lines[1], 'Dashboard sticker: /api/media/1/content')
    assert.equal(notice.more, '…and 2 more')

    const single = formatUnresolvedMediaNotice(items.slice(0, 1), enCopy, format)
    assert.match(single.title, /^1 media item .* was not bound$/)
    assert.equal(single.more, null)
  })

  it('carries the notice across the reload until cleared', () => {
    const storage = memoryStorage()
    const items = [{ setting: 'dashboard_layout', url: '/api/media/9/content' }]
    assert.equal(stashRestoreMediaNotice(items, storage), true)
    assert.deepEqual(readRestoreMediaNotice(storage), items)
    assert.deepEqual(readRestoreMediaNotice(storage), items, 'read keeps it')
    // A later clean restore clears a stale notice.
    assert.equal(stashRestoreMediaNotice([], storage), true)
    assert.deepEqual(readRestoreMediaNotice(storage), [])
    stashRestoreMediaNotice(items, storage)
    clearRestoreMediaNotice(storage)
    assert.equal(storage.map.has(RESTORE_MEDIA_NOTICE_KEY), false)
    storage.map.set(RESTORE_MEDIA_NOTICE_KEY, '{not json')
    assert.deepEqual(readRestoreMediaNotice(storage), [])
  })

  it('survives missing or failing storage', () => {
    assert.equal(stashRestoreMediaNotice([], null), false)
    assert.deepEqual(readRestoreMediaNotice(null), [])
    const failing = {
      getItem: () => {
        throw new Error('blocked')
      },
      setItem: () => {
        throw new Error('blocked')
      },
      removeItem: () => {
        throw new Error('blocked')
      },
    }
    assert.equal(
      stashRestoreMediaNotice([{ setting: 'x', url: '/a' }], failing),
      false,
    )
    assert.deepEqual(readRestoreMediaNotice(failing), [])
    clearRestoreMediaNotice(failing)
  })

  it('has copy in every host language', () => {
    const keys = [
      'importConfigSuccessUnresolvedMedia',
      'restoreUnresolvedMediaTitle',
      'restoreUnresolvedMediaHint',
      'restoreUnresolvedMediaMore',
      'restoreUnresolvedMediaWallpaper',
      'restoreUnresolvedMediaSticker',
      'restoreUnresolvedMediaDismiss',
    ] as const
    const locales = [
      ['en-US', enUS],
      ['zh-CN', zhCN],
      ['zh-TW', zhTW],
      ['ja-JP', jaJP],
      ['ko-KR', koKR],
      ['de-DE', deDE],
      ['fr-FR', frFR],
    ] as const
    for (const [locale, copy] of locales) {
      for (const key of keys) {
        const text = copy[key]
        assert.ok(text.length > 0, `${locale}.${key}`)
        const rendered = formatMessage(locale, text, { count: 3 })
        assert.doesNotMatch(rendered, /[{}]/, `${locale}.${key}: ${rendered}`)
      }
    }
  })
})
