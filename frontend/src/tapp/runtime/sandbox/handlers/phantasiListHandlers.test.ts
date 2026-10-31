import type { TappBridge, TappInstance, TappMessage } from '@myriad/tapp-runtime'
import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { registerPhantasiListHandlers } from './contentHandlers.ts'

const originalFetch = globalThis.fetch
const originalSessionStorage = globalThis.sessionStorage
const originalDocument = globalThis.document
const openedTabs: Array<{ href: string; target: string; rel: string }> = []
const GRANT = 'phantasi-runtime-grant'
const calls: Array<{
  url: string
  method: string
  grant?: string
  body?: string
}> = []

afterEach(() => {
  globalThis.fetch = originalFetch
  globalThis.sessionStorage = originalSessionStorage
  globalThis.document = originalDocument
  calls.length = 0
  openedTabs.length = 0
})

function installDocument() {
  const body = {
    appendChild(node: unknown) {
      return node
    },
    removeChild(node: unknown) {
      return node
    },
  }
  globalThis.document = {
    createElement() {
      const el = {
        href: '',
        target: '',
        rel: '',
        style: { display: '' },
        click() {
          openedTabs.push({ href: el.href, target: el.target, rel: el.rel })
        },
      }
      return el
    },
    body,
  } as unknown as Document
}

function installSessionStorage() {
  const store = new Map<string, string>()
  globalThis.sessionStorage = {
    getItem: (key: string) => (store.has(key) ? store.get(key)! : null),
    setItem: (key: string, value: string) => {
      store.set(key, value)
    },
    removeItem: (key: string) => {
      store.delete(key)
    },
  } as Storage
}

class FakeBridge {
  readonly handlers = new Map<
    string,
    (message: TappMessage) => Promise<unknown>
  >()

  registerHandler(
    action: string,
    handler: (message: TappMessage) => Promise<unknown>,
  ) {
    this.handlers.set(action, handler)
  }

  async getRuntimeGrant() {
    return GRANT
  }

  async hostAttributionHeaders() {
    return { 'X-Tapp-Runtime-Grant': GRANT }
  }
}

const instance: TappInstance = {
  id: 'com.example.phantasi',
  manifest: {
    id: 'com.example.phantasi',
    name: 'Phantasi',
    version: '1.0.0',
    core: { entry: 'core.js' },
    permissions: [],
    category: 'utility',
  },
  status: 'running',
  installedAt: '2026-09-10T00:00:00Z',
  grantedPermissions: [],
  userRole: 'admin',
}

async function invoke(
  bridge: FakeBridge,
  action: string,
  args: unknown[] = [],
) {
  const handler = bridge.handlers.get(action)
  assert.ok(handler, action)
  return handler({
    type: 'request',
    id: 'phantasi-1',
    action,
    payload: { args },
    timestamp: Date.now(),
  })
}

function mockPhantasi(body: unknown) {
  installSessionStorage()
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input)
    if (url.includes('/api/csrf-token')) {
      return Response.json(({ csrf_token: null }), { status: 200 })
    }
    const headers = (init?.headers || {}) as Record<string, string>
    calls.push({
      url,
      method: (init?.method || 'GET').toUpperCase(),
      grant: headers['X-Tapp-Runtime-Grant'],
      body: typeof init?.body === 'string' ? init.body : undefined,
    })
    return Response.json(body, { status: 200 })
  }) as typeof fetch
}

