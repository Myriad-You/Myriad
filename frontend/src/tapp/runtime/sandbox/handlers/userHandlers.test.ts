import type { TappInstance, TappMessage } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { registerUserHandlers } from './baseHandlers.ts'

const originalFetch = globalThis.fetch
const originalSessionStorage = globalThis.sessionStorage
const GRANT = 'user-runtime-grant'
const calls: Array<{ url: string; grant?: string }> = []

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

  constructor(readonly grant: string | null = GRANT) {}

  registerHandler(
    action: string,
    handler: (message: TappMessage) => Promise<unknown>,
  ) {
    this.handlers.set(action, handler)
  }

  async getRuntimeGrant() {
    if (!this.grant) throw new Error('Tapp runtime grant is not initialized')
    return this.grant
  }
}

function instance(
  role: TappInstance['userRole'] = 'guest',
): TappInstance {
  return {
    id: 'com.example.user',
    manifest: {
      id: 'com.example.user',
      name: 'User',
      version: '1.0.0',
      core: { entry: 'core.js' },
      permissions: [],
      category: 'utility',
    },
    status: 'running',
    installedAt: '2026-09-10T00:00:00Z',
    grantedPermissions: [],
    userRole: role,
  }
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
    id: 'user-1',
    action,
    payload: { args },
    timestamp: Date.now(),
  })
}

function mockContextUser(user: unknown) {
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
    calls.push({ url, grant: headers['X-Tapp-Runtime-Grant'] })
    return {
      ok: true,
      status: 200,
      json: async () => ({ success: true, data: user }),
    } as Response
  }) as typeof fetch
}

describe('registerUserHandlers', { concurrency: false }, () => {
  it('trusts an already resolved admin role without probing', async () => {
    const bridge = new FakeBridge(null)
    registerUserHandlers(bridge as unknown as TappBridge, instance('admin'))
    assert.deepEqual(await invoke(bridge, 'user.getRole'), {
      success: true,
      data: 'admin',
    })
    assert.deepEqual(await invoke(bridge, 'user.isAdmin'), {
      success: true,
      data: true,
    })
    assert.deepEqual(await invoke(bridge, 'user.isGuest'), {
      success: true,
      data: false,
    })
    assert.deepEqual(await invoke(bridge, 'user.isLoggedIn'), {
      success: true,
      data: true,
    })
    assert.equal(calls.length, 0)
  })

  it('promotes a guest from the live context user with Runtime Grant', async () => {
    mockContextUser({
      id: 'user_12',
      username: 'hitomi',
      role: 'user',
      authenticated: true,
    })
    const tapp = instance('guest')
    const bridge = new FakeBridge(GRANT)
    registerUserHandlers(bridge as unknown as TappBridge, tapp)
    assert.deepEqual(await invoke(bridge, 'user.getRole'), {
      success: true,
      data: 'user',
    })
    assert.equal(tapp.userRole, 'user')
    assert.equal(calls[0]?.grant, GRANT)
    assert.match(String(calls[0]?.url), /context\/user|\/user/)
  })
})
