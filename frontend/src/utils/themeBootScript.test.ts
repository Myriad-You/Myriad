import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import { createContext, runInContext } from 'node:vm'
import { themeBootInlineScript } from './themeBootScript.ts'

function bootTheme(options: {
  path?: string
  stored: string | null
  prefersDark: boolean
  embedIn?: { background: string; colorScheme?: string }
}) {
  const html = {
    classList: {
      tokens: new Set<string>(['light']),
      add(name: string) {
        this.tokens.add(name)
      },
      remove(name: string) {
        this.tokens.delete(name)
      },
    },
  }
  const themeColor = { content: '#f5f5f5', setAttribute(name: string, value: string) {
    if (name === 'content') this.content = value
  } }
  const frame = {
    attrs: {} as Record<string, string>,
    setAttribute(name: string, value: string) {
      this.attrs[name] = value
    },
  }
  const sandbox = createContext({
    localStorage: {
      getItem(key: string) {
        return key === 'theme' ? options.stored : null
      },
    },
    document: {
      documentElement: html,
      querySelector(selector: string) {
        return selector === 'meta[name="theme-color"]' ? themeColor : null
      },
    },
    getComputedStyle() {
      return { getPropertyValue: () => '#94a3b8' }
    },
    MutationObserver: class {
      observe() {}
    },
    location: { pathname: options.path ?? (options.embedIn ? '/embed/comments/7' : '/') },
    window: {
      frameElement: options.embedIn ? frame : undefined,
      parent: options.embedIn
        ? {
            document: {
              body: {},
              documentElement: {},
              defaultView: {
                getComputedStyle: () => ({
                  backgroundColor: options.embedIn?.background,
                  colorScheme: options.embedIn?.colorScheme ?? 'normal',
                }),
              },
            },
          }
        : undefined,
      matchMedia() {
        return {
          matches: options.prefersDark,
          addEventListener() {},
        }
      },
    },
  })
  if (!options.embedIn) sandbox.window.parent = sandbox.window
  runInContext(themeBootInlineScript(), sandbox)
  return {
    classes: [...html.classList.tokens].toSorted(),
    frameScheme: frame.attrs['data-embed-scheme'],
    themeColor: themeColor.content,
  }
}

describe('themeBootInlineScript', () => {
  it('is wired into the shared SPA document before React', () => {
    const source = readFileSync(
      new URL('../../index.html', import.meta.url),
      'utf8',
    )
    assert.match(source, /<!-- boot:theme -->/)
    assert.doesNotMatch(source, /function initializeTheme/)
  })

  it('follows a stored light/dark choice and otherwise the system', () => {
    assert.deepEqual(bootTheme({ stored: 'dark', prefersDark: false }).classes, [
      'dark',
    ])
    assert.deepEqual(bootTheme({ stored: 'light', prefersDark: true }).classes, [
      'light',
    ])
    assert.deepEqual(bootTheme({ stored: null, prefersDark: true }).classes, [
      'dark',
    ])
    assert.equal(
      bootTheme({ stored: 'dark', prefersDark: false }).themeColor,
      '#94a3b8',
    )
  })

  it('嵌入页跟着外面那页的底色，不看主站的设置', () => {
    assert.deepEqual(
      bootTheme({ stored: 'light', prefersDark: false, embedIn: { background: 'rgb(28, 25, 23)' } }).classes,
      ['dark', 'embed-frame'],
    )
    assert.deepEqual(
      bootTheme({ stored: 'dark', prefersDark: true, embedIn: { background: 'rgb(247, 245, 242)' } }).classes,
      ['embed-frame', 'light'],
    )
    assert.deepEqual(
      bootTheme({ stored: null, prefersDark: true, embedIn: { background: 'rgba(0, 0, 0, 0)' } }).classes,
      ['dark', 'embed-frame'],
    )
    assert.equal(
      bootTheme({ stored: null, prefersDark: false, embedIn: { background: 'rgb(28, 25, 23)' } }).frameScheme,
      'dark',
    )
    assert.equal(bootTheme({ stored: 'dark', prefersDark: false }).frameScheme, undefined)
  })

  it('正文小组件的框由外面那页负责加载态', () => {
    assert.deepEqual(
      bootTheme({
        stored: null,
        prefersDark: false,
        path: '/embed/widget/quote',
        embedIn: { background: 'rgb(28, 25, 23)' },
      }).classes,
      ['dark', 'embed-frame', 'embed-frame--host-reveal'],
    )
  })
})
