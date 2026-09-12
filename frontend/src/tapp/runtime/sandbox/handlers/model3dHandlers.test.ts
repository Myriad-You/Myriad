import type { TappMessage } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { registerModel3dHandlers } from './model3dHandlers.ts'

const originalFetch = globalThis.fetch
const originalSessionStorage = globalThis.sessionStorage
const GRANT = 'model3d-runtime-grant'
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

  grantCalls = 0

  registerHandler(
    action: string,
    handler: (message: TappMessage) => Promise<unknown>,
  ) {
    this.handlers.set(action, handler)
  }

  async getRuntimeGrant() {
    this.grantCalls += 1
    return GRANT
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
    id: 'm3d-1',
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

describe('registerModel3dHandlers', { concurrency: false }, () => {
  it('rejects incomplete upload/task/asset ids without a host call', async () => {
    mockOk({})
    const bridge = new FakeBridge()
    registerModel3dHandlers(bridge as unknown as TappBridge)
    assert.deepEqual(await invoke(bridge, 'model3d.upload', []), {
      success: false,
      error: 'Upload request required',
    })
    assert.deepEqual(await invoke(bridge, 'model3d.createTask', []), {
      success: false,
      error: 'Task request required',
    })
    assert.deepEqual(await invoke(bridge, 'model3d.getTask', [1]), {
      success: false,
      error: 'Task ID required',
    })
    const badAsset = await invoke(bridge, 'model3d.getUrl', ['../evil'])
    assert.equal((badAsset as { success: boolean }).success, false)
    assert.match(String((badAsset as { error?: string }).error), /Invalid 3D asset id/)
    assert.equal(calls.length, 0)
    assert.equal(bridge.grantCalls, 0)
  })

  it('sends Runtime Grant on status and createTask', async () => {
    mockOk({ enabled: true, configured: true, capabilities: [] })
    const bridge = new FakeBridge()
    registerModel3dHandlers(bridge as unknown as TappBridge)
    const status = await invoke(bridge, 'model3d.status')
    assert.equal((status as { success: boolean }).success, true)
    mockOk({ task_id: 't1' })
    const created = await invoke(bridge, 'model3d.createTask', [
      { operation: 'generate' },
    ])
    assert.equal((created as { success: boolean }).success, true)
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
          url: '/api/tapp/3d/status',
          grant: GRANT,
          body: undefined,
        },
        {
          method: 'POST',
          url: '/api/tapp/3d/tasks',
          grant: GRANT,
          body: { operation: 'generate' },
        },
      ],
    )
  })
})
