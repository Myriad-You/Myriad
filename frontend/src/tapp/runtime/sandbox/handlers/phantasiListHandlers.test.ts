import type { TappInstance, TappMessage } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { registerPhantasiListHandlers } from './contentHandlers.ts'

const originalFetch = globalThis.fetch
const originalSessionStorage = globalThis.sessionStorage
const GRANT = 'phantasi-runtime-grant'
const calls: Array<{
  url: string
  method: string
  grant?: string
}> = []

afterEach(() => {
  globalThis.fetch = originalFetch
  globalThis.sessionStorage = originalSessionStorage
  calls.length = 0
})

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
})
