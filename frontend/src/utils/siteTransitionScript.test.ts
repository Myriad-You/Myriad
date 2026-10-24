import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { createContext, runInContext } from 'node:vm'
import { siteTransitionInlineScript } from './siteTransitionScript.ts'

type Listener = (event: Record<string, unknown>) => void

interface Box { x: number; y: number; width: number; height: number }

interface BootOptions {
  referrer?: string
  activationFrom?: string
  perfMode?: string
  storedPerfMode?: string
  path?: string
  /** 换页画面里各个伪元素的位置，按 `::view-transition-group(名字)` 里的名字。 */
  groups?: Record<string, Box>
  /** 主站已经画好了自己的选中项（后台预渲染）。 */
  ownMark?: boolean
}

function boot(options: BootOptions = {}) {
  const listeners = new Map<string, Listener[]>()
  const classes = new Set<string>()
  const timers: (() => void)[] = []
  const animations: { keyframes: Record<string, unknown>; options: Record<string, unknown> }[] = []
  const sandbox = createContext({
    URL,
    DOMMatrixReadOnly: class {
      e: number
      f: number
      constructor(value: string) {
        const [, x, y] = /matrix\(1, 0, 0, 1, (\S+), (\S+)\)/.exec(value) ?? []
        this.e = Number(x ?? 0)
        this.f = Number(y ?? 0)
      }
    },
    location: { href: 'https://site.test/', origin: 'https://site.test', pathname: options.path ?? '/' },
    setTimeout: (callback: () => void) => timers.push(callback),
    localStorage: { getItem: (key: string) => (key === 'myriad-perf-mode' ? (options.storedPerfMode ?? null) : null) },
    getComputedStyle: (_: unknown, pseudo: string) => {
      const box = options.groups?.[/\((.*)\)/.exec(pseudo)![1]]
      return box
        ? { width: `${box.width}px`, height: `${box.height}px`, transform: `matrix(1, 0, 0, 1, ${box.x}, ${box.y})` }
        : { width: 'auto', height: 'auto', transform: 'none' }
    },
    document: {
      referrer: options.referrer ?? '',
      querySelector: (selector: string) => (selector === '.nav-item__mark' && options.ownMark ? {} : null),
      documentElement: {
        getAttribute: (name: string) => (name === 'data-perf-mode' ? (options.perfMode ?? null) : null),
        classList: { add: (name: string) => classes.add(name), remove: (name: string) => classes.delete(name) },
        animate: (keyframes: Record<string, unknown>, animation: Record<string, unknown>) => {
          animations.push(structuredClone({ keyframes, options: animation }))
        },
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
    fire(type, { ...event, viewTransition: { skipTransition: () => { skipped = true }, finished: new Promise(() => {}) } })
    return skipped
  }
  return { fire, run, sandbox, classes, timers, animations }
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

  it('去博客时拍旧页之前打上 to-blog，去别处不打；按返回键回来时摘掉', () => {
    const page = boot()
    page.run('pageswap', swapTo('https://site.test/config'))
    assert.ok(!page.classes.has('to-blog'))
    page.run('pageswap', swapTo('https://site.test/blog/'))
    assert.ok(page.classes.has('to-blog'))
    page.fire('pageshow', {})
    assert.ok(!page.classes.has('to-blog'))
    page.run('pageswap', swapTo('https://site.test/blog/'))
    page.fire('pagereveal', {})
    assert.ok(!page.classes.has('to-blog'))
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

  it('从博客回来时留着过渡，交给主站淡入完再结束，卡住了也会兜底结束', () => {
    const page = boot({ activationFrom: 'https://site.test/blog/' })
    let skipped = 0
    const transition = { skipTransition: () => { skipped++ }, finished: new Promise(() => {}) }
    page.fire('pagereveal', { viewTransition: transition })
    assert.equal(skipped, 0)
    assert.equal(page.sandbox.__myriadBlogReveal, transition)
    assert.ok(page.classes.has('from-blog'))
    page.timers.forEach((timer) => timer())
    assert.equal(skipped, 1)
  })

  it('上一页记下的是最省电的动画档位就不放', () => {
    assert.equal(boot({ activationFrom: 'https://site.test/blog/', storedPerfMode: 'exlight' }).run('pagereveal'), true)
    assert.equal(boot({ activationFrom: 'https://site.test/blog/', storedPerfMode: 'light' }).run('pagereveal'), false)
  })

  /** 从博客回到 `path`，等换页画面准备好，交回对选中底色做了什么。 */
  const reveal = async (options: BootOptions) => {
    const page = boot({ activationFrom: 'https://site.test/blog/', ...options })
    page.fire('pagereveal', {
      viewTransition: { skipTransition: () => {}, finished: new Promise(() => {}), ready: Promise.resolve() },
    })
    await new Promise((resolve) => setImmediate(resolve))
    return page.animations
  }
  const island = {
    'myriad-nav-current': { x: 14, y: 400, width: 44, height: 44 },
    'myriad-nav-icon-main': { x: 26, y: 300, width: 20, height: 20 },
    'myriad-nav-icon-tapp': { x: 26, y: 468, width: 20, height: 20 },
  }

  it('选中底色从博客那一项滑到主站要选中的那一项', async () => {
    const [home] = await reveal({ path: '/', groups: island })
    assert.deepEqual(home.keyframes, { translate: ['0px 0px', '0px -112px'] })
    assert.equal(home.options.pseudoElement, '::view-transition-group(myriad-nav-current)')
    const [tapp] = await reveal({ path: '/tapp/store', groups: island })
    assert.deepEqual(tapp.keyframes, { translate: ['0px 0px', '0px 56px'] })
  })

  it('主站这一页不在导航岛上，或者量不到位置，选中底色就淡掉', async () => {
    for (const options of [{ path: '/config', groups: island }, { path: '/' }]) {
      const [fade] = await reveal(options)
      assert.deepEqual(fade.keyframes, { opacity: [1, 0] })
      assert.equal(fade.options.pseudoElement, '::view-transition-old(myriad-nav-current)')
    }
  })

  it('主站已经画好自己的选中项时交给浏览器配对，不另外动', async () => {
    assert.deepEqual(await reveal({ path: '/', groups: island, ownMark: true }), [])
  })

  it('从别处来直接换页', () => {
    assert.equal(boot({ activationFrom: 'https://site.test/login' }).run('pagereveal'), true)
    assert.equal(boot().run('pagereveal'), true)
  })
})
