import type { TappInstance, TappMessage } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { registerReportHandlers } from './aiHandlers.ts'

const originalFetch = globalThis.fetch
const originalSessionStorage = globalThis.sessionStorage
const GRANT = 'report-runtime-grant'
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
  id: 'com.example.report',
  manifest: {
    id: 'com.example.report',
    name: 'Report',
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
    id: 'rep-1',
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
      return Response.json(({ csrf_token: null }), { status: 200 })
    }
    const headers = (init?.headers || {}) as Record<string, string>
    calls.push({
      url,
      method: (init?.method || 'GET').toUpperCase(),
      grant: headers['X-Tapp-Runtime-Grant'],
      body: typeof init?.body === 'string' ? JSON.parse(init.body) : undefined,
    })
    return Response.json(({ success: true, data: body }), { status: 200 })
  }) as typeof fetch
}

describe('registerReportHandlers', { concurrency: false }, () => {
  it('exposes the platform report summary and content through the sandbox bridge', async () => {
    const content = { summary: 'Weekly activity', insights: ['Played'], metadata: { games: 4 } }
    const detail = { id: 7, type: 'platform', platform: 'steam', content, createdAt: '2026-09-22' }
    mockOk(detail)
    const bridge = new FakeBridge()
    registerReportHandlers(bridge as unknown as TappBridge, instance, { readOnly: true })

    assert.deepEqual(await invoke(bridge, 'report.platform.get', ['7']), {
      success: true,
      data: { ...detail, summary: content.summary },
    })
    assert.deepEqual(calls, [{
      url: '/api/tapp/report-catalog/7',
      method: 'GET',
      grant: GRANT,
      body: undefined,
    }])
  })

  it('rejects incomplete reads and omits writes in read-only mode', async () => {
    const bridge = new FakeBridge()
    registerReportHandlers(bridge as unknown as TappBridge, instance, {
      readOnly: true,
    })
    assert.deepEqual(await invoke(bridge, 'report.platform.get', []), {
      success: false,
      error: 'Report ID required',
    })
    assert.deepEqual(await invoke(bridge, 'report.platform.byPlatform', []), {
      success: false,
      error: 'Platform required',
    })
    assert.equal(bridge.handlers.has('report.create'), false)
    assert.equal(bridge.handlers.has('report.update'), false)
    assert.equal(bridge.handlers.has('report.delete'), false)
  })

  it('creates a tapp report with the Runtime Grant', async () => {
    mockOk({ success: true, report: { id: 'r1' } })
    const bridge = new FakeBridge()
    registerReportHandlers(bridge as unknown as TappBridge, instance)
    const created = await invoke(bridge, 'report.create', [
      { title: 'Weekly', reportType: 'custom', content: { n: 1 } },
    ])
    assert.equal((created as { success: boolean }).success, true)
    assert.deepEqual(calls, [
      {
        url: '/api/tapp/reports',
        method: 'POST',
        grant: GRANT,
        body: {
          tapp_id: 'com.example.report',
          title: 'Weekly',
          report_type: 'custom',
          content: { n: 1 },
        },
      },
    ])
  })
})
