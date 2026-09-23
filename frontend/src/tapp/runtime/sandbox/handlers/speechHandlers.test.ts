import type { TappInstance, TappMessage } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { registerSpeechHandlers } from './advancedHandlers.ts'

const originalFetch = globalThis.fetch
const originalSessionStorage = globalThis.sessionStorage
const GRANT = 'speech-runtime-grant'
const calls: Array<{
  url: string
  method: string
  grant?: string
  body?: unknown
}> = []

afterEach(() => {
  globalThis.fetch = originalFetch
  globalThis.sessionStorage = originalSessionStorage
  calls.length = 0
})

function installSessionStorage() {
  const store = new Map<string, string>()
  globalThis.sessionStorage = {
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

  registerHandler(
    action: string,
    handler: (message: TappMessage) => Promise<unknown>,
  ) {
    this.handlers.set(action, handler)
  }

  async getRuntimeGrant() {
    return GRANT
  }

  async hostAttributionHeaders() {
    return { 'X-Tapp-Runtime-Grant': GRANT }
  }
}

const instance: TappInstance = {
  id: 'com.example.speech',
  manifest: {
    id: 'com.example.speech',
    name: 'Speech',
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
    id: 'speech-1',
    action,
    payload: { args },
    timestamp: Date.now(),
  })
}

function mockSpeech(body: unknown) {
  installSessionStorage()
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input)
    if (url.includes('/api/csrf-token')) {
      return Response.json(({ csrf_token: null }), { status: 200 })
    }
    const headers = (init?.headers || {}) as Record<string, string>
    calls.push({
      url,
      method: (init?.method || 'GET').toUpperCase(),
      grant: headers['X-Tapp-Runtime-Grant'],
      body: typeof init?.body === 'string' ? JSON.parse(init.body) : undefined,
    })
    return Response.json(body, { status: 200 })
  }) as typeof fetch
}

describe('registerSpeechHandlers', { concurrency: false }, () => {
  it('rejects empty tts/asr without calling the host', async () => {
    mockSpeech({ success: true })
    const bridge = new FakeBridge()
    registerSpeechHandlers(bridge as unknown as TappBridge, instance)
    assert.deepEqual(await invoke(bridge, 'speech.tts', [{}]), {
      success: false,
      error: 'Text is required',
    })
    assert.deepEqual(await invoke(bridge, 'speech.asr', [{}]), {
      success: false,
      error: 'Audio data is required',
    })
    assert.equal(calls.length, 0)
  })

  it('sends TTS with host attribution headers, not a sandbox secret', async () => {
    mockSpeech({
      success: true,
      audio: 'UklGRg==',
      session_id: 's1',
      cached: false,
    })
    const bridge = new FakeBridge()
    registerSpeechHandlers(bridge as unknown as TappBridge, instance)
    const result = await invoke(bridge, 'speech.tts', [{ text: '你好' }])
    assert.deepEqual(result, {
      success: true,
      data: { audio: 'UklGRg==', session_id: 's1', cached: false },
      error: undefined,
    })
    assert.equal(calls.length, 1)
    assert.equal(calls[0]?.method, 'POST')
    assert.match(String(calls[0]?.url), /\/api\/speech\/tts/)
    assert.equal(calls[0]?.grant, GRANT)
    assert.deepEqual(calls[0]?.body, { text: '你好' })
  })
})
