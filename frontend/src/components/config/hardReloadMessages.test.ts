/** hard vs hot reload copy must stay distinct */
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import enUS from '../../i18n/config.en-US.json' with { type: 'json' }
import jaJP from '../../i18n/config.ja-JP.json' with { type: 'json' }
import zhCN from '../../i18n/config.zh-CN.json' with { type: 'json' }

const locales = [
  { name: 'zh-CN', c: zhCN },
  { name: 'en-US', c: enUS },
  { name: 'ja-JP', c: jaJP },
] as const

describe('hard / runtime reload save messages', () => {
  it('hard-reload messages mention full page reload, not only “saved”', () => {
    for (const { name, c } of locales) {
      assert.ok(
        c.savedSuccessHardReload.length > 0,
        `${name}.savedSuccessHardReload`,
      )
      assert.ok(
        c.hardReloadPreparing.length > 0,
        `${name}.hardReloadPreparing`,
      )
      // must not collapse to the generic soft-save string
      assert.notEqual(c.savedSuccessHardReload, c.savedSuccess)
      assert.notEqual(c.hardReloadPreparing, c.savedSuccess)
      assert.notEqual(c.hardReloadPreparing, c.savedSuccessHardReload)
    }
  })

  it('runtime-reload message is distinct from soft and hard success', () => {
    for (const { name, c } of locales) {
      assert.ok(
        c.savedSuccessRuntimeReload.length > 0,
        `${name}.savedSuccessRuntimeReload`,
      )
      assert.notEqual(c.savedSuccessRuntimeReload, c.savedSuccess)
      assert.notEqual(c.savedSuccessRuntimeReload, c.savedSuccessHardReload)
    }
  })

  it('import / force-cache success copy still signals upcoming full reload', () => {
    assert.match(zhCN.importConfigSuccess, /整页刷新|刷新/)
    assert.match(zhCN.forceRefreshFrontendCacheSuccess, /整页刷新|刷新/)
    assert.match(enUS.importConfigSuccess, /reload|refresh/i)
    assert.match(enUS.forceRefreshFrontendCacheSuccess, /reload|refresh/i)
    assert.match(jaJP.importConfigSuccess, /再読み込み|更新/)
    assert.match(jaJP.forceRefreshFrontendCacheSuccess, /再読み込み|更新/)
    assert.match(zhCN.importConfirmMessage, /整页刷新/)
    assert.match(enUS.importConfirmMessage, /reload/i)
  })
})
