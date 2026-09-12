import type { TappMessage } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import type { TappStorageChange } from '../../WidgetRuntimeSignals'
import type { FullKvOps } from './kvHandlers.ts'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { registerFullKvHandlers } from './kvHandlers.ts'

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
    return 'grant-token'
  }
}

function request(args: unknown[] = []): TappMessage {
  return {
    type: 'request',
    id: 'req-1',
    action: 'private.set',
    payload: { args },
    timestamp: Date.now(),
  }
}

function memoryOps(calls: unknown[][]): FullKvOps {
  const store = new Map<string, unknown>()
  return {
    async get(_tappId, key, grant) {
      calls.push(['get', key, grant])
      return store.has(key) ? store.get(key) : null
    },
    async set(_tappId, key, value, grant) {
      calls.push(['set', key, value, grant])
      store.set(key, value)
    },
    async remove(_tappId, key, grant) {
      calls.push(['remove', key, grant])
      store.delete(key)
    },
    async keys(_tappId, grant) {
      calls.push(['keys', grant])
      return Iterator.from(store.keys()).toArray()
    },
    async getAll(_tappId, grant) {
      calls.push(['getAll', grant])
      return Object.fromEntries(store)
    },
    async clear(_tappId, grant) {
      calls.push(['clear', grant])
      store.clear()
    },
    async usage(_tappId, grant) {
      calls.push(['usage', grant])
      return { used: store.size, quota: 8 }
    },
  }
}

async function invoke(
  bridge: FakeBridge,
  action: string,
  args: unknown[] = [],
) {
  const handler = bridge.handlers.get(action)
  assert.ok(handler, action)
  return handler(request(args))
}

describe('registerFullKvHandlers', () => {
  it('passes Runtime Grant only when withGrant is true', async () => {
    const withGrant = new FakeBridge()
    const withoutGrant = new FakeBridge()
    const granted: unknown[][] = []
    const plain: unknown[][] = []
    registerFullKvHandlers(
      withGrant as unknown as TappBridge,
      'com.example.app',
      'storage',
      memoryOps(granted),
      () => {},
      { maxValueSize: 1024, withGrant: true },
    )
    registerFullKvHandlers(
      withoutGrant as unknown as TappBridge,
      'com.example.app',
      'private',
      memoryOps(plain),
      () => {},
      { maxValueSize: 1024, withGrant: false },
    )

    await invoke(withGrant, 'storage.get', ['ready'])
    await invoke(withoutGrant, 'private.get', ['token'])
    assert.equal(withGrant.grantCalls, 1)
    assert.equal(withoutGrant.grantCalls, 0)
    assert.deepEqual(granted, [['get', 'ready', 'grant-token']])
    assert.deepEqual(plain, [['get', 'token', undefined]])
  })

  it('emits set/remove/clear only after a successful write', async () => {
    const bridge = new FakeBridge()
    const emitted: TappStorageChange[] = []
    const calls: unknown[][] = []
    const ops = memoryOps(calls)
    registerFullKvHandlers(
      bridge as unknown as TappBridge,
      'com.example.app',
      'shared',
      ops,
      (change) => emitted.push(change),
      { maxValueSize: 1024, withGrant: false },
    )

    const set = await invoke(bridge, 'shared.set', ['posts', [1, 2]])
    assert.deepEqual(set, { success: true, data: null })
    const got = await invoke(bridge, 'shared.get', ['posts'])
    assert.deepEqual(got, { success: true, data: [1, 2] })
    await invoke(bridge, 'shared.remove', ['posts'])
    await invoke(bridge, 'shared.set', ['x', 1])
    await invoke(bridge, 'shared.clear')
    await invoke(bridge, 'shared.get', ['x'])

    assert.deepEqual(
      emitted.map((change) => ({
        key: change.key,
        operation: change.operation,
        tappId: change.tappId,
        source: change.source,
      })),
      [
        {
          key: 'posts',
          operation: 'set',
          tappId: 'com.example.app',
          source: bridge,
        },
        {
          key: 'posts',
          operation: 'remove',
          tappId: 'com.example.app',
          source: bridge,
        },
        {
          key: 'x',
          operation: 'set',
          tappId: 'com.example.app',
          source: bridge,
        },
        {
          key: undefined,
          operation: 'clear',
          tappId: 'com.example.app',
          source: bridge,
        },
      ],
    )
  })

  it('does not emit when the write fails', async () => {
    const bridge = new FakeBridge()
    const emitted: TappStorageChange[] = []
    registerFullKvHandlers(
      bridge as unknown as TappBridge,
      'com.example.app',
      'private',
      {
        ...memoryOps([]),
        async set() {
          throw new Error('owner-only write failed')
        },
      },
      (change) => emitted.push(change),
      { maxValueSize: 1024, withGrant: false },
    )
    const result = await invoke(bridge, 'private.set', ['token', 'x'])
    assert.equal((result as { success: boolean }).success, false)
    assert.equal(typeof (result as { error?: string }).error, 'string')
    assert.equal(emitted.length, 0)
  })

  it('rejects bad keys, oversized values, and non-JSON values without calling ops', async () => {
    const bridge = new FakeBridge()
    const calls: unknown[][] = []
    registerFullKvHandlers(
      bridge as unknown as TappBridge,
      'com.example.app',
      'private',
      memoryOps(calls),
      () => {},
      { maxValueSize: 32, withGrant: false },
    )

    assert.deepEqual(await invoke(bridge, 'private.get', []), {
      success: false,
      error: 'Key is required',
    })
    assert.deepEqual(await invoke(bridge, 'private.get', ['../x']), {
      success: false,
      error: 'Invalid key: Key contains invalid path characters',
    })
    const tooLarge = await invoke(bridge, 'private.set', [
      'k',
      'x'.repeat(64),
    ])
    assert.equal((tooLarge as { success: boolean }).success, false)
    assert.match((tooLarge as { error: string }).error, /too large/i)
    assert.deepEqual(await invoke(bridge, 'private.set', ['k', undefined]), {
      success: false,
      error: 'Value is not JSON-serializable',
    })
    assert.deepEqual(await invoke(bridge, 'private.set', ['k', () => 1]), {
      success: false,
      error: 'Value is not JSON-serializable',
    })
    assert.equal(calls.length, 0)

    const stored = await invoke(bridge, 'private.set', ['k', null])
    assert.deepEqual(stored, { success: true, data: null })
    assert.deepEqual(calls, [['set', 'k', null, undefined]])
  })
})
