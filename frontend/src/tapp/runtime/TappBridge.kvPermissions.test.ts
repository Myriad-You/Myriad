import type { TappInstance, TappPermission } from '../types'
import assert from 'node:assert/strict'
import { afterEach, beforeEach, describe, it } from 'node:test'
import { TappBridge } from './TappBridge.ts'

const SESSION = 'kv-perm-session-token'
const READS = [
  'storage.get',
  'shared.get',
  'private.get',
  'settings.get',
] as const
const WRITES = [
  'storage.set',
  'shared.set',
  'private.set',
  'settings.set',
] as const

function makeInstance(permissions: TappPermission[] = []): TappInstance {
  return {
    id: 'com.example.kv-perm',
    manifest: {
      id: 'com.example.kv-perm',
      name: 'KV Perm',
      version: '1.0.0',
      core: { entry: 'core.js' },
      permissions,
      category: 'utility',
    },
    status: 'running',
    installedAt: '2026-09-10T00:00:00Z',
    grantedPermissions: permissions,
    userRole: 'admin',
  }
}

describe('TappBridge KV permission gates', () => {
  let bridge: TappBridge
  let iframe: HTMLIFrameElement
  let reached: string[]
  let instance: TappInstance
  let seq: number

  beforeEach(() => {
    reached = []
    seq = 0
    instance = makeInstance()
    bridge = new TappBridge()
    const contentWindow = {} as Window
    iframe = { contentWindow } as HTMLIFrameElement
    bridge.initialize(iframe, instance, SESSION)
    bridge.attachSource()
    for (const action of [...READS, ...WRITES]) {
      bridge.registerHandler(action, async () => {
        reached.push(action)
        return { success: true, data: action }
      })
    }
  })

  afterEach(() => {
    bridge.destroy()
  })

  function captureResponses(): Array<Record<string, unknown>> {
    const responses: Array<Record<string, unknown>> = []
    const cw = iframe.contentWindow as Window & {
      postMessage: (msg: unknown, target: string) => void
    }
    cw.postMessage = (msg: unknown) => {
      responses.push(msg as Record<string, unknown>)
    }
    return responses
  }

  function dispatch(action: string, args: unknown[] = []): void {
    const [api, ...methodParts] = action.split('.')
    const event = {
      source: iframe.contentWindow,
      data: {
        type: 'request',
        id: `req-${action.replaceAll('.', '-')}-${++seq}`,
        action,
        payload: { api, method: methodParts.join('.'), args },
        timestamp: Date.now(),
        _sessionToken: SESSION,
      },
    } as MessageEvent
    const router = (
      TappBridge as unknown as {
        onSharedWindowMessage: (e: MessageEvent) => void
      }
    ).onSharedWindowMessage
    router(event)
  }

  async function lastPayload(responses: Array<Record<string, unknown>>) {
    await new Promise((resolve) => setTimeout(resolve, 0))
    const last = responses.at(-1)
    assert.ok(last, 'expected a bridge response')
    return last.payload as {
      success?: boolean
      code?: string
      data?: unknown
      error?: string
    }
  }

  it('denies every KV read and write when storage permissions are not granted', async () => {
    const responses = captureResponses()
    for (const action of [...READS, ...WRITES]) {
      dispatch(action, action.endsWith('.set') ? ['k', 1] : ['k'])
      const payload = await lastPayload(responses)
      assert.equal(payload.success, false, action)
      assert.equal(payload.code, 'PERMISSION_DENIED', action)
      assert.match(String(payload.error), /storage:/)
    }
    assert.deepEqual(reached, [])
  })

  it('allows KV reads with storage:read and still denies writes', async () => {
    instance.grantedPermissions = ['storage:read']
    const responses = captureResponses()
    for (const action of READS) {
      dispatch(action, ['k'])
      const payload = await lastPayload(responses)
      assert.equal(payload.success, true, action)
      assert.equal(payload.data, action)
    }
    for (const action of WRITES) {
      dispatch(action, ['k', 1])
      const payload = await lastPayload(responses)
      assert.equal(payload.success, false, action)
      assert.equal(payload.code, 'PERMISSION_DENIED', action)
    }
    assert.deepEqual(reached, Iterator.from(READS).toArray())
  })

  it('allows KV writes only after storage:write is granted', async () => {
    instance.grantedPermissions = ['storage:read', 'storage:write']
    const responses = captureResponses()
    for (const action of WRITES) {
      dispatch(action, ['k', 1])
      const payload = await lastPayload(responses)
      assert.equal(payload.success, true, action)
    }
    assert.deepEqual(reached, Iterator.from(WRITES).toArray())
  })
})
