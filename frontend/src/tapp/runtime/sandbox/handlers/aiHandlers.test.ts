import type { TappMessage } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { registerAIHandlers } from './aiHandlers.ts'

const originalFetch = globalThis.fetch
const originalSessionStorage = globalThis.sessionStorage
const GRANT = 'ai-runtime-grant'
const SECRET = 'top-secret-model-key'

interface CapturedCall {
  url: string
  method: string
  grant?: string
  body?: unknown
  blob: string
}

const calls: CapturedCall[] = []

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

  readonly emits: Array<{ action: string; payload: unknown }> = []

  registerHandler(
    action: string,
    handler: (message: TappMessage) => Promise<unknown>,
  ) {
    this.handlers.set(action, handler)
  }

  async getRuntimeGrant() {
    return GRANT
  }

  emit(action: string, payload: unknown) {
    this.emits.push({ action, payload })
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
    id: 'ai-1',
    action,
    payload: { args },
    timestamp: Date.now(),
  })
}

function mockOk(body: unknown = { id: 'task-1' }) {
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
    return {
      ok: true,
      status: 200,
      json: async () => ({ success: true, data: body }),
    } as Response
  }) as typeof fetch
}

describe('registerAIHandlers', { concurrency: false }, () => {
  it('rejects incomplete create/get/cancel without calling the host', async () => {
    mockOk()
    const bridge = new FakeBridge()
    const stop = registerAIHandlers(bridge as unknown as TappBridge)
    assert.deepEqual(await invoke(bridge, 'ai.tasks.create', []), {
      success: false,
      error: 'Request required',
    })
    assert.deepEqual(await invoke(bridge, 'ai.tasks.get', [1]), {
      success: false,
      error: 'Task ID required',
    })
    assert.deepEqual(await invoke(bridge, 'ai.tasks.cancel', []), {
      success: false,
      error: 'Task ID required',
    })
    assert.equal(calls.length, 0)
    stop()
  })

  it('sends Runtime Grant on create/get/cancel/usage and never a host secret', async () => {
    mockOk({ id: 'task-1', status: 'queued' })
    const bridge = new FakeBridge()
    const stop = registerAIHandlers(bridge as unknown as TappBridge)
    const created = await invoke(bridge, 'ai.tasks.create', [
      { operation: 'generate', prompt: 'hello' },
    ])
    assert.equal((created as { success: boolean }).success, true)
    mockOk({ id: 'task-1', status: 'running' })
    await invoke(bridge, 'ai.tasks.get', ['task-1'])
    mockOk({ success: true, taskId: 'task-1' })
    await invoke(bridge, 'ai.tasks.cancel', ['task-1'])
    mockOk({ used: 1, quota: 10 })
    const usage = await invoke(bridge, 'ai.tasks.usage')
    assert.equal((usage as { success: boolean }).success, true)
    assert.deepEqual(
      calls.map((call) => ({
        method: call.method,
        url: call.url,
        grant: call.grant,
      })),
      [
        {
          method: 'POST',
          url: '/api/tapp/ai/v2/tasks',
          grant: GRANT,
        },
        {
          method: 'GET',
          url: '/api/tapp/ai/v2/tasks/task-1',
          grant: GRANT,
        },
        {
          method: 'DELETE',
          url: '/api/tapp/ai/v2/tasks/task-1',
          grant: GRANT,
        },
        {
          method: 'GET',
          url: '/api/tapp/ai/v2/usage',
          grant: GRANT,
        },
      ],
    )
    assert.deepEqual(calls[0]?.body, {
      operation: 'generate',
      prompt: 'hello',
    })
    assert.equal(
      calls.some((call) => call.blob.includes(SECRET)),
      false,
    )
    stop()
  })
})
