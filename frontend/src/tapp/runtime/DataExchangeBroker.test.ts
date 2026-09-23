import type { TappInstance, TappMessage } from '../types'
import type { TappBridge } from './TappBridge'
import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { registerDataExchangeHandlers } from './DataExchangeBroker.ts'
import {
  decideDataExchangeConsent,
  getDataExchangeConsentSnapshot,
} from './DataExchangeConsent.ts'

const originalFetch = globalThis.fetch
const originalSessionStorage = globalThis.sessionStorage
const cleanups: Array<() => void> = []

afterEach(() => {
  globalThis.fetch = originalFetch
  globalThis.sessionStorage = originalSessionStorage
  while (cleanups.length > 0) cleanups.pop()?.()
  for (;;) {
    const current = getDataExchangeConsentSnapshot().current
    if (!current) break
    decideDataExchangeConsent(current.prepared.requestId, false)
  }
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

  constructor(
    readonly grant: string,
    readonly ownerId: number,
  ) {}

  registerHandler(
    action: string,
    handler: (message: TappMessage) => Promise<unknown>,
  ) {
    this.handlers.set(action, handler)
  }

  unregisterHandler(action: string) {
    this.handlers.delete(action)
  }

  async getRuntimeGrant() {
    return this.grant
  }

  async getRuntimeOwnerId() {
    return this.ownerId
  }

  emit(action: string, payload: unknown) {
    this.emits.push({ action, payload })
  }
}

function instance(
  id: string,
  exports: Array<{ id: string }> = [],
): TappInstance {
  return {
    id,
    manifest: {
      id,
      name: id,
      version: '1.0.0',
      core: { entry: 'core.js' },
      permissions: [],
      category: 'utility',
      dataExchange: {
        exports: exports.map((item) => ({
          id: item.id,
          schema: {},
          maxBytes: 1024,
        })),
      },
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
    id: 'dx-1',
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

interface ExchangeCall {
  url: string
  method: string
  grant?: string
  body?: unknown
}

function mockExchange(options: {
  prepared?: Record<string, unknown>
  access?: Record<string, unknown>
  consume?: unknown
  hangPrepare?: Promise<void>
  onCall?: (call: ExchangeCall) => void
}) {
  installSessionStorage()
  const calls: ExchangeCall[] = []
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input)
    if (url.includes('/api/csrf-token')) {
      return Response.json(({ csrf_token: null }), { status: 200 })
    }
    const headers = (init?.headers || {}) as Record<string, string>
    const call: ExchangeCall = {
      url,
      method: (init?.method || 'GET').toUpperCase(),
      grant: headers['X-Tapp-Runtime-Grant'],
      body: typeof init?.body === 'string' ? JSON.parse(init.body) : undefined,
    }
    calls.push(call)
    options.onCall?.(call)
    if (options.hangPrepare && url.endsWith('/api/tapp/data-exchange/requests') && call.method === 'POST') {
      await options.hangPrepare
    }
    if (url.endsWith('/api/tapp/data-exchange/requests') && call.method === 'POST') {
      return Response.json(({
          success: true,
          data: {
            requestId: 'req-1',
            requesterTappId: 'com.requester.app',
            requesterName: 'Requester',
            providerTappId: 'com.provider.app',
            providerOwnerId: 1,
            providerName: 'Provider',
            exportId: 'posts',
            params: { q: 1 },
            purpose: 'read posts',
            maxBytes: 1024,
            expiresAt: new Date(Date.now() + 60_000).toISOString(),
            ...options.prepared,
          },
        }), { status: 200 })
    }
    if (url.includes('/authorize')) {
      return Response.json(({
          success: true,
          data: {
            version: 1,
            grantId: 'g-1',
            token: 'one-shot-secret-token',
            requestId: 'req-1',
            providerTappId: 'com.provider.app',
            providerOwnerId: 1,
            exportId: 'posts',
            params: { q: 1 },
            purpose: 'read posts',
            requestHash: 'hash',
            maxBytes: 1024,
            expiresAt: new Date(Date.now() + 60_000).toISOString(),
            ...options.access,
          },
        }), { status: 200 })
    }
    if (url.endsWith('/api/tapp/data-exchange/consume')) {
      return Response.json(({
          success: true,
          data: options.consume ?? { items: [1] },
        }), { status: 200 })
    }
    return Response.json(({ success: true, data: null }), { status: 200 })
  }) as typeof fetch
  return calls
}

describe('DataExchangeBroker', { concurrency: false }, () => {
  it('rejects undeclared and malformed export ids', async () => {
    const provider = new FakeBridge('provider-grant', 1)
    cleanups.push(
      registerDataExchangeHandlers(
        provider as unknown as TappBridge,
        instance('com.provider.app', [{ id: 'posts' }]),
      ),
    )
    assert.deepEqual(
      await invoke(provider, 'dataExchange.registerProvider', ['posts']),
      { success: true, data: null },
    )
    const undeclared = await invoke(provider, 'dataExchange.registerProvider', [
      'secrets',
    ])
    assert.equal((undeclared as { success: boolean }).success, false)
    assert.match(
      String((undeclared as { error?: string }).error),
      /not declared/,
    )
    const malformed = await invoke(provider, 'dataExchange.registerProvider', [
      '../x',
    ])
    assert.equal((malformed as { success: boolean }).success, false)
    assert.match(String((malformed as { error?: string }).error), /Invalid/)
  })

  it('rejects invalid requests and missing providers', async () => {
    mockExchange({})
    const requester = new FakeBridge('requester-grant', 2)
    cleanups.push(
      registerDataExchangeHandlers(
        requester as unknown as TappBridge,
        instance('com.requester.app'),
      ),
    )
    const invalid = await invoke(requester, 'dataExchange.request', [
      { targetTappId: 'com.provider.app', exportId: 'posts', purpose: '' },
    ])
    assert.equal((invalid as { success: boolean }).success, false)

    const missing = await invoke(requester, 'dataExchange.request', [
      {
        targetTappId: 'com.provider.app',
        exportId: 'posts',
        purpose: 'read posts',
      },
    ])
    assert.equal((missing as { success: boolean }).success, false)
    assert.match(
      String((missing as { error?: string }).error),
      /not running/,
    )
  })

  it('completes a consented round-trip without leaking the one-shot token to the sandbox', async () => {
    const calls = mockExchange({ consume: { items: [1] } })
    const provider = new FakeBridge('provider-grant', 1)
    const requester = new FakeBridge('requester-grant', 2)
    cleanups.push(
      registerDataExchangeHandlers(
        provider as unknown as TappBridge,
        instance('com.provider.app', [{ id: 'posts' }]),
      ),
    )
    cleanups.push(
      registerDataExchangeHandlers(
        requester as unknown as TappBridge,
        instance('com.requester.app'),
      ),
    )
    await invoke(provider, 'dataExchange.registerProvider', ['posts'])

    const pending = invoke(requester, 'dataExchange.request', [
      {
        targetTappId: 'com.provider.app',
        exportId: 'posts',
        purpose: 'read posts',
        params: { q: 1 },
      },
    ])
    await waitFor(
      () => getDataExchangeConsentSnapshot().current?.prepared.requestId === 'req-1',
      'consent prompt',
    )
    assert.equal(decideDataExchangeConsent('req-1', true), true)
    await waitFor(
      () => provider.emits.some((event) => event.action === 'dataExchange:invoke'),
      'provider invoke',
    )
    const invokeEvent = provider.emits.find(
      (event) => event.action === 'dataExchange:invoke',
    )
    assert.deepEqual(invokeEvent?.payload, {
      requestId: 'req-1',
      exportId: 'posts',
      params: { q: 1 },
      purpose: 'read posts',
    })
    assert.equal(JSON.stringify(invokeEvent).includes('one-shot-secret-token'), false)
    assert.equal(
      JSON.stringify(invokeEvent).includes('requester-grant'),
      false,
    )

    await invoke(provider, 'dataExchange.respond', [
      { requestId: 'req-1', ok: true, data: { items: [1] } },
    ])
    assert.deepEqual(await pending, { success: true, data: { items: [1] } })

    const consume = calls.find((call) => call.url.endsWith('/consume'))
    assert.equal(consume?.grant, 'provider-grant')
    assert.deepEqual(consume?.body, {
      grantToken: 'one-shot-secret-token',
      response: { items: [1] },
    })
    const prepare = calls.find(
      (call) =>
        call.url.endsWith('/api/tapp/data-exchange/requests') &&
        call.method === 'POST',
    )
    assert.equal(prepare?.grant, 'requester-grant')
  })

  it('surfaces a denied consent without invoking the provider', async () => {
    mockExchange({})
    const provider = new FakeBridge('provider-grant', 1)
    const requester = new FakeBridge('requester-grant', 2)
    cleanups.push(
      registerDataExchangeHandlers(
        provider as unknown as TappBridge,
        instance('com.provider.app', [{ id: 'posts' }]),
      ),
    )
    cleanups.push(
      registerDataExchangeHandlers(
        requester as unknown as TappBridge,
        instance('com.requester.app'),
      ),
    )
    await invoke(provider, 'dataExchange.registerProvider', ['posts'])
    const pending = invoke(requester, 'dataExchange.request', [
      {
        targetTappId: 'com.provider.app',
        exportId: 'posts',
        purpose: 'read posts',
      },
    ])
    await waitFor(
      () => getDataExchangeConsentSnapshot().current != null,
      'consent prompt',
    )
    assert.equal(
      decideDataExchangeConsent(
        getDataExchangeConsentSnapshot().current!.prepared.requestId,
        false,
      ),
      true,
    )
    const result = await pending
    assert.equal((result as { success: boolean }).success, false)
    assert.match(String((result as { error?: string }).error), /denied/)
    assert.equal(provider.emits.length, 0)
  })

  it('caps concurrent requests per requester runtime', async () => {
    const { promise: gate, resolve: release } = Promise.withResolvers<void>()
    let prepares = 0
    mockExchange({
      hangPrepare: gate,
      onCall(call) {
        if (
          call.url.endsWith('/api/tapp/data-exchange/requests') &&
          call.method === 'POST'
        ) {
          prepares += 1
        }
      },
    })
    const requester = new FakeBridge('requester-grant', 2)
    cleanups.push(
      registerDataExchangeHandlers(
        requester as unknown as TappBridge,
        instance('com.requester.app'),
      ),
    )
    const request = {
      targetTappId: 'com.provider.app',
      exportId: 'posts',
      purpose: 'read posts',
    }
    const first = invoke(requester, 'dataExchange.request', [request])
    const second = invoke(requester, 'dataExchange.request', [request])
    const third = invoke(requester, 'dataExchange.request', [request])
    await waitFor(() => prepares === 3, 'three in-flight prepares')
    const fourth = await invoke(requester, 'dataExchange.request', [request])
    assert.equal((fourth as { success: boolean }).success, false)
    assert.match(
      String((fourth as { error?: string }).error),
      /Too many active/,
    )
    release()
    await Promise.all([first, second, third])
  })
})
