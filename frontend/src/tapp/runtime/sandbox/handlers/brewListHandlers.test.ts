import type { TappInstance, TappMessage } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { registerBrewListHandlers } from './contentHandlers.ts'

const originalFetch = globalThis.fetch
const originalSessionStorage = globalThis.sessionStorage
const GRANT = 'brew-runtime-grant'
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
  id: 'com.example.brew',
  manifest: {
    id: 'com.example.brew',
    name: 'Brew',
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
    id: 'brew-1',
    action,
    payload: { args },
    timestamp: Date.now(),
  })
}

function mockBrew(body: unknown) {
  installSessionStorage()
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input)
    if (url.includes('/api/csrf-token')) {
      return {
        ok: true,
        status: 200,
        json: async () => ({ csrf_token: null }),
      } as Response
    }
    const headers = (init?.headers || {}) as Record<string, string>
    calls.push({
      url,
      method: (init?.method || 'GET').toUpperCase(),
      grant: headers['X-Tapp-Runtime-Grant'],
    })
    return {
      ok: true,
      status: 200,
      json: async () => body,
    } as Response
  }) as typeof fetch
}

describe('registerBrewListHandlers', { concurrency: false }, () => {
  it('lists items with host attribution headers', async () => {
    mockBrew({
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
    registerBrewListHandlers(bridge as unknown as TappBridge, instance)
    const result = await invoke(bridge, 'brewList.list', [{ limit: 10 }])
    assert.equal((result as { success: boolean }).success, true)
    assert.equal(
      (result as { data: { items: Array<{ id: number }> } }).data.items[0]?.id,
      9,
    )
    assert.equal(calls.length, 1)
    assert.equal(calls[0]?.method, 'GET')
    assert.match(String(calls[0]?.url), /\/api\/brew\/items\?/)
    assert.match(String(calls[0]?.url), /per_page=10/)
    assert.equal(calls[0]?.grant, GRANT)
  })
})
