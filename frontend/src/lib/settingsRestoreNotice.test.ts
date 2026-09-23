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
  clearRestoreNotice,
  EMPTY_RESTORE_NOTICE,
  formatRestoreNotice,
  hasRestoreNotice,
  parseSettingKeys,
  parseUnresolvedRestoredMedia,
  readRestoreNotice,
  RESTORE_NOTICE_KEY,
  stashRestoreNotice,
  summarizeKeys,
} from './settingsRestoreNotice'

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
  mediaTitle: enUS.restoreUnresolvedMediaTitle,
  skippedTitle: enUS.restoreSkippedSettingsTitle,
  more: enUS.restoreUnresolvedMediaMore,
  wallpaper: enUS.restoreUnresolvedMediaWallpaper,
  sticker: enUS.restoreUnresolvedMediaSticker,
}
function format(template: string, params: Record<string, string | number>) {
  return formatMessage('en-US', template, params)
}

describe('settings restore notice', () => {
  it('parses the server fields and drops malformed entries', () => {
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
    assert.deepEqual(parseSettingKeys(undefined), [])
    assert.deepEqual(
      parseSettingKeys(['ui_wallpaper_url', '', 3, null, 'proxy_url']),
      ['ui_wallpaper_url', 'proxy_url'],
    )
  })

  it('labels media, lists skipped settings, pluralizes and truncates', () => {
    const unresolvedMedia = [
      { setting: 'ui_wallpaper_url', url: '/media/federation/1/w.png' },
      ...Array.from({ length: 6 }, (_, i) => ({
        setting: 'dashboard_layout',
        url: `/api/media/${i + 1}/content`,
      })),
    ]
    const notice = formatRestoreNotice(
      { unresolvedMedia, skippedSettings: ['umami_script_url'] },
      enCopy,
      format,
    )
    assert.match(notice.media!.title, /^7 media items /)
    assert.equal(notice.media!.lines.length, 5)
    assert.equal(notice.media!.lines[0], 'Wallpaper: /media/federation/1/w.png')
    assert.equal(notice.media!.lines[1], 'Dashboard sticker: /api/media/1/content')
    assert.equal(notice.media!.more, '…and 2 more')
    assert.match(notice.skipped!.title, /^1 setting .* was skipped; the current value was kept$/)
    assert.deepEqual(notice.skipped!.lines, ['umami_script_url'])
    assert.equal(notice.skipped!.more, null)

    const onlySkipped = formatRestoreNotice(
      { unresolvedMedia: [], skippedSettings: ['a', 'b'] },
      enCopy,
      format,
    )
    assert.equal(onlySkipped.media, null)
    assert.match(onlySkipped.skipped!.title, /^2 settings /)
    assert.deepEqual(summarizeKeys(['a', 'b', 'c'], 2), {
      shown: ['a', 'b'],
      hidden: 1,
    })
  })

  it('carries the notice across the reload until cleared', () => {
    const storage = memoryStorage()
    const notice = {
      unresolvedMedia: [{ setting: 'dashboard_layout', url: '/api/media/9/content' }],
      skippedSettings: ['ui_wallpaper_url'],
    }
    assert.equal(hasRestoreNotice(notice), true)
    assert.equal(stashRestoreNotice(notice, storage), true)
    assert.deepEqual(readRestoreNotice(storage), notice)
    assert.deepEqual(readRestoreNotice(storage), notice, 'read keeps it')
    // A later clean restore clears a stale notice.
    assert.equal(stashRestoreNotice(EMPTY_RESTORE_NOTICE, storage), true)
    assert.deepEqual(readRestoreNotice(storage), EMPTY_RESTORE_NOTICE)
    stashRestoreNotice(notice, storage)
    clearRestoreNotice(storage)
    assert.equal(storage.map.has(RESTORE_NOTICE_KEY), false)
    storage.map.set(RESTORE_NOTICE_KEY, '{not json')
    assert.deepEqual(readRestoreNotice(storage), EMPTY_RESTORE_NOTICE)
    storage.map.set(RESTORE_NOTICE_KEY, 'null')
    assert.deepEqual(readRestoreNotice(storage), EMPTY_RESTORE_NOTICE)
  })

  it('survives missing or failing storage', () => {
    assert.equal(stashRestoreNotice(EMPTY_RESTORE_NOTICE, null), false)
    assert.deepEqual(readRestoreNotice(null), EMPTY_RESTORE_NOTICE)
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
      stashRestoreNotice({ unresolvedMedia: [], skippedSettings: ['x'] }, failing),
      false,
    )
    assert.deepEqual(readRestoreNotice(failing), EMPTY_RESTORE_NOTICE)
    clearRestoreNotice(failing)
  })

  it('has copy in every host language', () => {
    const keys = [
      'importConfigSuccessNeedsAttention',
      'restoreUnresolvedMediaTitle',
      'restoreUnresolvedMediaHint',
      'restoreSkippedSettingsTitle',
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
        for (const count of [1, 3]) {
          const rendered = formatMessage(locale, text, { count })
          assert.doesNotMatch(rendered, /[{}]/, `${locale}.${key}: ${rendered}`)
        }
      }
    }
  })
})
