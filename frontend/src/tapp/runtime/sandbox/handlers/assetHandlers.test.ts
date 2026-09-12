import type { TappInstance, TappMessage } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { registerAssetHandlers } from './baseHandlers.ts'

const originalFetch = globalThis.fetch
const originalSessionStorage = globalThis.sessionStorage
const calls: Array<{ url: string; method: string }> = []

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
    throw new Error('assets must not request a Runtime Grant')
  }
}

const instance: TappInstance = {
  id: 'com.example.assets',
  manifest: {
    id: 'com.example.assets',
    name: 'Assets',
    version: '1.0.0',
    core: { entry: 'core.js' },
    permissions: [],
    category: 'utility',
    assets: ['assets/icon.png', 'assets/data.json'],
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
    id: 'asset-1',
    action,
    payload: { args },
    timestamp: Date.now(),
  })
}

describe('registerAssetHandlers', { concurrency: false }, () => {
  it('lists declared assets and rejects traversal or undeclared paths', async () => {
    const bridge = new FakeBridge()
    registerAssetHandlers(bridge as unknown as TappBridge, instance)
    assert.deepEqual(await invoke(bridge, 'assets.list'), {
      success: true,
      data: ['assets/icon.png', 'assets/data.json'],
    })
    const missing = await invoke(bridge, 'assets.get', [])
    assert.equal((missing as { success: boolean }).success, false)
    const traversal = await invoke(bridge, 'assets.get', ['assets/../secret'])
    assert.equal((traversal as { success: boolean }).success, false)
    assert.match(String((traversal as { error?: string }).error), /Invalid/)
    const absolute = await invoke(bridge, 'assets.get', ['/etc/passwd'])
    assert.equal((absolute as { success: boolean }).success, false)
    const undeclared = await invoke(bridge, 'assets.get', ['assets/secret.png'])
    assert.equal((undeclared as { success: boolean }).success, false)
    assert.match(String((undeclared as { error?: string }).error), /not declared/)
    assert.equal(calls.length, 0)
  })

  it('fetches a declared asset without a Runtime Grant', async () => {
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
      calls.push({ url, method: (init?.method || 'GET').toUpperCase() })
      return {
        ok: true,
        status: 200,
        json: async () => ({
          success: true,
          data: {
            path: 'assets/icon.png',
            mimeType: 'image/png',
            size: 4,
            base64: 'iVBOR',
          },
        }),
      } as Response
    }) as typeof fetch
    const bridge = new FakeBridge()
    registerAssetHandlers(bridge as unknown as TappBridge, instance)
    const result = await invoke(bridge, 'assets.get', ['assets/icon.png'])
    assert.deepEqual(result, {
      success: true,
      data: {
        path: 'assets/icon.png',
        mimeType: 'image/png',
        size: 4,
        base64: 'iVBOR',
      },
    })
    assert.equal(calls.length, 1)
    assert.equal(calls[0]?.method, 'GET')
    assert.match(calls[0]!.url, /\/api\/tapps\/com\.example\.assets\/asset\?/)
    assert.match(calls[0]!.url, /path=assets%2Ficon\.png/)
  })
})
