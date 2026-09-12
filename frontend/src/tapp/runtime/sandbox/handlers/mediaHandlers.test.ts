import type { TappInstance, TappMessage } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { registerMediaHandlers } from './advancedHandlers.ts'

const originalWindow = globalThis.window
const GRANT = 'media-runtime-grant'
const events: Array<{ type: string; detail?: unknown }> = []

afterEach(() => {
  globalThis.window = originalWindow
  events.length = 0
})

function installWindow(state: Record<string, unknown> | null) {
  const target = new EventTarget()
  const fake = target as EventTarget & {
    __musicPlayerState?: Record<string, unknown>
    dispatchEvent: (event: Event) => boolean
  }
  if (state) fake.__musicPlayerState = state
  const originalDispatch = target.dispatchEvent.bind(target)
  fake.dispatchEvent = (event: Event) => {
    events.push({
      type: event.type,
      detail: (event as CustomEvent).detail,
    })
    return originalDispatch(event)
  }
  globalThis.window = fake as unknown as Window
}

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
    return GRANT
  }
}

const instance: TappInstance = {
  id: 'com.example.media',
  manifest: {
    id: 'com.example.media',
    name: 'Media',
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
    id: 'media-1',
    action,
    payload: { args },
    timestamp: Date.now(),
  })
}

describe('registerMediaHandlers', { concurrency: false }, () => {
  it('dispatches player events and skips host calls for high-frequency actions', async () => {
    installWindow({ isPlaying: true, volume: 0.5 })
    const bridge = new FakeBridge()
    const stop = registerMediaHandlers(
      bridge as unknown as TappBridge,
      instance,
    )
    const next = await invoke(bridge, 'media.control', [{ action: 'next' }])
    assert.deepEqual(next, {
      success: true,
      data: { action: 'next', value: undefined },
    })
    assert.equal(
      events.some((event) => event.type === 'music-player-next'),
      true,
    )

    events.length = 0
    const seek = await invoke(bridge, 'media.control', [
      { action: 'seek', value: 12 },
    ])
    assert.equal((seek as { success: boolean }).success, true)
    assert.deepEqual(events, [
      { type: 'music-player-seek', detail: { position: 12 } },
    ])
    stop()
  })

  it('reads status from the host music player state', async () => {
    installWindow({
      isPlaying: true,
      currentTime: 10,
      audioDuration: 100,
      volume: 0.4,
      playMode: 'loop',
      currentSong: {
        id: '1',
        name: 'Song',
        artist: 'A',
        duration: 100,
        source: 'netease',
      },
    })
    const bridge = new FakeBridge()
    const stop = registerMediaHandlers(
      bridge as unknown as TappBridge,
      instance,
    )
    const status = await invoke(bridge, 'media.getStatus')
    assert.equal((status as { success: boolean }).success, true)
    assert.equal(
      (status as { data: { isPlaying: boolean } }).data.isPlaying,
      true,
    )
    assert.equal(
      (status as { data: { currentTrack: { title: string } } }).data
        .currentTrack.title,
      'Song',
    )
    stop()
  })
})
