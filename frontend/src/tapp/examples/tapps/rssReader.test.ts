import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { dirname, join } from 'node:path'
import { afterEach, describe, it, mock } from 'node:test'
import { fileURLToPath } from 'node:url'

interface Item {
  id: number
  title?: string
  source_id?: number
  is_read?: boolean
  is_starred?: boolean
  content?: string
}

interface TappStub {
  permissions: string[]
  i18n: {
    t: (key: string, variables?: Record<string, unknown>) => string
    getLocale: () => string
  }
  lifecycle: { getInfo: () => { permissions: string[]; sandboxed: true } }
  ui: {
    setTitle: (title: string) => Promise<void>
    onLocaleChange: (callback: () => void) => void
  }
  file: { download: (...args: unknown[]) => Promise<void> }
  storage?: {
    get: (key: string) => Promise<unknown>
    set: (key: string, value: unknown) => Promise<void>
  }
  phantasiList: {
    sources: (options?: unknown) => Promise<unknown[]>
    categories: () => Promise<unknown[]>
    stats: () => Promise<{ total_unread: number; total_starred?: number }>
    list: (options?: unknown) => Promise<{ items: Item[]; total?: number; next_cursor: string | null }>
    get: (id: number) => Promise<Item>
    markRead: (id: number) => Promise<unknown>
    openOriginal: (id: number) => Promise<unknown>
  }
}

const host = globalThis as typeof globalThis & { Tapp?: TappStub }

const require = createRequire(import.meta.url)
const pageDir = join(
  dirname(fileURLToPath(import.meta.url)),
  '../../../../../tapps/rss-reader/page',
)

/* ---------- 只够阅读器用的假 DOM：属性、类名、id 与后代选择器 ---------- */

interface FakeNode {
  tagName: string
  className: string
  textContent: string
  hidden: boolean
  disabled: boolean
  value: string
  checked: boolean
  style: Record<string, string>
  children: FakeNode[]
  childNodes: FakeNode[]
  parent: FakeNode | null
  attrs: Record<string, string>
  classList: {
    contains: (name: string) => boolean
    toggle: (name: string, on?: boolean) => void
  }
  append: (...nodes: FakeNode[]) => void
  appendChild: (node: FakeNode) => FakeNode
  replaceChildren: (...nodes: FakeNode[]) => void
  setAttribute: (name: string, value: string) => void
  getAttribute: (name: string) => string | null
  removeAttribute: (name: string) => void
  querySelector: (selector: string) => FakeNode | null
  querySelectorAll: (selector: string) => FakeNode[]
  closest: (selector: string) => FakeNode | null
  contains: (node: FakeNode) => boolean
  addEventListener: () => void
  focus: () => void
}

interface Simple { tag?: string; id?: string; classes: string[]; attrs: Array<[string, string | null]> }

function parseSimple(text: string): Simple {
  const simple: Simple = { classes: [], attrs: [] }
  const pattern = /([a-z][a-z0-9-]*)|#([\w-]+)|\.([\w-]+)|\[([\w-]+)(?:\^?="([^"]*)")?\]/gi
  for (const match of text.matchAll(pattern)) {
    if (match[1]) simple.tag = match[1].toLowerCase()
    else if (match[2]) simple.id = match[2]
    else if (match[3]) simple.classes.push(match[3])
    else if (match[4]) simple.attrs.push([match[4], match[5] ?? null])
  }
  return simple
}

function matchSimple(node: FakeNode, simple: Simple): boolean {
  if (node.tagName === '#text') return false
  if (simple.tag && node.tagName.toLowerCase() !== simple.tag) return false
  if (simple.id && node.attrs.id !== simple.id) return false
  const classes = node.className.split(/\s+/)
  if (!simple.classes.every((name) => classes.includes(name))) return false
  return simple.attrs.every(([name, value]) => {
    if (!(name in node.attrs)) return false
    return value === null || node.attrs[name] === value
  })
}

