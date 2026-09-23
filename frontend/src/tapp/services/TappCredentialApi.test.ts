import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import {
  getTappCredentialStatuses,
  removeTappCredential,
  setTappCredential,
} from './TappCredentialApi.ts'

const originalFetch = globalThis.fetch
const originalSessionStorage = globalThis.sessionStorage
const calls: Array<{ url: string; method: string; body?: string }> = []

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

function mockOk(data: unknown = null) {
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input)
    if (url.includes('/api/csrf-token')) {
      return Response.json(({ csrf_token: null }), { status: 200 })
    }
    calls.push({
      url,
      method: (init?.method || 'GET').toUpperCase(),
      body: typeof init?.body === 'string' ? init.body : undefined,
    })
    return Response.json(({ success: true, data }), { status: 200 })
  }) as typeof fetch
}

describe('TappCredentialApi', { concurrency: false }, () => {
  it('lists statuses and writes values, never GETs a secret by key', async () => {
    installSessionStorage()
    mockOk([{ key: 'wegame', configured: true, needsReauthorization: false, origins: [] }])
    const statuses = await getTappCredentialStatuses('com.example.app')
    assert.equal(statuses[0]?.configured, true)
    assert.equal(Object.hasOwn(statuses[0] as object, 'value'), false)

    mockOk(null)
    await setTappCredential('com.example.app', 'wegame', 'top-secret')
    await removeTappCredential('com.example.app', 'wegame')

    const tappCalls = calls.filter((call) => call.url.includes('/credentials'))
    assert.deepEqual(
      tappCalls.map((call) => ({
        method: call.method,
        url: call.url,
        body: call.body,
      })),
      [
        {
          method: 'GET',
          url: '/api/tapps/com.example.app/credentials',
          body: undefined,
        },
        {
          method: 'POST',
          url: '/api/tapps/com.example.app/credentials/wegame',
          body: JSON.stringify({ value: 'top-secret' }),
        },
        {
          method: 'DELETE',
          url: '/api/tapps/com.example.app/credentials/wegame',
          body: undefined,
        },
      ],
    )
    assert.equal(
      tappCalls.some((call) => call.method === 'GET' && call.url.includes('/credentials/')),
      false,
    )
  })
})
