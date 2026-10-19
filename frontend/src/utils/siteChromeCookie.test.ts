import assert from 'node:assert/strict'
import { test } from 'node:test'
import { siteChromeValue, toHex6 } from './siteChromeCookie.ts'

test('颜色只认十六进制和 rgb，统一成六位小写', () => {
  assert.equal(toHex6(' #ABC '), 'aabbcc')
  assert.equal(toHex6('#6B7280'), '6b7280')
  assert.equal(toHex6('rgb(107, 114, 128)'), '6b7280')
  assert.equal(toHex6('rgb(107 114 128 / 50%)'), '6b7280')
  for (const bad of ['', 'red', '#6b72', 'rgb(300, 0, 0)', 'oklch(0.5 0.1 200)', 'var(--x)']) {
    assert.equal(toHex6(bad), null, bad)
  }
})

test('cookie 的值和后端 SiteChrome::parse 的格式一致', () => {
  assert.equal(
    siteChromeValue({ setting: 'auto', dark: true, primary: '#6b7280', accent: 'rgb(199, 210, 254)' }),
    'auto.dark.6b7280.c7d2fe',
  )
  assert.equal(siteChromeValue({ setting: 'light', dark: false, primary: '#fff', accent: '#000' }), 'light.light.ffffff.000000')
  assert.equal(siteChromeValue({ setting: 'auto', dark: false, primary: 'oklch(1 0 0)', accent: '#000' }), null)
})