function matches(node: FakeNode, selector: string): boolean {
  return selector.split(',').some((part) => {
    const chain = part.trim().split(/\s+/).map(parseSimple)
    const last = chain.pop()
    if (!last || !matchSimple(node, last)) return false
    let cursor = node.parent
    for (let index = chain.length - 1; index >= 0; index -= 1) {
      while (cursor && !matchSimple(cursor, chain[index])) cursor = cursor.parent
      if (!cursor) return false
      cursor = cursor.parent
    }
    return true
  })
}

function walk(node: FakeNode, visit: (node: FakeNode) => void) {
  visit(node)
  for (const child of node.children) walk(child, visit)
}

function createNode(tag: string): FakeNode {
  const node: FakeNode = {
    tagName: tag.toUpperCase(),
    className: '',
    textContent: '',
    hidden: false,
    disabled: false,
    value: '',
    checked: false,
    style: {},
    children: [],
    get childNodes() {
      return node.children
    },
    parent: null,
    attrs: {},
    classList: {
      contains: (name) => node.className.split(/\s+/).includes(name),
      toggle(name, on) {
        const rest = node.className.split(/\s+/).filter((item) => item && item !== name)
        if (on) rest.push(name)
        node.className = rest.join(' ')
      },
    },
    append(...nodes) {
      for (const child of nodes) node.appendChild(child)
    },
    appendChild(child) {
      child.parent = node
      node.children.push(child)
      return child
    },
    replaceChildren(...nodes) {
      node.children = []
      node.append(...nodes)
    },
    setAttribute(name, value) {
      node.attrs[name] = String(value)
      if (name === 'class') node.className = String(value)
    },
    getAttribute(name) {
      return name in node.attrs ? node.attrs[name] : null
    },
    removeAttribute(name) {
      delete node.attrs[name]
    },
    querySelector(selector) {
      let found: FakeNode | null = null
      walk(node, (current) => {
        if (!found && current !== node && matches(current, selector)) found = current
      })
      return found
    },
    querySelectorAll(selector) {
      const found: FakeNode[] = []
      walk(node, (current) => {
        if (current !== node && matches(current, selector)) found.push(current)
      })
      return found
    },
    closest(selector) {
      let cursor: FakeNode | null = node
      while (cursor && !matches(cursor, selector)) cursor = cursor.parent
      return cursor
    },
    contains(target) {
      let found = false
      walk(node, (current) => {
        if (current === target) found = true
      })
      return found
    },
    addEventListener() {},
    focus() {},
  }
  return node
}

function installDocument() {
  const root = createNode('div')
  root.attrs.id = 'rss-app'
  globalThis.document = {
    getElementById: (id: string) => (id === 'rss-app' ? root : null),
    createElement: (tag: string) => createNode(tag),
    createElementNS: (_ns: string, tag: string) => createNode(tag),
    createTextNode: (text: string) => {
      const node = createNode('#text')
      node.textContent = text
      return node
    },
    createDocumentFragment: () => createNode('#fragment'),
    addEventListener() {},
    documentElement: { lang: '' },
  } as unknown as Document
  ;(globalThis as { DOMParser?: unknown }).DOMParser = class {
    parseFromString() {
      return { body: { childNodes: [] } }
    }
  }
  return root
}

function installTapp(permissions: string[], overrides?: Partial<TappStub['phantasiList']>) {
  const opened: number[] = []
  const marked: number[] = []
  host.Tapp = {
    permissions,
    i18n: {
      t: (key: string) => key,
      getLocale: () => 'zh-CN',
    },
    lifecycle: {
      getInfo: () => ({ permissions, sandboxed: true }),
    },
    ui: {
      setTitle: async () => {},
      onLocaleChange: () => {},
    },
    file: { download: async () => {} },
    storage: {
      get: async () => ({}),
      set: async () => {},
    },
    phantasiList: {
      sources: async () => [],
      categories: async () => [],
      stats: async () => ({ total_unread: 0 }),
      list: async () => ({ items: [], next_cursor: null }),
      get: async (id: number) => ({ id }),
      markRead: async (id: number) => {
        marked.push(id)
      },
      openOriginal: async (id: number) => {
        opened.push(id)
      },
      ...overrides,
    },
  }
  return { opened, marked }
}

