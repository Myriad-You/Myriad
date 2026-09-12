import assert from 'node:assert/strict'
import { afterEach, describe, it, mock } from 'node:test'

import {
  __resetSiteOwnerProfileInflightForTests,
  fetchSiteOwnerProfile,
} from './useSiteOwnerProfile.ts'

interface FetchCall {
  url: string
  init?: RequestInit
}

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

function mockFetch(
  handler: (url: string, init?: RequestInit) => Response | Promise<Response>,
): { calls: FetchCall[] } {
  const calls: FetchCall[] = []
  mock.method(globalThis, 'fetch', (input: RequestInfo | URL, init?: RequestInit) => {
    const url =
      typeof input === 'string'
        ? input
        : input instanceof URL
          ? input.toString()
          : input.url
    calls.push({ url, init })
    return Promise.resolve(handler(url, init))
  })
  return { calls }
}

afterEach(() => {
  __resetSiteOwnerProfileInflightForTests()
  mock.restoreAll()
})

describe('fetchSiteOwnerProfile', () => {
  it('cold path uses default cache mode and no cache-bust query', async () => {
    const { calls } = mockFetch(() =>
      jsonResponse({
        success: true,
        user_info: {
          name: 'Alice',
          avatar: 'https://avatars.githubusercontent.com/u/1?v=4',
          bio: 'hi',
          platform: 'GitHub',
        },
      }),
    )

    const profile = await fetchSiteOwnerProfile()
    assert.equal(profile?.name, 'Alice')
    assert.equal(calls.length, 1)
    assert.match(calls[0]!.url, /\/api\/profile\/user-info$/)
    assert.equal(calls[0]!.init?.cache, undefined)
    assert.equal(calls[0]!.init?.credentials, 'include')
  })

  it('treats leftover placeholder bios as empty so the UI catalog can fill in', async () => {
    mockFetch(() =>
      jsonResponse({
        success: true,
        user_info: {
          name: 'Alice',
          avatar: null,
          bio: '这家伙很懒，没有介绍呢',
          platform: 'GitHub',
        },
      }),
    )
    const leftover = await fetchSiteOwnerProfile()
    assert.equal(leftover?.bio, '')
    __resetSiteOwnerProfileInflightForTests()
    mockFetch(() =>
      jsonResponse({
        success: true,
        user_info: {
          name: 'Alice',
          avatar: null,
          bio: 'No bio available',
          platform: 'GitHub',
        },
      }),
    )
    const english = await fetchSiteOwnerProfile()
    assert.equal(english?.bio, '')
  })

  it('force path sets cache: no-store and cache-busts the URL', async () => {
    const { calls } = mockFetch(() =>
      jsonResponse({
        success: true,
        user_info: {
          name: 'Bob',
          avatar: null,
          bio: '',
          platform: null,
        },
      }),
    )

    const profile = await fetchSiteOwnerProfile({ force: true })
    assert.equal(profile?.name, 'Bob')
    assert.equal(calls.length, 1)
    assert.match(calls[0]!.url, /\/api\/profile\/user-info\?_ts=\d+$/)
    assert.equal(calls[0]!.init?.cache, 'no-store')
  })

  it('non-force concurrent callers share one in-flight request', async () => {
    const { promise: pending, resolve: resolveFetch } =
      Promise.withResolvers<Response>()
    const { calls } = mockFetch(() => pending)

    const a = fetchSiteOwnerProfile()
    const b = fetchSiteOwnerProfile()
    assert.equal(calls.length, 1)

    resolveFetch(
      jsonResponse({
        success: true,
        user_info: { name: 'Shared', avatar: null, bio: '', platform: null },
      }),
    )

    const [pa, pb] = await Promise.all([a, b])
    assert.equal(pa?.name, 'Shared')
    assert.equal(pb?.name, 'Shared')
    assert.equal(calls.length, 1)
  })

  it('force does not reuse a stale non-force in-flight response', async () => {
    const { promise: coldPending, resolve: resolveCold } =
      Promise.withResolvers<Response>()
    let call = 0
    const { calls } = mockFetch((_url, init) => {
      call += 1
      if (init?.cache === 'no-store') {
        return jsonResponse({
          success: true,
          user_info: {
            name: 'Fresh',
            avatar: 'https://example.com/new.png',
            bio: 'new',
            platform: 'GitHub',
          },
        })
      }
      return coldPending
    })

    const cold = fetchSiteOwnerProfile()
    const forced = await fetchSiteOwnerProfile({ force: true })
    assert.equal(forced?.name, 'Fresh')
    assert.ok(calls.some((c) => c.init?.cache === 'no-store'))

    resolveCold(
      jsonResponse({
        success: true,
        user_info: {
          name: 'Stale',
          avatar: 'https://example.com/old.png',
          bio: 'old',
          platform: 'Bilibili',
        },
      }),
    )
    const coldResult = await cold
    assert.equal(coldResult?.name, 'Stale')

    // force 与 cold 各发一次，互不合并。
    assert.equal(call, 2)
  })

  it('concurrent force callers share one in-flight request', async () => {
    const { promise: pending, resolve: resolveFetch } =
      Promise.withResolvers<Response>()
    const { calls } = mockFetch(() => pending)

    // Simulates notifyAvatarChanged dual-firing avatar + profile-display
    const a = fetchSiteOwnerProfile({ force: true })
    const b = fetchSiteOwnerProfile({ force: true })
    assert.equal(calls.length, 1)

    resolveFetch(
      jsonResponse({
        success: true,
        user_info: { name: 'Once', avatar: null, bio: '', platform: null },
      }),
    )

    const [pa, pb] = await Promise.all([a, b])
    assert.equal(pa?.name, 'Once')
    assert.equal(pb?.name, 'Once')
    assert.equal(calls.length, 1)
  })

  it('returns null on non-OK or malformed payloads', async () => {
    mockFetch(() => jsonResponse({ success: false }, 404))
    assert.equal(await fetchSiteOwnerProfile({ force: true }), null)

    mock.restoreAll()
    __resetSiteOwnerProfileInflightForTests()
    mockFetch(() => jsonResponse({ success: true, user_info: null }))
    assert.equal(await fetchSiteOwnerProfile({ force: true }), null)
  })
})
