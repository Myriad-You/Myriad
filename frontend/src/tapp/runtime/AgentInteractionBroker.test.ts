import type { TappInstance, TappMessage } from '../types'
import type { TappBridge } from './TappBridge'
import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { registerAgentInteractionHandlers } from './AgentInteractionBroker.ts'

const originalFetch = globalThis.fetch
const originalSessionStorage = globalThis.sessionStorage
const originalLocalStorage = globalThis.localStorage
const originalWindow = globalThis.window
const GRANT = 'agent-runtime-grant'

interface CapturedCall {
  url: string
  method: string
  grant?: string
  body?: unknown
}

const calls: CapturedCall[] = []

afterEach(() => {
  globalThis.fetch = originalFetch
  globalThis.sessionStorage = originalSessionStorage
  globalThis.localStorage = originalLocalStorage
  globalThis.window = originalWindow
  calls.length = 0
})

function installMemoryStorage(): Storage {
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

function installSessionStorage() {
  globalThis.sessionStorage = installMemoryStorage()
}

function installLocalStorage() {
  globalThis.localStorage = installMemoryStorage()
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

  emit() {}
}

const instance: TappInstance = {
  id: 'com.example.agent',
  manifest: {
    id: 'com.example.agent',
    name: 'Agent App',
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
    id: 'agent-1',
    action,
    payload: { args },
    timestamp: Date.now(),
  })
}

function mockOk(data: unknown = { id: 'ix-1' }) {
  installSessionStorage()
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input)
    if (url.includes('/api/csrf-token')) {
      return Response.json(({ csrf_token: null }), { status: 200 })
    }
    const headers = (init?.headers || {}) as Record<string, string>
    calls.push({
      url,
      method: (init?.method || 'GET').toUpperCase(),
      grant: headers['X-Tapp-Runtime-Grant'],
      body: typeof init?.body === 'string' ? JSON.parse(init.body) : undefined,
    })
    return Response.json(({ success: true, data }), { status: 200 })
  }) as typeof fetch
}

describe('registerAgentInteractionHandlers', { concurrency: false }, () => {
  it('accepts and submits results with the Runtime Grant', async () => {
    mockOk({ id: 'ix-1', status: 'accepted' })
    const bridge = new FakeBridge()
    const stop = registerAgentInteractionHandlers(
      bridge as unknown as TappBridge,
      instance,
    )
    const accepted = await invoke(bridge, 'agent.v2.accept', ['ix-1'])
    assert.equal((accepted as { success: boolean }).success, true)
    mockOk({ id: 'ix-1', status: 'done' })
    const submitted = await invoke(bridge, 'agent.v2.result', [
      'ix-1',
      { data: { ok: true }, idempotencyKey: 'result-ix-1' },
    ])
    assert.equal((submitted as { success: boolean }).success, true)
    assert.deepEqual(
      calls.map((call) => ({
        method: call.method,
        url: call.url,
        grant: call.grant,
        body: call.body,
      })),
      [
        {
          method: 'POST',
          url: '/api/tapp/agent/v2/interactions/ix-1/accept',
          grant: GRANT,
          body: undefined,
        },
        {
          method: 'POST',
          url: '/api/tapp/agent/v2/interactions/ix-1/result',
          grant: GRANT,
          body: { data: { ok: true }, idempotencyKey: 'result-ix-1' },
        },
      ],
    )
    stop()
  })

  it('does not call the host when the user denies an intent', async () => {
    mockOk()
    installLocalStorage()
    globalThis.window = { confirm: () => false } as Window
    const bridge = new FakeBridge()
    const stop = registerAgentInteractionHandlers(
      bridge as unknown as TappBridge,
      instance,
    )
    const denied = await invoke(bridge, 'agent.v2.intent', [
      'ix-1',
      { type: 'ui.open', params: { tappId: 'com.other.app' }, reason: 'open' },
    ])
    assert.deepEqual(denied, {
      success: false,
      error: 'User denied Agent intent',
    })
    assert.equal(calls.length, 0)
    stop()
  })

  it('asks window.confirm then posts hostConfirmed intent', async () => {
    mockOk()
    installLocalStorage()
    const prompts: string[] = []
    globalThis.window = {
      confirm: (message?: string) => {
        prompts.push(String(message || ''))
        return true
      },
    } as Window
    const bridge = new FakeBridge()
    const stop = registerAgentInteractionHandlers(
      bridge as unknown as TappBridge,
      instance,
    )
    const executed = await invoke(bridge, 'agent.v2.intent', [
      'ix-1',
      {
        type: 'report.create',
        params: { title: 'Weekly', reportType: 'custom' },
        reason: 'file a report',
      },
    ])
    assert.equal((executed as { success: boolean }).success, true)
    assert.equal(prompts.length, 1)
    assert.match(prompts[0]!, /Agent App/)
    assert.match(prompts[0]!, /report\.create/)
    assert.match(prompts[0]!, /file a report/)
    assert.equal(
      calls.some(
        (call) =>
          call.method === 'POST' &&
          call.url.includes('/api/tapp/agent/v2/interactions/ix-1/intents') &&
          call.grant === GRANT &&
          (call.body as { hostConfirmed?: boolean })?.hostConfirmed === true,
      ),
      true,
    )
    stop()
  })
})