describe('registerPhantasiListHandlers', { concurrency: false }, () => {
  it('lists items with host attribution headers', async () => {
    mockPhantasi({
      items: [
        {
          id: 9,
          title: 'Hello',
          link: 'https://example.com/a',
          summary: 's',
          image: '',
          author: '',
          source_name: 'Src',
          source_icon: '',
          published_at: '2026-09-10T00:00:00Z',
          is_read: false,
          is_starred: false,
        },
      ],
      total: 1,
    })
    const bridge = new FakeBridge()
    registerPhantasiListHandlers(bridge as unknown as TappBridge, instance)
    const result = await invoke(bridge, 'phantasiList.list', [{ limit: 10 }])
    assert.equal((result as { success: boolean }).success, true)
    assert.equal(
      (result as { data: { items: Array<{ id: number }> } }).data.items[0]?.id,
      9,
    )
    assert.equal(calls.length, 1)
    assert.equal(calls[0]?.method, 'GET')
    assert.match(String(calls[0]?.url), /\/api\/phantasi\/items\?/)
    assert.match(String(calls[0]?.url), /per_page=10/)
    assert.equal(calls[0]?.grant, GRANT)
  })

  it('forwards cursor and category and returns next_cursor', async () => {
    mockPhantasi({
      items: [],
      total: 0,
      page: 1,
      per_page: 20,
      next_cursor: 'us:1:2',
    })
    const bridge = new FakeBridge()
    registerPhantasiListHandlers(bridge as unknown as TappBridge, instance)
    const result = await invoke(bridge, 'phantasiList.list', [
      {
        cursor: 'us:1:1',
        category: 'News',
        filter: 'unread',
        limit: 20,
        sort_order: 'asc',
      },
    ])
    const data = (result as { data: { next_cursor: string } }).data
    assert.equal(data.next_cursor, 'us:1:2')
    const url = String(calls[0]?.url)
    assert.match(url, /cursor=us%3A1%3A1/)
    assert.match(url, /category=News/)
    assert.match(url, /filter=unread/)
    assert.match(url, /sort_order=asc/)
    assert.match(url, /per_page=20/)
    assert.doesNotMatch(url, /[?&]page=/)
  })

  it('keeps catalog sources unless a board is requested', async () => {
    mockPhantasi({
      sources: [
        {
          id: 3,
          name: 'Feed',
          url: 'https://example.com/feed.xml',
          icon: '',
          description: '',
          category: 'News',
          source_type: 'rss',
          feed_type: 'rss',
          site_url: 'https://example.com',
          update_interval: 60,
          enabled: true,
          item_count: 4,
          unread_count: 2,
          last_error: '',
          last_fetched_at: 1700000000000,
          last_success_at: 1700000000000,
          error_count: 0,
        },
      ],
    })
    const bridge = new FakeBridge()
    registerPhantasiListHandlers(bridge as unknown as TappBridge, instance)
    await invoke(bridge, 'phantasiList.sources', [])
    assert.match(String(calls[0]?.url), /view=catalog/)
    assert.doesNotMatch(String(calls[0]?.url), /board=/)

    calls.length = 0
    const listed = await invoke(bridge, 'phantasiList.sources', [
      { board: 'feeds' },
    ])
    assert.match(String(calls[0]?.url), /board=feeds/)
    assert.doesNotMatch(String(calls[0]?.url), /view=catalog/)
    const row = (listed as {
      data: Array<{ unread_count: number; source_type: string; last_success_at: number | null }>
    }).data[0]
    assert.equal(row?.unread_count, 2)
    assert.equal(row?.source_type, 'rss')
    assert.equal(row?.last_success_at, 1700000000000)
  })

  it('opens the host article link and refuses anything else', async () => {
    installDocument()
    installSessionStorage()
    const articles = new Map<number, { status: number; link?: string }>([
      [9, { status: 200, link: 'https://news.example/posts/1?x=1' }],
      [10, { status: 200, link: 'javascript:alert(1)' }],
      [11, { status: 200, link: 'https://user:secret@news.example/a' }],
      [12, { status: 404 }],
    ])
    globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url.includes('/api/csrf-token')) {
        return Response.json({ csrf_token: null }, { status: 200 })
      }
      const headers = (init?.headers || {}) as Record<string, string>
      calls.push({
        url,
        method: (init?.method || 'GET').toUpperCase(),
        grant: headers['X-Tapp-Runtime-Grant'],
      })
      const id = Number(url.match(/\/items\/(\d+)/)?.[1])
      const article = articles.get(id)
      if (!article || article.status !== 200) {
        return Response.json({ error: 'not found' }, { status: 404 })
      }
      return Response.json({
        success: true,
        item: {
          id,
          title: 'Hello',
          link: article.link,
          content: '',
        },
      })
    }) as typeof fetch

    const bridge = new FakeBridge()
    registerPhantasiListHandlers(bridge as unknown as TappBridge, instance)
    const denied = await invoke(bridge, 'phantasiList.openOriginal', [9])
    assert.equal((denied as { success: boolean }).success, false)
    assert.match(
      String((denied as { error?: string }).error),
      /phantasi:read/,
    )
    assert.equal(calls.length, 0)
    assert.equal(openedTabs.length, 0)

    const readable = {
      ...instance,
      grantedPermissions: ['phantasi:read'] as typeof instance.grantedPermissions,
    }
    registerPhantasiListHandlers(bridge as unknown as TappBridge, readable)

    const forged = await invoke(bridge, 'phantasiList.openOriginal', [
      'https://evil.example/phish',
    ])
    assert.equal((forged as { success: boolean }).success, false)
    assert.equal(calls.length, 0)
    assert.equal(openedTabs.length, 0)

    const missing = await invoke(bridge, 'phantasiList.openOriginal', [12])
    assert.equal((missing as { success: boolean }).success, false)
    assert.equal(openedTabs.length, 0)
    assert.match(String(calls.at(-1)?.url), /\/items\/12$/)

    const script = await invoke(bridge, 'phantasiList.openOriginal', [10])
    assert.equal((script as { success: boolean }).success, false)
    assert.equal((script as { error?: string }).error, 'article link cannot be opened')
    assert.equal(openedTabs.length, 0)

    const credential = await invoke(bridge, 'phantasiList.openOriginal', [11])
    assert.equal((credential as { success: boolean }).success, false)
    assert.doesNotMatch(String((credential as { error?: string }).error), /secret/)
    assert.equal(openedTabs.length, 0)

    const opened = await invoke(bridge, 'phantasiList.openOriginal', [9])
    assert.deepEqual(opened, { success: true })
    assert.equal(calls.at(-1)?.grant, GRANT)
    assert.deepEqual(openedTabs, [
      {
        href: 'https://news.example/posts/1?x=1',
        target: '_blank',
        rel: 'noopener noreferrer',
      },
    ])
  })

  it('forwards source edit fields and drops credential bags', async () => {
    mockPhantasi({ success: true, source: { id: 8, name: 'Feed', url: 'https://ex/feed' } })
    const bridge = new FakeBridge()
    registerPhantasiListHandlers(bridge as unknown as TappBridge, instance)
    const result = await invoke(bridge, 'phantasiList.updateSource', [
      8,
      {
        name: 'Feed',
        url: 'https://ex/feed',
        category: 'News',
        update_interval: 30,
        enabled: false,
        extra_config: { token: 'secret' },
      },
    ])
    assert.equal((result as { success: boolean }).success, true)
    assert.equal(calls[0]?.method, 'PUT')
    assert.match(String(calls[0]?.url), /\/sources\/8$/)
    const body = JSON.parse(calls[0]?.body || '{}') as Record<string, unknown>
    assert.equal(body.name, 'Feed')
    assert.equal(body.url, 'https://ex/feed')
    assert.equal(body.update_interval, 30)
    assert.equal(body.enabled, false)
    assert.equal(body.extra_config, undefined)
  })
})
