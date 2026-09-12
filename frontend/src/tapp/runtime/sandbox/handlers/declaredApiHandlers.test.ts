import type { TappInstance, TappMessage } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { registerContextHandlers } from './advancedHandlers.ts'

const originalFetch = globalThis.fetch
const originalSessionStorage = globalThis.sessionStorage
const SECRET = 'top-secret-credential-value'
const GRANT = 'runtime-grant-token'

const calls: Array<{
  url: string
  method: string
  grant?: string
  body?: unknown
  blob: string
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
  id: 'com.example.app',
  manifest: {
    id: 'com.example.app',
    name: 'Example',
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
    id: 'api-1',
    action,
    payload: { args },
    timestamp: Date.now(),
  })
}

function mockHost(handler: (url: string) => {
  ok: boolean
  status: number
  body: unknown
}) {
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
      blob: JSON.stringify({
        url,
        method: init?.method,
        headers,
        body: init?.body,
      }),
    })
    const result = handler(url)
    return {
      ok: result.ok,
      status: result.status,
      json: async () => result.body,
    } as Response
  }) as typeof fetch
}

describe('registerContextHandlers declared API', { concurrency: false }, () => {
  it('rejects a missing API name without calling the host', async () => {
    mockHost(() => ({
      ok: true,
      status: 200,
      body: { success: true, data: {} },
    }))
    const bridge = new FakeBridge()
    registerContextHandlers(bridge as unknown as TappBridge, instance)
    const result = await invoke(bridge, 'api.execute', [undefined, { q: 1 }])
    assert.deepEqual(result, { success: false, error: 'API name required' })
    assert.equal(calls.length, 0)
  })

  it('executes with Runtime Grant and sandbox params only', async () => {
    mockHost(() => ({
      ok: true,
      status: 200,
      body: { success: true, data: { echo: '[REDACTED]' }, cached: true },
    }))
    const bridge = new FakeBridge()
    registerContextHandlers(bridge as unknown as TappBridge, instance)
    const result = await invoke(bridge, 'api.execute', [
      'weather',
      { q: 'tokyo' },
    ])
    assert.deepEqual(result, {
      success: true,
      data: { echo: '[REDACTED]' },
      error: undefined,
    })
    assert.deepEqual(
      calls.map((call) => ({
        url: call.url,
        method: call.method,
        grant: call.grant,
        body: call.body,
      })),
      [
        {
          url: '/api/tapp/com.example.app/api/weather',
          method: 'POST',
          grant: GRANT,
          body: { params: { q: 'tokyo' } },
        },
      ],
    )
    assert.equal(calls[0]!.blob.includes(GRANT), true)
    assert.equal(calls[0]!.blob.includes('Authorization'), false)
    assert.equal(calls[0]!.blob.includes(SECRET), false)
    assert.equal(JSON.stringify(result).includes(SECRET), false)
  })

  it('forwards a redacted host error and never a credential', async () => {
    mockHost(() => ({
      ok: false,
      status: 400,
      body: {
        success: false,
        error: 'HTTP 401 - {"echo":"[REDACTED]"}',
      },
    }))
    const bridge = new FakeBridge()
    registerContextHandlers(bridge as unknown as TappBridge, instance)
    const result = await invoke(bridge, 'api.execute', ['weather', {}])
    assert.deepEqual(result, {
      success: false,
      data: undefined,
      error: 'HTTP 401 - {"echo":"[REDACTED]"}',
    })
    assert.equal(JSON.stringify(result).includes(SECRET), false)
    assert.equal(calls[0]?.grant, GRANT)
  })

  it('lists declared APIs with the Runtime Grant and no secret fields', async () => {
    mockHost(() => ({
      ok: true,
      status: 200,
      body: {
        success: true,
        apis: [
          {
            name: 'weather',
            access: 'protected',
            type: 'http',
            description: 'forecast',
          },
        ],
      },
    }))
    const bridge = new FakeBridge()
    registerContextHandlers(bridge as unknown as TappBridge, instance)
    const result = await invoke(bridge, 'api.list')
    assert.deepEqual(result, {
      success: true,
      data: [
        {
          name: 'weather',
          access: 'protected',
          type: 'http',
          description: 'forecast',
        },
      ],
    })
    assert.equal(calls[0]?.url, '/api/tapp/com.example.app/apis')
    assert.equal(calls[0]?.method, 'GET')
    assert.equal(calls[0]?.grant, GRANT)
    assert.equal(calls[0]?.body, undefined)
    assert.equal(
      Object.hasOwn((result as { data: object[] }).data[0] as object, 'value'),
      false,
    )
    assert.equal(JSON.stringify(result).includes(SECRET), false)
  })
})
