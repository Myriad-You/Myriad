import assert from 'node:assert/strict'
import { test } from 'node:test'
import { siteChromeSwitched } from '../../utils/siteChromeCookie.ts'
import { parseSchemeMark } from './companionScheme.ts'

test('认后端写在 <html> 上的标记', () => {
  assert.deepEqual(parseSchemeMark('class html dark -'), {
    kind: 'class',
    target: 'html',
    name: 'class',
    dark: 'dark',
    light: null,
  })
  assert.deepEqual(parseSchemeMark('attr body data-theme dark light'), {
    kind: 'attr',
    target: 'body',
    name: 'data-theme',
    dark: 'dark',
    light: 'light',
  })
  assert.equal(parseSchemeMark('attr body theme dark -')?.name, 'theme')
  for (const bad of [null, '', 'system', 'class head dark -', 'attr html onload x -', 'attr body style x -', 'class html a"b -']) {
    assert.equal(parseSchemeMark(bad), null, String(bad))
  }
})

test('固定成另一边时沿用 cookie 里的颜色', () => {
  assert.equal(siteChromeSwitched('auto.light.8070b0.635198', true), 'dark.dark.8070b0.635198')
  assert.equal(siteChromeSwitched('dark.dark.8070b0.635198', false), 'light.light.8070b0.635198')
  assert.equal(siteChromeSwitched(null, true), 'dark.dark.94a3b8.94a3b8')
  assert.equal(siteChromeSwitched('junk', false), 'light.light.94a3b8.94a3b8')
})
