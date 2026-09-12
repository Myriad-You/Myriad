import type { AITaskRequest, TappInstance, TappMessage } from '../../types'
import type { TappBridge } from '../TappBridge'
import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import vm from 'node:vm'
import { registerAIHandlers } from './handlers/aiHandlers.ts'
import { generateFullSDK, generateWidgetSDK } from './sdkGenerator.ts'

const originalFetch = globalThis.fetch
const originalStorage = Object.getOwnPropertyDescriptor(globalThis, 'sessionStorage')
type MessageHandler = Parameters<TappBridge['registerHandler']>[1]

afterEach(() => {
  globalThis.fetch = originalFetch
  if (originalStorage) Object.defineProperty(globalThis, 'sessionStorage', originalStorage)
  else Reflect.deleteProperty(globalThis, 'sessionStorage')
})

function sandbox(profile: 'page' | 'widget' | 'headless') {
  const handlers = new Map<string, MessageHandler>()
  const bridge = {
    registerHandler: (action: string, handler: MessageHandler) => handlers.set(action, handler),
    getRuntimeGrant: async () => 'host-only-runtime-grant',
  } as unknown as TappBridge
  registerAIHandlers(bridge)
  const listeners = new Map<string, (event: unknown) => void>()
  const messages: TappMessage[] = []
  const host = {
    postMessage: async (message: TappMessage) => {
      messages.push(message)
      const handler = handlers.get(message.action)
      assert.ok(handler)
      const payload = await handler(message)
      listeners.get('message')?.({
        source: host,
        data: { type: 'response', id: message.id, payload },
      })
    },
  }
  const windowLike = {
    parent: host,
    addEventListener: (name: string, listener: (event: unknown) => void) => listeners.set(name, listener),
  }
  const instance = {
    id: 'com.example.image-test',
    manifest: { id: 'com.example.image-test', name: 'Image Test', version: '1.0.0' },
    grantedPermissions: ['ai:image'],
  } as TappInstance
  const source = profile === 'widget'
    ? generateWidgetSDK(instance, 'session-token')
    : generateFullSDK(instance, 'session-token', profile)
  vm.runInNewContext(source, Object.assign(windowLike, {
    window: windowLike,
    setTimeout: () => 0,
    clearTimeout: () => {},
    console: { log: () => {} },
  }))
  const sdk = (windowLike as unknown as {
    Tapp: { ai: { tasks: { create: (request: AITaskRequest) => Promise<unknown> } } }
  }).Tapp
  const token = `v1.${'a'.repeat(100)}.${'b'.repeat(43)}`
  Object.defineProperty(globalThis, 'sessionStorage', {
    configurable: true,
    value: { getItem: (key: string) => key === 'csrf_token' ? token : String(Date.now() + 60_000) },
  })
  return { create: sdk.ai.tasks.create, messages }
}

describe('image references through the sandbox and host transport', () => {
  for (const profile of ['page', 'widget', 'headless'] as const) {
    it(`${profile} preserves large reference inputs and keeps the grant in HTTP headers`, async () => {
      const { create, messages } = sandbox(profile)
      const request: AITaskRequest = {
        version: 2,
        operation: 'image',
        input: {
          prompt: 'Use the first subject and the second style',
          referenceImages: [
            `data:image/png;base64,${'A'.repeat(300_000)}`,
            `/api/brew/image-cache/aa/${'a'.repeat(64)}.png`,
          ],
        },
        output: { format: 'image' },
      }
      globalThis.fetch = async (url, options) => {
        assert.equal(String(url), '/api/tapp/ai/v2/tasks')
        assert.equal(options?.method, 'POST')
        assert.deepEqual(JSON.parse(String(options?.body)), request)
        assert.equal(new Headers(options?.headers).get('X-Tapp-Runtime-Grant'), 'host-only-runtime-grant')
        return Response.json({ taskId: 'image-task', status: 'queued' }, { status: 202 })
      }
      assert.deepEqual(await create(request), { taskId: 'image-task', status: 'queued' })
      assert.equal(messages.length, 1)
      assert.equal(JSON.stringify(messages).includes('host-only-runtime-grant'), false)
    })
  }

  it('preserves reference validation error codes for callers', async () => {
    const { create } = sandbox('page')
    globalThis.fetch = async () => Response.json({
      error: 'referenceImages accepts at most 4 images',
      code: 'AI_IMAGE_REFERENCE_LIMIT',
    }, { status: 413 })
    await assert.rejects(create({
      version: 2,
      operation: 'image',
      input: { prompt: 'draw', referenceImages: Array.from({ length: 5 }, () => 'data:image/png;base64,AA==') },
    }), (error: unknown) => {
      assert.equal((error as { code?: string }).code, 'AI_IMAGE_REFERENCE_LIMIT')
      return true
    })
  })
})
