import type { TappInstance, TappMessage } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { registerTappListHandlers } from './contentHandlers.ts'

const originalFetch = globalThis.fetch
const originalSessionStorage = globalThis.sessionStorage
const originalLocalStorage = globalThis.localStorage
const calls: Array<{ url: string; method: string; body?: unknown }> = []

afterEach(() => {
  globalThis.fetch = originalFetch
  globalThis.sessionStorage = originalSessionStorage
  globalThis.localStorage = originalLocalStorage
  calls.length = 0
})

function memoryStorage(): Storage {
  const store = new Map<string, string>()
  return {
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
    throw new Error('tappList must not request a Runtime Grant')
  }
}

const instance: TappInstance = {
  id: 'com.example.list',
  manifest: {
    id: 'com.example.list',
    name: 'List',
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
    id: 'list-1',
    action,
    payload: { args },
    timestamp: Date.now(),
  })
}

function mockOk(body: unknown) {
  globalThis.sessionStorage = memoryStorage()
  globalThis.localStorage = memoryStorage()
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input)
    if (url.includes('/api/csrf-token')) {
      return Response.json(({ csrf_token: null }), { status: 200 })
    }
    calls.push({
      url,
      method: (init?.method || 'GET').toUpperCase(),
      body: typeof init?.body === 'string' ? JSON.parse(init.body) : undefined,
    })
    return Response.json(({ success: true, data: body }), { status: 200 })
  }) as typeof fetch
}

describe('registerTappListHandlers', { concurrency: false }, () => {
  it('rejects incomplete install and package lookups without calling the host', async () => {
    mockOk([])
    const bridge = new FakeBridge()
    registerTappListHandlers(bridge as unknown as TappBridge, instance)
    const install = await invoke(bridge, 'tappList.install', [
      { source: 'direct' },
    ])
    assert.equal((install as { success: boolean }).success, false)
    assert.match(String((install as { error?: string }).error), /manifest/)
    const pack = await invoke(bridge, 'tappList.getInstallPackage', [])
    assert.deepEqual(pack, { success: false, error: 'tappId is required' })
    const store = await invoke(bridge, 'tappList.resolveStoreSource', [])
    assert.deepEqual(store, { success: false, error: 'tappId is required' })
    assert.equal(calls.length, 0)
  })

  it('lists installed tapps without a Runtime Grant', async () => {
    mockOk([
      {
        id: 'com.example.demo',
        name: 'Demo',
        version: '1.0.0',
        description: 'Hi',
        icon: '',
        status: 'stopped',
      },
    ])
    const bridge = new FakeBridge()
    registerTappListHandlers(bridge as unknown as TappBridge, instance)
    const listed = await invoke(bridge, 'tappList.list')
    assert.equal((listed as { success: boolean }).success, true)
    assert.deepEqual((listed as { data: Array<{ id: string }> }).data[0]?.id, 'com.example.demo')
    assert.equal(calls[0]?.url, '/api/tapps')
    assert.equal(calls[0]?.method, 'GET')
  })
})
