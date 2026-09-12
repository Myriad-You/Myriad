import type { TappInstance, TappMessage } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  onTappPrivateChange,
  onTappStorageChange,
} from '../../WidgetRuntimeSignals.ts'
import { registerPlaygroundPreviewHandlers } from './playgroundPreviewHandlers.ts'

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
    throw new Error('preview must not request a Runtime Grant')
  }
}

const instance: TappInstance = {
  id: 'com.example.preview',
  manifest: {
    id: 'com.example.preview',
    name: 'Preview',
    version: '1.0.0',
    core: { entry: 'core.js' },
    permissions: [],
    category: 'utility',
  },
  status: 'running',
  installedAt: '2026-09-10T00:00:00Z',
  grantedPermissions: [],
  userRole: 'admin',
  previewMode: true,
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
    id: '1',
    action,
    payload: { args },
    timestamp: Date.now(),
  })
}

describe('registerPlaygroundPreviewHandlers KV', () => {
  it('keeps storage/shared/private in isolated maps', async () => {
    const bridge = new FakeBridge()
    const storage = new Map<string, unknown>()
    const settings = new Map<string, unknown>()
    const shared = new Map<string, unknown>()
    const priv = new Map<string, unknown>()
    registerPlaygroundPreviewHandlers(
      bridge as unknown as TappBridge,
      instance,
      storage,
      settings,
      {},
      shared,
      priv,
    )

    await invoke(bridge, 'private.set', ['token', 'owner-only'])
    await invoke(bridge, 'shared.set', ['posts', [1]])
    await invoke(bridge, 'storage.set', ['ready', true])
    assert.deepEqual(await invoke(bridge, 'private.get', ['token']), {
      success: true,
      data: 'owner-only',
    })
    assert.deepEqual(await invoke(bridge, 'shared.get', ['token']), {
      success: true,
      data: null,
    })
    assert.deepEqual(await invoke(bridge, 'storage.get', ['token']), {
      success: true,
      data: null,
    })
    assert.deepEqual(await invoke(bridge, 'private.keys'), {
      success: true,
      data: ['token'],
    })
    assert.equal(storage.get('ready'), true)
    assert.equal(shared.get('posts') !== undefined, true)
    assert.equal(priv.get('token'), 'owner-only')
  })

  it('emits private changes on the private bus only', async () => {
    const privateHits: string[] = []
    const storageHits: string[] = []
    const offPrivate = onTappPrivateChange((change) => {
      privateHits.push(change.key ?? '')
    })
    const offStorage = onTappStorageChange((change) => {
      storageHits.push(change.key ?? '')
    })
    const bridge = new FakeBridge()
    registerPlaygroundPreviewHandlers(
      bridge as unknown as TappBridge,
      instance,
      new Map(),
      new Map(),
      {},
      new Map(),
      new Map(),
    )
    try {
      await invoke(bridge, 'private.set', ['token', 'x'])
      assert.deepEqual(privateHits, ['token'])
      assert.deepEqual(storageHits, [])
    } finally {
      offPrivate()
      offStorage()
    }
  })

  it('rejects invalid keys and non-JSON values', async () => {
    const bridge = new FakeBridge()
    registerPlaygroundPreviewHandlers(
      bridge as unknown as TappBridge,
      instance,
      new Map(),
      new Map(),
    )
    assert.deepEqual(await invoke(bridge, 'private.set', ['../x', 1]), {
      success: false,
      error: 'Invalid private key',
    })
    assert.deepEqual(await invoke(bridge, 'private.set', ['k', undefined]), {
      success: false,
      error: 'Preview private value is not JSON-serializable',
    })
  })
})