interface State {
  scope: { kind: string; value: unknown }
  unreadOnly: boolean
  items: Item[]
  sources: Array<{ id: number; unread_count: number }>
  stats: { total_unread: number }
}
interface Ctx { root: FakeNode; state: State }

function startApp(): Ctx {
  const { start } = require(join(pageDir, 'app.js')) as { start: (root: FakeNode) => Ctx }
  const root = document.getElementById('rss-app') as unknown as FakeNode
  root.children = []
  return start(root)
}

const settle = () => new Promise((resolve) => setImmediate(resolve))

afterEach(() => {
  delete host.Tapp
})

describe('rss reader sandbox page', () => {
  it('only builds source management when phantasi:manage is granted', async () => {
    installDocument()
    installTapp(['phantasi:read', 'phantasi:write'])
    const reader = startApp()
    await settle()
    assert.equal(reader.root.querySelector('[data-action="open-manage"]'), null)
    assert.equal(reader.root.querySelector('[data-role="manage"]'), null)

    installTapp(['phantasi:read', 'phantasi:write', 'phantasi:manage'])
    const managed = startApp()
    await settle()
    assert.ok(managed.root.querySelector('[data-action="open-manage"]'))
    assert.ok(managed.root.querySelector('[data-role="manage"]'))
  })

  it('opens on unread articles and migrates the old stored filter', async () => {
    installDocument()
    installTapp(['phantasi:read'])
    const fresh = startApp()
    await settle()
    assert.equal(fresh.root.querySelector('[data-role="list-title"]')?.textContent, 'scopeAll')
    assert.equal(
      fresh.root.querySelector('[data-action="unread-only"][data-value="true"]')?.getAttribute('aria-pressed'),
      'true',
    )

    installTapp(['phantasi:read', 'storage:read', 'storage:write'])
    assert.ok(host.Tapp?.storage)
    host.Tapp.storage.get = async () => ({ filter: 'starred', category: '', sourceId: null })
    const restored = startApp()
    await settle()
    assert.equal(restored.root.querySelector('[data-role="list-title"]')?.textContent, 'scopeStarred')
    assert.equal(restored.root.querySelector('[data-role="segment"]')?.hidden, true)
  })

  it('marks an opened article read at once and settles the counts', async () => {
    installDocument()
    const { marked } = installTapp(['phantasi:read', 'phantasi:write'], {
      sources: async () => [{ id: 1, name: 'Feed', source_type: 'rss', unread_count: 2 }],
      stats: async () => ({ total_unread: 2 }),
      list: async () => ({ items: [{ id: 3, title: 'Hello', source_id: 1, is_read: false }], next_cursor: null }),
      get: async (id: number) => ({ id, title: 'Hello', source_id: 1, is_read: false, content: '<p>x</p>' }),
    })
    const reader = startApp()
    await settle()
    await settle()
    const { openArticle } = require(join(pageDir, 'reading.js')) as {
      openArticle: (ctx: Ctx, id: number) => Promise<void>
    }
    await openArticle(reader, 3)
    assert.deepEqual(marked, [3])
    assert.equal(reader.state.items[0]?.is_read, true)
    assert.equal(reader.state.sources[0]?.unread_count, 1)
    assert.equal(reader.state.stats.total_unread, 1)
    assert.equal(reader.root.querySelector('.rss-row[data-id="3"]')?.classList.contains('is-unread'), false)
  })

  it('puts the unread state back when the host refuses', async () => {
    mock.timers.enable({ apis: ['setTimeout'] })
    try {
      installDocument()
      installTapp(['phantasi:read', 'phantasi:write'], {
        sources: async () => [{ id: 1, name: 'Feed', source_type: 'rss', unread_count: 1 }],
        stats: async () => ({ total_unread: 1 }),
        list: async () => ({ items: [{ id: 5, source_id: 1, is_read: false }], next_cursor: null }),
        get: async (id: number) => ({ id, source_id: 1, is_read: false }),
        markRead: async () => {
          throw new Error('denied')
        },
      })
      const reader = startApp()
      await settle()
      await settle()
      const { openArticle } = require(join(pageDir, 'reading.js')) as {
        openArticle: (ctx: Ctx, id: number) => Promise<void>
      }
      await openArticle(reader, 5)
      assert.equal(reader.state.items[0]?.is_read, false)
      assert.equal(reader.state.sources[0]?.unread_count, 1)
      assert.equal(reader.root.querySelector('[data-role="toast"]')?.hidden, false)
    } finally {
      mock.timers.reset()
    }
  })

  it('asks the host to open an article by id', async () => {
    const { opened } = installTapp(['phantasi:read'])
    const api = require(join(pageDir, 'api.js')) as {
      openOriginal: (id: number) => Promise<unknown>
    }
    await api.openOriginal(9)
    assert.deepEqual(opened, [9])
  })

  it('returns list data from the unwrapped host payload', async () => {
    installTapp(['phantasi:read'], {
      list: async () => ({ items: [{ id: 3, title: 'Hello' }], next_cursor: null, total: 1 }),
    })
    const api = require(join(pageDir, 'api.js')) as {
      list: () => Promise<{ items: Array<{ id: number }> }>
    }
    const page = await api.list()
    assert.equal(page.items[0]?.id, 3)
  })
})

