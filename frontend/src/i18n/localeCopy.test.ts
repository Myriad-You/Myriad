import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import { loadLocale } from './loadLocale.ts'
import { copyForLocale, currentCopy, formatCurrent } from './localeCopy.ts'

describe('currentCopy', () => {
  it('does not statically import ja or zh locale modules', () => {
    const src = readFileSync(new URL('./localeCopy.ts', import.meta.url), 'utf8')
    assert.equal(src.includes('ja-JP.json'), false)
    assert.equal(src.includes('zh-CN.json'), false)
    assert.equal(src.includes('ko-KR.json'), false)
    assert.equal(src.includes('fr-FR.json'), false)
    assert.equal(src.includes('de-DE.json'), false)
  })

  it('does not use JSON import attributes on dynamic locale packs', () => {
    // Vite serves `*.json?import` as JS; a JSON import attribute then fails in the browser.
    for (const rel of [
      './loadLocale.ts',
      './notificationCatalog.ts',
      '../components/settings/guides/catalog.ts',
      '../components/settings/guides/tappPermissionGuides.ts',
    ]) {
      const src = readFileSync(new URL(rel, import.meta.url), 'utf8')
      assert.equal(src.includes('with: { type:'), false, rel)
    }
  })

  it('formats ICU against the returned copy locale', () => {
    const src = readFileSync(new URL('./localeCopy.ts', import.meta.url), 'utf8')
    assert.match(src, /function resolveServiceCopy/)
    assert.equal(src.includes('formatMessage(getDefaultLocale()'), false)
    assert.match(src, /formatMessage\(resolveServiceCopy\(\)\.locale/)
    for (const rel of [
      '../utils/userFacingError.ts',
      '../utils/notificationFacing.ts',
      '../utils/httpRateLimitToast.ts',
    ]) {
      const service = readFileSync(new URL(rel, import.meta.url), 'utf8')
      assert.equal(service.includes('getDefaultLocale'), false, rel)
      assert.match(service, /formatCurrent/)
    }
  })

  it('keeps namespace catalogs out of the core locale files', () => {
    for (const locale of [
      'en-US',
      'zh-CN',
      'zh-TW',
      'ja-JP',
      'ko-KR',
      'fr-FR',
      'de-DE',
    ] as const) {
      const core = JSON.parse(
        readFileSync(new URL(`./${locale}.json`, import.meta.url), 'utf8'),
      ) as Record<string, unknown>
      for (const ns of ['config', 'tapp', 'brew', 'merope', 'errors', 'agentCaps']) {
        assert.equal(
          Object.hasOwn(core, ns),
          false,
          `${locale}.json must not contain top-level "${ns}"`,
        )
      }
    }
  })

  it('returns localized wallpaper and store errors', () => {
    const copy = currentCopy()
    assert.equal(typeof copy.wallpaperStatus.unsafeUrl, 'string')
    assert.ok(copy.wallpaperStatus.unsafeUrl.length > 0)
    assert.equal(typeof copy.tapp.storeAdminRequired, 'string')
    assert.equal(typeof copy.brew.loadSourcesFailed, 'string')
    assert.equal(typeof copy.errors.setupCheckFailed, 'string')
    assert.ok(copy.tapp.storeAppNotFound.includes('{id}'))
    assert.ok(copy.merope.anime25dPartCount.includes('{max}'))
    assert.ok(copy.errors.rateLimitedRetry.includes('{sec}'))
    assert.ok(copy.brew.webSearch.length > 0)
    assert.ok(copy.errors.serverError.includes('{status}'))
    assert.ok(copy.errors.lyricsFailed.includes('{status}'))
    assert.ok(copy.tapp.storeDownloadFailed.includes('{name}'))
    assert.equal(
      formatCurrent(copy.tapp.storeAppNotFound, { id: 'hello' }),
      copy.tapp.storeAppNotFound.replace('{id}', 'hello'),
    )
  })

  it('falls back to English until a non-default locale chunk is loaded', async () => {
    const before = copyForLocale('ja-JP')
    assert.equal(before.common.loading, currentCopy().common.loading)
    const ja = await loadLocale('ja-JP')
    assert.equal(copyForLocale('ja-JP'), ja)
    assert.equal(copyForLocale('ja-JP').common.loading, '読み込み中...')
    assert.equal(typeof ja.config.poweredBy, 'string')
    assert.equal(typeof ja.errors.configurationMode, 'string')
    assert.equal(ja.agentCaps['platform.read'], 'プラットフォームデータ読み取り')
    assert.equal(typeof ja.brew['voiceDesc.502006'], 'string')
    assert.equal(
      ja.agentCaps['platform.read.desc'],
      'キャッシュ済みプラットフォームデータを読みます。',
    )
    assert.equal(currentCopy().common.loading, 'Loading...')
  })

  it('loads Korean, French, and German as their own catalogs', async () => {
    const ko = await loadLocale('ko-KR')
    const fr = await loadLocale('fr-FR')
    const de = await loadLocale('de-DE')
    assert.equal(copyForLocale('ko-KR'), ko)
    assert.match(ko.common.loading, /불러/)
    assert.match(fr.common.loading, /Chargement/)
    assert.match(de.common.loading, /Laden/)
    assert.notEqual(ko.common.save, 'Save')
    assert.notEqual(fr.common.save, 'Save')
    assert.notEqual(de.common.save, 'Save')
  })

  it('loads Traditional Chinese as its own catalog', async () => {
    const zh = await loadLocale('zh-CN')
    const tw = await loadLocale('zh-TW')
    assert.equal(copyForLocale('zh-TW'), tw)
    assert.match(tw.controlPanel.languageZhTw, /繁體/)
    assert.notEqual(tw.config.title, zh.config.title)
    assert.match(tw.config.title, /設定|系統/)
    assert.match(zh.config.title, /设置|系统/)
  })
})
