import type { TappInstance, TappMessage } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { TappRuntime } from '../../TappRuntime.ts'
import { registerWidgetHandlers } from './platformHandlers.ts'

const GRANT = 'widget-runtime-grant'

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
    return GRANT
  }
}

const instance: TappInstance = {
  id: 'com.example.widget',
  manifest: {
    id: 'com.example.widget',
    name: 'Widget',
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
    id: 'widget-1',
    action,
    payload: { args },
    timestamp: Date.now(),
  })
}

afterEach(() => {
  TappRuntime.reset()
})

describe('registerWidgetHandlers', { concurrency: false }, () => {
  it('rejects incomplete register/unregister without a Runtime Grant', async () => {
    TappRuntime.reset()
    const bridge = new FakeBridge()
    registerWidgetHandlers(bridge as unknown as TappBridge, instance)
    assert.deepEqual(await invoke(bridge, 'widget.register', []), {
      success: false,
      error: 'Widget config is required',
    })
    assert.deepEqual(await invoke(bridge, 'widget.unregister', []), {
      success: false,
      error: 'Widget ID is required',
    })
    assert.equal(bridge.grantCalls, 0)
    assert.deepEqual(await invoke(bridge, 'widget.listRegistered'), {
      success: true,
      data: [],
    })
  })

  it('does not register a widget for an uninstalled tapp', async () => {
    TappRuntime.reset()
    const bridge = new FakeBridge()
    registerWidgetHandlers(bridge as unknown as TappBridge, instance)
    const result = await invoke(bridge, 'widget.register', [
      { id: 'clock', name: 'Clock' },
    ])
    assert.equal((result as { success: boolean }).success, false)
    assert.match(
      String((result as { error?: string }).error),
      /uninstalled|not installed/,
    )
    assert.equal(bridge.grantCalls, 1)
  })
})