describe('rss reader state', () => {
  const model = require(join(pageDir, 'model.js')) as {
    UNCATEGORIZED: string
    createState: (permissions: { write?: boolean; manage?: boolean }) => State & Record<string, unknown>
    listQuery: (state: unknown, cursor: string | null, size: number) => Record<string, unknown>
    canMarkAll: (state: unknown) => boolean
    neighbour: (items: Array<{ id: number }>, current: number | null, dir: number) => number | null
    restorePrefs: (state: unknown, raw: unknown) => State & Record<string, unknown>
    prefsOf: (state: unknown) => Record<string, unknown>
  }

  it('keeps where to look apart from unread-only', () => {
    const state = model.createState({ write: true })
    state.scope = { kind: 'source', value: 7 }
    assert.deepEqual(model.listQuery(state, 'c1', 30), {
      limit: 30,
      sort_order: 'desc',
      filter: 'unread',
      source_id: 7,
      cursor: 'c1',
    })
    state.scope = { kind: 'starred', value: null }
    assert.equal(model.listQuery(state, null, 30).filter, 'starred')
    state.scope = { kind: 'category', value: model.UNCATEGORIZED }
    state.unreadOnly = false
    assert.equal(model.listQuery(state, null, 30).category, undefined)
    assert.equal(model.listQuery(state, null, 30).filter, 'all')
  })

  it('does not offer mark-all where it would be wrong', () => {
    const state = model.createState({ write: true })
    assert.equal(model.canMarkAll(state), true)
    state.scope = { kind: 'starred', value: null }
    assert.equal(model.canMarkAll(state), false)
    state.scope = { kind: 'category', value: model.UNCATEGORIZED }
    assert.equal(model.canMarkAll(state), false)
    assert.equal(model.canMarkAll(model.createState({ write: false })), false)
  })

  it('steps to neighbours and stops at the ends', () => {
    const items = [{ id: 1 }, { id: 2 }, { id: 3 }]
    assert.equal(model.neighbour(items, null, 1), 1)
    assert.equal(model.neighbour(items, 2, 1), 3)
    assert.equal(model.neighbour(items, 3, 1), null)
    assert.equal(model.neighbour(items, 1, -1), null)
    assert.equal(model.neighbour(items, null, -1), 1)
  })

  it('round-trips the stored preferences and ignores junk', () => {
    const state = model.createState({})
    state.scope = { kind: 'category', value: 'News' }
    state.unreadOnly = false
    const restored = model.restorePrefs(model.createState({}), model.prefsOf(state))
    assert.deepEqual(restored.scope, { kind: 'category', value: 'News' })
    assert.equal(restored.unreadOnly, false)

    const junk = model.restorePrefs(model.createState({}), {
      version: 3,
      scope: { kind: 'source', value: 'x' },
      fontStep: 99,
      fontFamily: 'comic',
    })
    assert.deepEqual(junk.scope, { kind: 'all', value: null })
    assert.equal(junk.fontStep, 3)
    assert.equal(junk.fontFamily, 'serif')
  })
})
