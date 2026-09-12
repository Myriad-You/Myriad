import type { TappInstance, TappMessage } from '../types'
import type { TappBridge } from './TappBridge'
import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
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

  it('returns empty channels for a known guest without a Runtime Grant', async () => {
    installLocalStorage()
    setKnownAuthState(false)
    const bridge = new FakeBridge()
    const stop = registerFederationHandlers(
      bridge as unknown as TappBridge,
      instance,
    )
    const result = await invoke(bridge, 'federation.getChannels')
    assert.deepEqual(result, {
      success: true,
      data: { channels: [], total: 0 },
    })
    assert.equal(bridge.grantCalls, 0)
    stop()
  })
})
