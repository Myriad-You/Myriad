import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { createContext, runInContext } from 'node:vm'
import { siteTransitionInlineScript } from './siteTransitionScript.ts'

type Listener = (event: Record<string, unknown>) => void

function boot(options: { referrer?: string; activationFrom?: string; perfMode?: string } = {}) {
  const listeners = new Map<string, Listener[]>()
  const sandbox = createContext({
    URL,
    location: { href: 'https://site.test/', origin: 'https://site.test' },
    document: {
      referrer: options.referrer ?? '',
      documentElement: {
        getAttribute: (name: string) => (name === 'data-perf-mode' ? (options.perfMode ?? null) : null),
      },
    },
    navigation: options.activationFrom ? { activation: { from: { url: options.activationFrom } } } : undefined,
    addEventListener(type: string, listener: Listener) {
      listeners.set(type, [...(listeners.get(type) ?? []), listener])
    },
  })
  sandbox.window = sandbox
  runInContext(siteTransitionInlineScript(), sandbox)
  const fire = (type: string, event: Record<string, unknown>) => {
    for (const listener of listeners.get(type) ?? []) listener(event)
  }
  /** 放一次换页，交回动效有没有被跳过。 */
  const run = (type: 'pageswap' | 'pagereveal', event: Record<string, unknown> = {}) => {
    let skipped = false
    fire(type, { ...event, viewTransition: { skipTransition: () => { skipped = true } } })
    return skipped
  }
  return { fire, run }
}

const swapTo = (url: string) => ({ activation: { entry: { url } } })

describe('主站离开时', () => {
  it('去博客放动效', () => {
    assert.equal(boot().run('pageswap', swapTo('https://site.test/blog/posts/a/')), false)
    assert.equal(boot().run('pageswap', swapTo('https://site.test/blog')), false)
  })

  it('去别处（登录回跳、整页重定向）直接换页', () => {
    assert.equal(boot().run('pageswap', swapTo('https://site.test/config')), true)
    assert.equal(boot().run('pageswap', swapTo('https://site.test/blogroll')), true)
    assert.equal(boot().run('pageswap', swapTo('https://other.test/blog/')), true)
  })

  it('不知道去哪就看最后点的链接', () => {
    const page = boot()
    page.fire('click', { target: { closest: () => ({ href: 'https://site.test/blog/' }) } })
    assert.equal(page.run('pageswap'), false)
    page.fire('click', { target: { closest: () => null } })
    assert.equal(page.run('pageswap'), true)
  })

  it('最省电的动画档位不放', () => {
    assert.equal(boot({ perfMode: 'exlight' }).run('pageswap', swapTo('https://site.test/blog/')), true)
  })
})

describe('主站打开时', () => {
  it('从博客回来放动效', () => {
    assert.equal(boot({ activationFrom: 'https://site.test/blog/posts/a/' }).run('pagereveal'), false)
    assert.equal(boot({ referrer: 'https://site.test/blog/' }).run('pagereveal'), false)
  })

  it('从别处来直接换页', () => {
    assert.equal(boot({ activationFrom: 'https://site.test/login' }).run('pagereveal'), true)
    assert.equal(boot().run('pagereveal'), true)
  })
})
