import type { TappInstance, TappMessage } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { registerAdvancedHandlers } from './advancedHandlers.ts'

const originalFetch = globalThis.fetch
const originalSessionStorage = globalThis.sessionStorage
const originalWindow = globalThis.window
const GRANT = 'component-runtime-grant'
const calls: Array<{
  url: string
  method: string
  grant?: string
  body?: unknown
}> = []

afterEach(() => {
  globalThis.fetch = originalFetch
  globalThis.sessionStorage = originalSessionStorage
  globalThis.window = originalWindow
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

  getSessionToken() {
    return 'session'
  }
}

const instance: TappInstance = {
  id: 'com.example.comp',
  manifest: {
    id: 'com.example.comp',
    name: 'Comp',
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
    id: 'comp-1',
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

describe('registerAdvancedHandlers component/shortcut', { concurrency: false }, () => {
  it('lists and registers components with the Runtime Grant', async () => {
    mockOk({ success: true, components: [] })
    const bridge = new FakeBridge()
    const stop = registerAdvancedHandlers(
      bridge as unknown as TappBridge,
      instance,
    )
    const listed = await invoke(bridge, 'component.list', ['theme'])
    assert.equal((listed as { success: boolean }).success, true)
    mockOk({ success: true, component: { id: 'dark' } })
    const registered = await invoke(bridge, 'component.registerTheme', [
      { id: 'dark' },
    ])
    assert.equal((registered as { success: boolean }).success, true)
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
          url: '/api/tapp/components/com.example.comp?type=theme',
          grant: GRANT,
          body: undefined,
        },
        {
          method: 'POST',
          url: '/api/tapp/components/register',
          grant: GRANT,
          body: {
            tapp_id: 'com.example.comp',
            component_type: 'theme',
            config: { id: 'dark' },
          },
        },
      ],
    )
    stop()
  })

  it('lists shortcuts with the Runtime Grant and does not rehydrate without the permission', async () => {
    mockOk({ success: true, shortcuts: [] })
    const bridge = new FakeBridge()
    const stop = registerAdvancedHandlers(
      bridge as unknown as TappBridge,
      instance,
    )
    await new Promise((resolve) => setTimeout(resolve, 20))
    assert.equal(
      calls.some((call) => call.url.includes('/api/tapp/shortcuts')),
      false,
    )
    const listed = await invoke(bridge, 'shortcut.list')
    assert.equal((listed as { success: boolean }).success, true)
    assert.equal(calls[0]?.grant, GRANT)
    assert.match(String(calls[0]?.url), /\/api\/tapp\/shortcuts/)
    stop()
  })
})
