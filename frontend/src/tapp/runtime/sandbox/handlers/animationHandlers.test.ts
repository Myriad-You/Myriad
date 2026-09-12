import type { TappMessage } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { registerAnimationHandlers } from './advancedHandlers.ts'

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

async function invoke(
  bridge: FakeBridge,
  action: string,
  args: unknown[] = [],
) {
  const handler = bridge.handlers.get(action)
  assert.ok(handler, action)
  return handler({
    type: 'request',
    id: 'anim-1',
    action,
    payload: { args },
    timestamp: Date.now(),
  })
}

describe('registerAnimationHandlers', () => {
  it('defaults to standard and rejects a missing stagger index', async () => {
    const bridge = new FakeBridge()
    registerAnimationHandlers(bridge as unknown as TappBridge)
    assert.deepEqual(await invoke(bridge, 'animation.getLevel'), {
      success: true,
      data: 'standard',
    })
    assert.deepEqual(await invoke(bridge, 'animation.shouldAnimate'), {
      success: true,
      data: true,
    })
    assert.deepEqual(await invoke(bridge, 'animation.getStaggerDelay', []), {
      success: false,
      error: 'Index required',
    })
    assert.deepEqual(await invoke(bridge, 'animation.getStaggerDelay', [2, 50]), {
      success: true,
      data: 100,
    })
  })
})
