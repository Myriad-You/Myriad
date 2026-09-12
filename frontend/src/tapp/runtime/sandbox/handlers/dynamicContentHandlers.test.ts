import type { TappInstance, TappMessage } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { registerDynamicContentHandlers } from './advancedHandlers.ts'

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
}

const instance: TappInstance = {
  id: 'com.example.dyn',
  manifest: {
    id: 'com.example.dyn',
    name: 'Dyn',
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
    id: 'dyn-1',
    action,
    payload: { args },
    timestamp: Date.now(),
  })
}

describe('registerDynamicContentHandlers', () => {
  it('rejects incomplete set/update and returns null when nothing is set', async () => {
    const bridge = new FakeBridge()
    registerDynamicContentHandlers(bridge as unknown as TappBridge, instance)
    assert.deepEqual(await invoke(bridge, 'dynamicContent.set', [{}]), {
      success: false,
      error: 'Icon and text required',
    })
    assert.deepEqual(await invoke(bridge, 'dynamicContent.update', []), {
      success: false,
      error: 'Updates required',
    })
    assert.deepEqual(await invoke(bridge, 'dynamicContent.get'), {
      success: true,
      data: null,
    })
  })
})
