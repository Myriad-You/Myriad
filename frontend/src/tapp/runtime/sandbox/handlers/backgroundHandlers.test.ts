import type { TappInstance, TappMessage } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { TappRuntime } from '../../TappRuntime.ts'
import { registerBackgroundHandlers } from './advancedHandlers.ts'

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
  id: 'com.example.bg',
  manifest: {
    id: 'com.example.bg',
    name: 'BG',
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
    id: 'bg-1',
    action,
    payload: { args },
    timestamp: Date.now(),
  })
}

afterEach(() => {
  TappRuntime.reset()
})

describe('registerBackgroundHandlers', { concurrency: false }, () => {
  it('rejects unknown requirements and registers valid ones', async () => {
    TappRuntime.reset()
    const bridge = new FakeBridge()
    registerBackgroundHandlers(bridge as unknown as TappBridge, instance)
    const missing = await invoke(bridge, 'background.require', [])
    assert.equal((missing as { success: boolean }).success, false)
    const invalid = await invoke(bridge, 'background.require', ['shell'])
    assert.equal((invalid as { success: boolean }).success, false)
    assert.match(String((invalid as { error?: string }).error), /Invalid/)
    const registered = await invoke(bridge, 'background.require', [
      'scheduler',
      'daily digest',
    ])
    assert.deepEqual(registered, {
      success: true,
      data: { requirement: 'scheduler', registered: true },
    })
    assert.deepEqual(await invoke(bridge, 'background.list'), {
      success: true,
      data: ['scheduler'],
    })
    assert.deepEqual(await invoke(bridge, 'background.has', ['scheduler']), {
      success: true,
      data: true,
    })
    assert.deepEqual(await invoke(bridge, 'background.has', ['media']), {
      success: true,
      data: false,
    })
    const released = await invoke(bridge, 'background.release', ['scheduler'])
    assert.deepEqual(released, {
      success: true,
      data: { requirement: 'scheduler', released: true },
    })
    assert.deepEqual(await invoke(bridge, 'background.list'), {
      success: true,
      data: [],
    })
  })
})
