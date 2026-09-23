import type { TappInstance, TappMessage } from '../types'
import type { TappBridge } from './TappBridge'
import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { registerEventHandlers } from './EventBroker.ts'

const originalFetch = globalThis.fetch
const originalSessionStorage = globalThis.sessionStorage

afterEach(() => {
  globalThis.fetch = originalFetch
  globalThis.sessionStorage = originalSessionStorage
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
    return 'event-grant'
  }

  async getRuntimeId() {
    return 'runtime-host'
  }

  emit(action: string, payload: unknown) {
    this.emits.push({ action, payload })
  }
}

function instance(subscribe: string[] = []): TappInstance {
  return {
    id: 'com.example.events',
    manifest: {
      id: 'com.example.events',
      name: 'Events',
      version: '1.0.0',
      core: { entry: 'core.js' },
      permissions: [],
      category: 'utility',
      events: { subscribe },
    },
    status: 'running',
    installedAt: '2026-09-10T00:00:00Z',
    grantedPermissions: [],
    userRole: 'admin',
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
    id: 'evt-1',
    action,
    payload: { args },
    timestamp: Date.now(),
  })
}

describe('EventBroker', { concurrency: false }, () => {
  it('rejects sandbox publishes of system.* without calling the host API', async () => {
    const calls: string[] = []
    globalThis.fetch = (async (input: RequestInfo | URL) => {
      calls.push(String(input))
      return Response.json(({ success: true, data: { accepted: true } }), { status: 200 })
    }) as typeof fetch
    const bridge = new FakeBridge()
    const stop = registerEventHandlers(
      bridge as unknown as TappBridge,
      instance(['system.theme.changed']),
    )
    const result = await invoke(bridge, 'event.publish', [
      { topic: 'system.theme.changed', scope: 'instance' },
    ])
    assert.equal((result as { success: boolean }).success, false)
    assert.match(String((result as { error?: string }).error), /system\.\*/)
    assert.equal(calls.length, 0)
    stop()
  })

  it('publishes app topics with the Runtime Grant and does not start a stream for system-only subscriptions', async () => {
    installSessionStorage()
    const calls: Array<{ url: string; grant?: string; body?: unknown }> = []
    globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url.includes('/api/csrf-token')) {
        return Response.json(({ csrf_token: null }), { status: 200 })
      }
      const headers = (init?.headers || {}) as Record<string, string>
      calls.push({
        url,
        grant: headers['X-Tapp-Runtime-Grant'],
        body: typeof init?.body === 'string' ? JSON.parse(init.body) : undefined,
      })
      return Response.json(({
          success: true,
          data: { accepted: true, deduplicated: false, delivered: 1, event: {} },
        }), { status: 200 })
    }) as typeof fetch

    const bridge = new FakeBridge()
    const stop = registerEventHandlers(
      bridge as unknown as TappBridge,
      instance(['system.theme.changed']),
    )
    const result = await invoke(bridge, 'event.publish', [
      { topic: 'app.posts.created', scope: 'owner', payload: { id: 1 } },
    ])
    assert.equal((result as { success: boolean }).success, true)
    assert.deepEqual(calls, [
      {
        url: '/api/tapp/events/publish',
        grant: 'event-grant',
        body: {
          topic: 'app.posts.created',
          scope: 'owner',
          payload: { id: 1 },
        },
      },
    ])
    assert.equal(
      calls.some((call) => call.url.includes('/events/stream')),
      false,
    )
    stop()
  })
})
