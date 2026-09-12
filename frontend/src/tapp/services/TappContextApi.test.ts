import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { executeTappApi, listTappApis } from './TappContextApi.ts'

const originalFetch = globalThis.fetch
const originalSessionStorage = globalThis.sessionStorage
const SECRET = 'top-secret-credential-value'
const GRANT = 'runtime-grant-token'

interface CapturedCall {
  url: string
  method: string
  grant?: string
  csrf?: string
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

function headerRecord(init?: RequestInit): Record<string, string> {
  return (init?.headers || {}) as Record<string, string>
}

function mockFetch(
  handler: (url: string, init?: RequestInit) => {
    ok: boolean
    status: number
    body: unknown
  },
) {
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
    const headers = headerRecord(init)
    calls.push({
      url,
      method: (init?.method || 'GET').toUpperCase(),
      grant: headers['X-Tapp-Runtime-Grant'],
      csrf: headers['X-CSRF-Token'],
      body: typeof init?.body === 'string' ? JSON.parse(init.body) : undefined,
      blob: JSON.stringify({ url, method: init?.method, headers, body: init?.body }),
    })
    const result = handler(url, init)
    return {
      ok: result.ok,
      status: result.status,
      json: async () => result.body,
    } as Response
  }) as typeof fetch
}

function assertNoSecret(value: unknown) {
  assert.equal(JSON.stringify(value).includes(SECRET), false)
}

describe('TappContextApi declared API client', { concurrency: false }, () => {
  it('POSTs execute with CSRF and Runtime Grant, never a credential', async () => {
    mockFetch(() => ({
      ok: true,
      status: 200,
      body: { success: true, data: { echo: '[REDACTED]' }, cached: false },
    }))
    const result = await executeTappApi(
      'com.example.app',
      'weather',
      { q: 'tokyo' },
      GRANT,
    )
    assert.deepEqual(result, {
      success: true,
      data: { echo: '[REDACTED]' },
      error: undefined,
      cached: false,
    })
    assert.equal(calls.length, 1)
    assert.equal(calls[0]!.url, '/api/tapp/com.example.app/api/weather')
    assert.equal(calls[0]!.method, 'POST')
    assert.equal(calls[0]!.grant, GRANT)
    assert.equal(calls[0]!.csrf, '')
    assert.deepEqual(calls[0]!.body, { params: { q: 'tokyo' } })
    assertNoSecret(calls)
    assert.equal(calls[0]!.blob.includes(SECRET), false)
  })

  it('encodes tapp id and API name on both execute and list', async () => {
    mockFetch((url) => {
      if (url.includes('/apis')) {
        return {
          ok: true,
          status: 200,
          body: {
            success: true,
            apis: [{ name: 'get weather', access: 'protected', type: 'http' }],
          },
        }
      }
      return {
        ok: true,
        status: 200,
        body: { success: true, data: { ok: true } },
      }
    })
    await executeTappApi('com.example/app', 'get weather', { q: 1 }, GRANT)
    const apis = await listTappApis('com.example/app', GRANT)
    assert.equal(
      calls[0]?.url,
      '/api/tapp/com.example%2Fapp/api/get%20weather',
    )
    assert.equal(calls[1]?.url, '/api/tapp/com.example%2Fapp/apis')
    assert.equal(calls[0]?.grant, GRANT)
    assert.equal(calls[1]?.grant, GRANT)
    assert.equal(apis[0]?.name, 'get weather')
    assert.equal(Object.hasOwn(apis[0] as object, 'value'), false)
    assertNoSecret(calls)
    assertNoSecret(apis)
  })

  it('maps HTTP errors from message/error/status without inventing a secret', async () => {
    mockFetch(() => ({
      ok: false,
      status: 400,
      body: {
        success: false,
        error: 'HTTP 401 - {"echo":"[REDACTED]"}',
      },
    }))
    const result = await executeTappApi(
      'com.example.app',
      'weather',
      undefined,
      GRANT,
    )
    assert.deepEqual(result, {
      success: false,
      error: 'HTTP 401 - {"echo":"[REDACTED]"}',
    })
    assertNoSecret(result)

    mockFetch(() => ({
      ok: false,
      status: 409,
      body: {
        code: 'TAPP_CREDENTIAL_MISSING',
        message: 'Required Tapp credential is not configured',
      },
    }))
    const missing = await executeTappApi(
      'com.example.app',
      'weather',
      undefined,
      GRANT,
    )
    assert.equal(missing.success, false)
    assert.equal(missing.error, 'Required Tapp credential is not configured')
    assertNoSecret(missing)

    mockFetch(() => ({
      ok: false,
      status: 502,
      body: {},
    }))
    const fallback = await executeTappApi(
      'com.example.app',
      'weather',
      undefined,
      GRANT,
    )
    assert.equal(fallback.success, false)
    assert.match(String(fallback.error), /502/)
    assertNoSecret(fallback)
  })

  it('does not loop when a rejected Runtime Grant cannot be recovered', async () => {
    mockFetch(() => ({
      ok: false,
      status: 401,
      body: {
        code: 'INVALID_RUNTIME_GRANT',
        message: 'Runtime grant is no longer valid',
      },
    }))
    const result = await executeTappApi(
      'com.example.app',
      'weather',
      { q: 1 },
      GRANT,
    )
    assert.equal(result.success, false)
    assert.equal(result.error, 'Runtime grant is no longer valid')
    assert.equal(calls.length, 1)
    assertNoSecret(result)
  })
})
