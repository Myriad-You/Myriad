import type { TappInstance, TappMessage } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import {
  registerAnalyticsHandlers,
  registerPlatformHandlers,
} from './platformHandlers.ts'

const originalFetch = globalThis.fetch
const originalSessionStorage = globalThis.sessionStorage
const GRANT = 'platform-runtime-grant'
const calls: Array<{
  url: string
  method: string
  grant?: string
  body?: unknown
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
}

const instance: TappInstance = {
  id: 'com.example.platform',
  manifest: {
    id: 'com.example.platform',
    name: 'Platform',
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
    id: 'plat-1',
    action,
    payload: { args },
    timestamp: Date.now(),
  })
}

function mockOk(body: unknown) {
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
      body: typeof init?.body === 'string' ? JSON.parse(init.body) : undefined,
    })
    return {
      ok: true,
      status: 200,
      json: async () => ({ success: true, data: body }),
    } as Response
  }) as typeof fetch
}

describe('registerPlatformHandlers', { concurrency: false }, () => {
  it('rejects incomplete reads and omits writes in read-only mode', async () => {
    const bridge = new FakeBridge()
    registerPlatformHandlers(bridge as unknown as TappBridge, instance, {
      readOnly: true,
    })
    assert.deepEqual(await invoke(bridge, 'platform.getData', []), {
      success: false,
      error: 'Platform required',
    })
    assert.equal(bridge.handlers.has('platform.addItem'), false)
    assert.equal(bridge.handlers.has('platform.addItems'), false)
    assert.equal(bridge.handlers.has('platform.registerPlatform'), false)
  })

  it('lists enabled platforms and writes items with the Runtime Grant', async () => {
    mockOk({
      platforms: [
        { id: 1, name: 'Bangumi', enabled: true, slug: 'bangumi' },
        { id: 2, name: 'Hidden', enabled: false, slug: 'hidden' },
      ],
    })
    const bridge = new FakeBridge()
    registerPlatformHandlers(bridge as unknown as TappBridge, instance)
    const listed = await invoke(bridge, 'platform.listEnabled')
    assert.equal((listed as { success: boolean }).success, true)
    assert.deepEqual(
      ((listed as { data: Array<{ id: string }> }).data).map((row) => row.id),
      ['bangumi'],
    )
    mockOk({ id: 'item-1' })
    const added = await invoke(bridge, 'platform.addItem', [
      { platform: 'bangumi', title: 'demo' },
    ])
    assert.equal((added as { success: boolean }).success, true)
    assert.deepEqual(
      calls.map((call) => ({
        method: call.method,
        url: call.url,
        grant: call.grant,
        body: call.body,
      })),
      [
        {
          method: 'GET',
          url: '/api/platforms',
          grant: GRANT,
          body: undefined,
        },
        {
          method: 'POST',
          url: '/api/tapp/platform/items',
          grant: GRANT,
          body: {
            tapp_id: 'com.example.platform',
            item: { platform: 'bangumi', title: 'demo' },
          },
        },
      ],
    )
  })
})

describe('registerAnalyticsHandlers', { concurrency: false }, () => {
  it('reads summary and visitor card with the Runtime Grant', async () => {
    mockOk({ today: { views: 1, unique_visitors: 1 } })
    const bridge = new FakeBridge()
    registerAnalyticsHandlers(bridge as unknown as TappBridge)
    await invoke(bridge, 'analytics.getSummary', [{ days: 7 }])
    mockOk({ today: { views: 2, unique_visitors: 1 } })
    await invoke(bridge, 'analytics.getVisitorCard')
    assert.deepEqual(
      calls.map((call) => ({
        method: call.method,
        url: call.url,
        grant: call.grant,
      })),
      [
        {
          method: 'GET',
          url: '/api/tapp/analytics/summary?days=7',
          grant: GRANT,
        },
        {
          method: 'GET',
          url: '/api/tapp/analytics/visitor',
          grant: GRANT,
        },
      ],
    )
  })
})
