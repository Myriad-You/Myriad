import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import {
  clearPrivate,
  getPrivate,
  getPrivateUsage,
  getShared,
  getStorage,
  listPrivateEntries,
  listPrivateKeys,
  removePrivate,
  setPrivate,
  setShared,
  setStorage,
} from './TappStorageApi.ts'

const originalFetch = globalThis.fetch
const originalSessionStorage = globalThis.sessionStorage
const calls: Array<{
  url: string
  method: string
  grant?: string
  body?: string
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

function mockOk(data: unknown = null) {
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input)
    const headers = (init?.headers || {}) as Record<string, string>
    if (url.includes('/api/csrf-token')) {
      return Response.json(({ csrf_token: null }), { status: 200 })
    }
    calls.push({
      url,
      method: (init?.method || 'GET').toUpperCase(),
      grant: headers['X-Tapp-Runtime-Grant'],
      body: typeof init?.body === 'string' ? init.body : undefined,
    })
    return Response.json(({ success: true, data }), { status: 200 })
  }) as typeof fetch
}

describe('TappStorageApi paths', { concurrency: false }, () => {
  it('encodes install KV keys and never sends a Runtime Grant on private/shared', async () => {
    mockOk(null)
    await getPrivate('com.example.app', 'user:pref')
    await getShared('com.example.app', 'user:pref')
    assert.deepEqual(
      calls.map((call) => call.url),
      [
        '/api/tapps/com.example.app/private/user%3Apref',
        '/api/tapps/com.example.app/shared/user%3Apref',
      ],
    )
    assert.deepEqual(
      calls.map((call) => call.grant),
      [undefined, undefined],
    )
  })

  it('sends Runtime Grant only on subject storage', async () => {
    mockOk({ v: 1 })
    await getStorage('com.example.app', 'ready', 'grant-token')
    assert.equal(calls[0]?.url, '/api/tapps/com.example.app/storage/ready')
    assert.equal(calls[0]?.grant, 'grant-token')
  })

  it('uses collection routes for private keys/entries/usage', async () => {
    mockOk([])
    await listPrivateKeys('com.example.app')
    mockOk({})
    await listPrivateEntries('com.example.app')
    mockOk({ used: 0, quota: 1 })
    await getPrivateUsage('com.example.app')
    assert.deepEqual(
      calls.map((call) => call.url),
      [
        '/api/tapps/com.example.app/private',
        '/api/tapps/com.example.app/private/entries',
        '/api/tapps/com.example.app/private/usage',
      ],
    )
  })

  it('mutates private/shared without a Runtime Grant and storage with one', async () => {
    installSessionStorage()
    mockOk(null)
    await setPrivate('com.example.app', 'token', { secret: 1 })
    await removePrivate('com.example.app', 'token')
    await clearPrivate('com.example.app')
    await setShared('com.example.app', 'posts', [])
    await setStorage('com.example.app', 'ready', true, 'grant-token')
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
          url: '/api/tapps/com.example.app/private/token',
          grant: undefined,
          body: JSON.stringify({ secret: 1 }),
        },
        {
          method: 'DELETE',
          url: '/api/tapps/com.example.app/private/token',
          grant: undefined,
          body: undefined,
        },
        {
          method: 'DELETE',
          url: '/api/tapps/com.example.app/private',
          grant: undefined,
          body: undefined,
        },
        {
          method: 'POST',
          url: '/api/tapps/com.example.app/shared/posts',
          grant: undefined,
          body: JSON.stringify([]),
        },
        {
          method: 'POST',
          url: '/api/tapps/com.example.app/storage/ready',
          grant: 'grant-token',
          body: JSON.stringify(true),
        },
      ],
    )
  })
})
