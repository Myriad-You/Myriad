import type { TappInstance, TappMessage } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { TappScheduler } from '../../TappScheduler.ts'
import { registerSchedulerHandlers } from './schedulerHandlers.ts'

const originalWebSocket = globalThis.WebSocket
const originalFetch = globalThis.fetch
const originalSessionStorage = globalThis.sessionStorage
const sockets: FakeWebSocket[] = []

class FakeWebSocket {
  url: string
  closed = false
  onopen: ((event: Event) => void) | null = null
  onmessage: ((event: MessageEvent) => void) | null = null
  onclose: (() => void) | null = null
  onerror: ((error: Event) => void) | null = null

  constructor(url: string) {
    this.url = url
    sockets.push(this)
  }

  close() {
    this.closed = true
  }

  send() {}
}

afterEach(() => {
  globalThis.WebSocket = originalWebSocket
  globalThis.fetch = originalFetch
  globalThis.sessionStorage = originalSessionStorage
  sockets.length = 0
  TappScheduler.reset()
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
    return 'scheduler-grant'
  }

  emit(action: string, payload: unknown) {
    this.emits.push({ action, payload })
  }
}

const instance: TappInstance = {
  id: 'com.example.sched',
  manifest: {
    id: 'com.example.sched',
    name: 'Sched',
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
    id: 'sch-1',
    action,
    payload: { args },
    timestamp: Date.now(),
  })
}

async function waitFor(predicate: () => boolean, label: string) {
  const deadline = Date.now() + 1000
  while (Date.now() < deadline) {
    if (predicate()) return
    await new Promise((resolve) => setTimeout(resolve, 5))
  }
  throw new Error(`timed out waiting for ${label}`)
}

describe('registerSchedulerHandlers', { concurrency: false }, () => {
  it('keeps HTTP scheduler calls off the websocket', async () => {
    globalThis.WebSocket = FakeWebSocket as unknown as typeof WebSocket
    installSessionStorage()
    const grants: string[] = []
    globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url.includes('/api/csrf-token')) {
        return Response.json(({ csrf_token: null }), { status: 200 })
      }
      const headers = (init?.headers || {}) as Record<string, string>
      grants.push(headers['X-Tapp-Runtime-Grant'] || '')
      return Response.json(({ success: true, tasks: [] }), { status: 200 })
    }) as typeof fetch

    const bridge = new FakeBridge()
    const stop = registerSchedulerHandlers(
      bridge as unknown as TappBridge,
      instance,
    )
    assert.equal(sockets.length, 0)
    await invoke(bridge, 'scheduler.list')
    assert.equal(sockets.length, 0)
    await invoke(bridge, 'scheduler.get', ['daily'])
    assert.equal(sockets.length, 0)
    assert.deepEqual(grants, ['scheduler-grant', 'scheduler-grant'])
    await invoke(bridge, 'scheduler.subscribe', ['daily'])
    assert.equal(sockets.length, 1)
    assert.match(sockets[0]!.url, /\/api\/tapp\/scheduler\/ws/)
    await invoke(bridge, 'scheduler.unsubscribe', ['daily'])
    assert.equal(sockets[0]!.closed, true)
    stop()
  })

  it('rejects incomplete register and complete-without-pending', async () => {
    globalThis.WebSocket = FakeWebSocket as unknown as typeof WebSocket
    const bridge = new FakeBridge()
    const stop = registerSchedulerHandlers(
      bridge as unknown as TappBridge,
      instance,
    )
    const missing = await invoke(bridge, 'scheduler.register', [{}])
    assert.equal((missing as { success: boolean }).success, false)
    const complete = await invoke(bridge, 'scheduler.complete', [1, true])
    assert.equal((complete as { success: boolean }).success, false)
    assert.match(
      String((complete as { error?: string }).error),
      /no longer pending/,
    )
    stop()
  })

  it('resolves a sandbox execution when complete succeeds', async () => {
    globalThis.WebSocket = FakeWebSocket as unknown as typeof WebSocket
    installSessionStorage()
    globalThis.fetch = (async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url.includes('/api/csrf-token')) {
        return Response.json(({ csrf_token: null }), { status: 200 })
      }
      return Response.json(({ success: true }), { status: 200 })
    }) as typeof fetch

    const bridge = new FakeBridge()
    const stop = registerSchedulerHandlers(
      bridge as unknown as TappBridge,
      instance,
    )
    await invoke(bridge, 'scheduler.subscribe', ['daily'])
    assert.equal(sockets.length, 1)
    sockets[0]!.onmessage?.({
      data: JSON.stringify({
        type: 'task:execute',
        task: {
          id: 9,
          taskId: 'daily',
          tappId: 'com.example.sched',
        },
        payload: { n: 1 },
        scheduledAt: '2026-09-10T00:00:00Z',
        executionId: 42,
      }),
    } as MessageEvent)
    await waitFor(
      () => bridge.emits.some((event) => event.action === 'schedulerTask'),
      'schedulerTask emit',
    )
    const emitted = bridge.emits.find((event) => event.action === 'schedulerTask')
    assert.equal(
      (emitted?.payload as { event?: { executionId?: number } }).event
        ?.executionId,
      42,
    )
    const done = await invoke(bridge, 'scheduler.complete', [42, true])
    assert.deepEqual(done, {
      success: true,
      data: { executionId: 42, completed: true },
    })
    stop()
  })

  it('does not open websocket for backend-only register', async () => {
    globalThis.WebSocket = FakeWebSocket as unknown as typeof WebSocket
    installSessionStorage()
    globalThis.fetch = (async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url.includes('/api/csrf-token')) {
        return Response.json(({ csrf_token: null }), { status: 200 })
      }
      return Response.json(({ success: true, task: { taskId: 'job' } }), { status: 200 })
    }) as typeof fetch

    const bridge = new FakeBridge()
    const stop = registerSchedulerHandlers(
      bridge as unknown as TappBridge,
      instance,
    )
    const result = await invoke(bridge, 'scheduler.register', [
      {
        taskId: 'job',
        name: 'Job',
        scheduleType: 'interval',
        schedule: { interval: 60 },
        executionTarget: 'backend',
      },
    ])
    assert.equal((result as { success: boolean }).success, true)
    assert.equal(sockets.length, 0)
    stop()
  })

  it('opens websocket for frontend register', async () => {
    globalThis.WebSocket = FakeWebSocket as unknown as typeof WebSocket
    installSessionStorage()
    globalThis.fetch = (async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url.includes('/api/csrf-token')) {
        return Response.json(({ csrf_token: null }), { status: 200 })
      }
      return Response.json(({ success: true, task: { taskId: 'daily' } }), { status: 200 })
    }) as typeof fetch

    const bridge = new FakeBridge()
    const stop = registerSchedulerHandlers(
      bridge as unknown as TappBridge,
      instance,
    )
    const result = await invoke(bridge, 'scheduler.register', [
      {
        taskId: 'daily',
        name: 'Daily',
        scheduleType: 'cron',
        schedule: { cron: '0 9 * * *' },
      },
    ])
    assert.equal((result as { success: boolean }).success, true)
    assert.equal(sockets.length, 1)
    stop()
  })
})
