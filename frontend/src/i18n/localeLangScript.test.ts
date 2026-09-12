import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import { createContext, runInContext } from 'node:vm'
import {
  LOCALE_LANG_DEFAULT_DESC,
  LOCALE_LANG_DEFAULT_TITLE,
  localeLangInlineScript,
} from './localeLangScript.ts'
import { parseLocale } from './locales.ts'

const sharedSource = readFileSync(
  new URL('./parseLocale.shared.js', import.meta.url),
  'utf8',
)

function applyFirstPaint(options: {
  stored?: string | null
  cookie?: string
  languages?: string[]
  language?: string
}) {
  const title = { textContent: LOCALE_LANG_DEFAULT_TITLE }
  const desc = {
    content: LOCALE_LANG_DEFAULT_DESC,
    getAttribute(name: string) {
      return name === 'content' ? this.content : null
    },
    setAttribute(name: string, value: string) {
      if (name === 'content') this.content = value
    },
  }
  const noscript = { textContent: '' }
  const documentElement = { lang: 'en-US' }
  const sandbox = createContext({
    localStorage: {
      getItem() {
        return options.stored ?? null
      },
    },
    document: {
      cookie: options.cookie ?? '',
      documentElement,
      querySelector(selector: string) {
        return selector === 'title' ? title : null
      },
      getElementById(id: string) {
        if (id === 'meta-description') return desc
        if (id === 'noscript-enable-js') return noscript
        return null
      },
    },
    navigator: {
      languages: options.languages,
      language: options.language ?? 'en-US',
    },
  })
  runInContext(localeLangInlineScript(sharedSource), sandbox)
  return { lang: documentElement.lang, title: title.textContent }
}

describe('localeLangInlineScript', () => {
  it('is generated from the shared parser, not a second copy', () => {
    const astro = readFileSync(
      new URL('../components/LocaleLang.astro', import.meta.url),
      'utf8',
    )
    assert.match(astro, /localeLangInlineScript/)
    assert.match(astro, /parseLocale\.shared\.js\?raw/)
    assert.equal(astro.includes('function mapTag'), false)
    const script = localeLangInlineScript(sharedSource)
    assert.equal(script.includes('export '), false)
    assert.match(script, /function parseLocale/)
    assert.match(script, /function resolveHostLocale/)
    const localeLang = readFileSync(
      new URL('./localeLangScript.ts', import.meta.url),
      'utf8',
    )
    assert.match(localeLang, /en-US\.json/)
    assert.equal(localeLang.includes('万千灯火'), false)
    const en = JSON.parse(
      readFileSync(new URL('./en-US.json', import.meta.url), 'utf8'),
    ) as { chrome: { title: string } }
    assert.match(script, new RegExp(RegExp.escape(en.chrome.title)))
  })

  it('uses the same Traditional mapping as parseLocale', () => {
    assert.equal(parseLocale('zh-HK'), 'zh-TW')
    const painted = applyFirstPaint({ stored: 'zh-HK' })
    assert.equal(painted.lang, 'zh-TW')
    assert.match(painted.title ?? '', /萬千燈火/)
  })

  it('honors Accept-Language quality values through the shared parser', () => {
    const painted = applyFirstPaint({
      stored: null,
      languages: undefined,
      language: 'it, zh-TW;q=0.9',
    })
    assert.equal(painted.lang, 'zh-TW')
  })

  it('paints Korean chrome when the stored locale is ko-KR', () => {
    const painted = applyFirstPaint({ stored: 'ko-KR' })
    assert.equal(painted.lang, 'ko-KR')
    assert.match(painted.title ?? '', /만천/)
  })
})
