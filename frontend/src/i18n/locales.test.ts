import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import {
  htmlLang,
  isLocale,
  parseLocale,
  parseLocaleCookie,
} from './locales.ts'

const SHARED_CASES = JSON.parse(
  readFileSync(
    new URL('../../../shared/host_locale_cases.json', import.meta.url),
    'utf8',
  ),
) as {
  parse: Array<{ input: string; output: string | null }>
}

describe('parseLocale', () => {
  it('matches shared/host_locale_cases.json', () => {
    for (const { input, output } of SHARED_CASES.parse) {
      assert.equal(parseLocale(input), output, input)
    }
  })

  it('rejects unknown values as non-locales', () => {
    assert.equal(isLocale('zh-HK'), false)
  })

  it('uses a short html lang for English', () => {
    assert.equal(htmlLang('en-US'), 'en')
    assert.equal(htmlLang('zh-TW'), 'zh-TW')
    assert.equal(htmlLang('ko-KR'), 'ko-KR')
    assert.equal(htmlLang('fr-FR'), 'fr-FR')
    assert.equal(htmlLang('de-DE'), 'de-DE')
  })
})

describe('parseLocaleCookie', () => {
  it('reads the locale cookie among other cookies', () => {
    assert.equal(parseLocaleCookie('theme=dark; locale=zh-TW; sid=1'), 'zh-TW')
    assert.equal(parseLocaleCookie('locale=en-US'), 'en-US')
    assert.equal(parseLocaleCookie('theme=dark'), null)
  })
})
