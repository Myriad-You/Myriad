import type { TappBridge, TappInstance, TappMessage } from '@myriad/tapp-runtime'
import assert from 'node:assert/strict'
import { afterEach, describe, it, mock } from 'node:test'
import { federationApi } from '../../services/federationApi.ts'
import { setKnownAuthState } from '../../utils/authState.ts'
import { registerFederationHandlers } from './FederationBridge.ts'

const originalFetch = globalThis.fetch
const originalLocalStorage = globalThis.localStorage
const originalWindow = globalThis.window

afterEach(() => {
  globalThis.fetch = originalFetch
  globalThis.localStorage = originalLocalStorage
  globalThis.window = originalWindow
  setKnownAuthState(true)
})

function installLocalStorage() {
  const store = new Map<string, string>()
  globalThis.localStorage = {
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

  grantCalls = 0

  registerHandler(
    action: string,
    handler: (message: TappMessage) => Promise<unknown>,
  ) {
    this.handlers.set(action, handler)
  }

  async getRuntimeGrant() {
    this.grantCalls += 1
    return 'federation-grant'
  }

  emit() {}
}

const instance: TappInstance = {
  id: 'com.example.fed',
  manifest: {
    id: 'com.example.fed',
    name: 'Fed',
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
    id: 'fed-1',
    action,
    payload: { args },
    timestamp: Date.now(),
  })
}

describe('registerFederationHandlers', { concurrency: false }, () => {
  it('rejects rotateKeys without confirm and incomplete follow/object ids', async () => {
    installLocalStorage()
    const bridge = new FakeBridge()
    const stop = registerFederationHandlers(
      bridge as unknown as TappBridge,
      instance,
    )
    const rotate = await invoke(bridge, 'federation.rotateKeys', [false])
    assert.equal((rotate as { success: boolean }).success, false)
    const follow = await invoke(bridge, 'federation.follow', [])
    assert.equal((follow as { success: boolean }).success, false)
    const object = await invoke(bridge, 'federation.getObject', [1])
    assert.equal((object as { success: boolean }).success, false)
    assert.equal(bridge.grantCalls, 0)
    stop()
  })

  it('returns empty session-scoped reads for a known guest without a Runtime Grant', async () => {
    installLocalStorage()
    setKnownAuthState(false)
    const bridge = new FakeBridge()
    const stop = registerFederationHandlers(
      bridge as unknown as TappBridge,
      instance,
    )
    assert.deepEqual(await invoke(bridge, 'federation.getChannels'), {
      success: true,
      data: { channels: [], total: 0 },
    })
    assert.deepEqual(await invoke(bridge, 'federation.getRooms'), {
      success: true,
      data: { rooms: [], total: 0 },
    })
    assert.deepEqual(await invoke(bridge, 'federation.getFollowing'), {
      success: true,
      data: { items: [], total: 0 },
    })
    assert.deepEqual(await invoke(bridge, 'federation.getFollowers'), {
      success: true,
      data: { items: [], total: 0 },
    })
    assert.deepEqual(await invoke(bridge, 'federation.getBookmarks'), {
      success: true,
      data: { items: [], total: 0 },
    })
    assert.deepEqual(await invoke(bridge, 'federation.getPublished'), {
      success: true,
      data: { items: [], total: 0 },
    })
    assert.deepEqual(await invoke(bridge, 'federation.getTimeline'), {
      success: true,
      data: { items: [], total: 0 },
    })
    assert.deepEqual(await invoke(bridge, 'federation.getRings'), {
      success: true,
      data: { rings: [], total: 0 },
    })
    const identity = await invoke(bridge, 'federation.getIdentity')
    assert.equal((identity as { success: boolean }).success, true)
    assert.equal(
      ((identity as { data: { username: string } }).data.username),
      '',
    )
    assert.equal(bridge.grantCalls, 0)
    await invoke(bridge, 'federation.getFeed')
    assert.equal(bridge.grantCalls, 1)
    stop()
  })

  it('gives every publish call its own idempotency key', async () => {
    installLocalStorage()
    const keys: Array<string | undefined> = []
    const reply = async (_req: unknown, _grant?: string, key?: string) => {
      keys.push(key)
      return {
        success: true,
        activity_id: 'a',
        content_type: 'note',
        content_id: 'note_1',
        visibility: 'public',
        delivered_queued: 0,
        author_timeline: true,
      }
    }
    mock.method(federationApi, 'createNote', reply)
    mock.method(federationApi, 'publish', reply)
    const bridge = new FakeBridge()
    const stop = registerFederationHandlers(
      bridge as unknown as TappBridge,
      instance,
    )
    for (const [action, req] of [
      ['federation.createNote', { text: 'one' }],
      ['federation.createNote', { text: 'one' }],
      ['federation.publish', { content_type: 'note', text: 'two' }],
    ] as const) {
      const out = await invoke(bridge, action, [req])
      assert.equal((out as { success: boolean }).success, true)
    }
    assert.equal(keys.length, 3)
    for (const key of keys) assert.match(key ?? '', /^publish-/)
    assert.equal(new Set(keys).size, 3)
    mock.restoreAll()
    stop()
  })
})
