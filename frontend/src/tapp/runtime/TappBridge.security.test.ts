/**
 * Bridge inbound message hardening (session token + event allowlist).
 *
 *   pnpm exec tsx --test src/tapp/runtime/TappBridge.security.test.ts
 */

import type { TappInstance } from '../types'
import assert from 'node:assert/strict'
import { afterEach, beforeEach, describe, it } from 'node:test'
import { TappBridge } from './TappBridge.ts'

const instance: TappInstance = {
  id: 'com.example.sec',
  manifest: {
    id: 'com.example.sec',
    name: 'Sec',
    version: '1.0.0',
    main: 'main.js',
    permissions: [],
    category: 'utility',
  },
  status: 'running',
  installedAt: '2026-07-18T00:00:00Z',
  grantedPermissions: [],
  userRole: 'admin',
}

describe('TappBridge session token + inbound event allowlist', () => {
  let bridge: TappBridge
  let iframe: HTMLIFrameElement
  let readyFired: number
  const SESSION = 'session-token-abc-xyz'

  beforeEach(() => {
    readyFired = 0
    bridge = new TappBridge()
    // Minimal iframe stub: contentWindow is a unique object identity.
    const contentWindow = {} as Window
    iframe = {
      contentWindow,
    } as HTMLIFrameElement
    bridge.initialize(iframe, instance, SESSION)
    // Force route registration (attachSource uses contentWindow)
    bridge.attachSource()
    // Explicit opt-in (static allowlist removed — host must register)
    bridge.allowSandboxEvent('tapp.ready')
    bridge.on('tapp.ready', () => {
      readyFired += 1
    })
    bridge.on('evil.ping', () => {
      readyFired += 100
    })
  })

  afterEach(() => {
    bridge.destroy()
  })

  function dispatchFromIframe(data: Record<string, unknown>): void {
    const event = {
      source: iframe.contentWindow,
      data,
    } as MessageEvent
    // Access private handle via shared listener path: simulate window message
    // by calling the same static router the production code uses.
    const router = (
      TappBridge as unknown as {
        onSharedWindowMessage: (e: MessageEvent) => void
      }
    ).onSharedWindowMessage
    if (typeof router === 'function') {
      router(event)
      return
    }
    // Fallback: invoke bound handleMessage if exposed for tests
    void (bridge as unknown as { handleMessage: (e: MessageEvent) => void })
      .handleMessage(event)
  }

  it('accepts tapp.ready when session token matches', async () => {
    dispatchFromIframe({
      type: 'event',
      id: 'ready-1',
      action: 'tapp.ready',
      payload: null,
      timestamp: Date.now(),
      _sessionToken: SESSION,
    })
    // handleMessage is async for requests; events are sync after validation
    await new Promise((r) => setTimeout(r, 0))
    assert.equal(readyFired, 1)
  })

  it('rejects events without session token', async () => {
    dispatchFromIframe({
      type: 'event',
      id: 'ready-2',
      action: 'tapp.ready',
      payload: null,
      timestamp: Date.now(),
    })
    await new Promise((r) => setTimeout(r, 0))
    assert.equal(readyFired, 0)
  })

  it('rejects events with wrong session token', async () => {
    dispatchFromIframe({
      type: 'event',
      id: 'ready-3',
      action: 'tapp.ready',
      payload: null,
      timestamp: Date.now(),
      _sessionToken: 'wrong-token',
    })
    await new Promise((r) => setTimeout(r, 0))
    assert.equal(readyFired, 0)
  })

  it('drops non-allowlisted sandbox events even with valid token', async () => {
    dispatchFromIframe({
      type: 'event',
      id: 'evil-1',
      action: 'evil.ping',
      payload: null,
      timestamp: Date.now(),
      _sessionToken: SESSION,
    })
    await new Promise((r) => setTimeout(r, 0))
    assert.equal(readyFired, 0)
  })

  it('ignores messages from a foreign source window', async () => {
    const foreign = {
      source: {} as Window,
      data: {
        type: 'event',
        id: 'ready-4',
        action: 'tapp.ready',
        payload: null,
        timestamp: Date.now(),
        _sessionToken: SESSION,
      },
    } as MessageEvent
    const router = (
      TappBridge as unknown as {
        onSharedWindowMessage: (e: MessageEvent) => void
      }
    ).onSharedWindowMessage
    router(foreign)
    await new Promise((r) => setTimeout(r, 0))
    assert.equal(readyFired, 0)
  })

  it('requires allowSandboxEvent before dispatching', async () => {
    bridge.disallowSandboxEvent('tapp.ready')
    assert.equal(bridge.isSandboxEventAllowed('tapp.ready'), false)
    dispatchFromIframe({
      type: 'event',
      id: 'ready-5',
      action: 'tapp.ready',
      payload: null,
      timestamp: Date.now(),
      _sessionToken: SESSION,
    })
    await new Promise((r) => setTimeout(r, 0))
    assert.equal(readyFired, 0)
    bridge.allowSandboxEvent('tapp.ready')
    dispatchFromIframe({
      type: 'event',
      id: 'ready-6',
      action: 'tapp.ready',
      payload: null,
      timestamp: Date.now(),
      _sessionToken: SESSION,
    })
    await new Promise((r) => setTimeout(r, 0))
    assert.equal(readyFired, 1)
  })

  it('rejects stale timestamps beyond skew window', async () => {
    dispatchFromIframe({
      type: 'event',
      id: 'ready-stale',
      action: 'tapp.ready',
      payload: null,
      timestamp: Date.now() - 10 * 60 * 1000,
      _sessionToken: SESSION,
    })
    await new Promise((r) => setTimeout(r, 0))
    assert.equal(readyFired, 0)
  })

  it('resolves bridge via contentWindow scan when source map is cold', async () => {
    // Simulate srcdoc race: message arrives before attachSource populated the map.
    const bridgesBySource = (
      TappBridge as unknown as {
        bridgesBySource: Map<MessageEventSource, TappBridge>
      }
    ).bridgesBySource
    bridgesBySource.clear()
    dispatchFromIframe({
      type: 'event',
      id: 'ready-race',
      action: 'tapp.ready',
      payload: null,
      timestamp: Date.now(),
      _sessionToken: SESSION,
    })
    await new Promise((r) => setTimeout(r, 0))
    assert.equal(readyFired, 1)
    // Map should be healed for subsequent messages
    assert.equal(bridgesBySource.get(iframe.contentWindow as MessageEventSource), bridge)
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

  it('while muted, answers request-shaped messages with BRIDGE_MUTED + retryAfter', async () => {
    const responses = captureResponses()

    // Force mute window
    ;(bridge as unknown as { mutedUntil: number }).mutedUntil =
      Date.now() + 60_000

    bridge.registerHandler('ui.getTheme', async () => ({
      success: true,
      data: 'dark',
    }))

    // Intentionally omit token/payload details — mute path is a cheap shape check
    dispatchFromIframe({
      type: 'request',
      id: 'req-muted-1',
      action: 'ui.getTheme',
      payload: { api: 'ui', method: 'getTheme', args: [] },
      timestamp: Date.now(),
    })
    await new Promise((r) => setTimeout(r, 0))

    assert.ok(responses.length >= 1, 'expected a response while muted')
    const last = responses[responses.length - 1]!
    assert.equal(last.type, 'response')
    assert.equal(last.id, 'req-muted-1')
    const payload = last.payload as {
      success?: boolean
      code?: string
      retryAfter?: number
    }
    assert.equal(payload.success, false)
    assert.equal(payload.code, 'BRIDGE_MUTED')
    assert.ok(
      typeof payload.retryAfter === 'number' && payload.retryAfter > 0,
      'expected positive retryAfter while muted',
    )
  })

  it('while muted, does not full-validate large payloads (still answers by id)', async () => {
    const responses = captureResponses()
    ;(bridge as unknown as { mutedUntil: number }).mutedUntil =
      Date.now() + 60_000

    // Huge payload would be expensive under full validateMessage; mute path
    // must only inspect request shape (type + id).
    const huge = 'x'.repeat(2 * 1024 * 1024)
    dispatchFromIframe({
      type: 'request',
      id: 'req-muted-huge',
      action: 'storage.set',
      payload: { api: 'storage', method: 'set', args: [huge] },
      timestamp: Date.now(),
      _sessionToken: SESSION,
    })
    await new Promise((r) => setTimeout(r, 0))

    assert.ok(responses.length >= 1)
    const last = responses[responses.length - 1]!
    assert.equal(last.id, 'req-muted-huge')
    assert.equal(
      (last.payload as { code?: string }).code,
      'BRIDGE_MUTED',
    )
  })

  it('while muted, drops events without hanging requests', async () => {
    ;(bridge as unknown as { mutedUntil: number }).mutedUntil =
      Date.now() + 60_000
    const before = readyFired
    dispatchFromIframe({
      type: 'event',
      id: 'ready-muted',
      action: 'tapp.ready',
      payload: null,
      timestamp: Date.now(),
      _sessionToken: SESSION,
    })
    await new Promise((r) => setTimeout(r, 0))
    assert.equal(readyFired, before)
  })

  it('answers duplicate request ids with DUPLICATE_REQUEST', async () => {
    const responses = captureResponses()
    bridge.registerHandler('ui.getTheme', async () => ({
      success: true,
      data: 'dark',
    }))

    const msg = {
      type: 'request',
      id: 'req-dup-1',
      action: 'ui.getTheme',
      payload: { api: 'ui', method: 'getTheme', args: [] },
      timestamp: Date.now(),
      _sessionToken: SESSION,
    }
    dispatchFromIframe(msg)
    await new Promise((r) => setTimeout(r, 0))
    dispatchFromIframe(msg)
    await new Promise((r) => setTimeout(r, 0))

    const dup = responses.find(
      (r) =>
        r.id === 'req-dup-1' &&
        (r.payload as { code?: string })?.code === 'DUPLICATE_REQUEST',
    )
    assert.ok(dup, 'expected DUPLICATE_REQUEST response for replayed id')
    assert.equal((dup!.payload as { success?: boolean }).success, false)
  })

  it('forwards retryAfter on RATE_LIMITED (bridge inbound soft limit)', async () => {
    const responses = captureResponses()
    const inboundRate = (
      bridge as unknown as {
        inboundRate: { tryTake: () => boolean; retryAfterMs: () => number }
      }
    ).inboundRate
    const original = inboundRate.tryTake.bind(inboundRate)
    inboundRate.tryTake = () => false
    ;(
      inboundRate as { retryAfterMs: () => number }
    ).retryAfterMs = () => 1234

    dispatchFromIframe({
      type: 'request',
      id: 'req-rate-1',
      action: 'ui.getTheme',
      payload: { api: 'ui', method: 'getTheme', args: [] },
      timestamp: Date.now(),
      _sessionToken: SESSION,
    })
    await new Promise((r) => setTimeout(r, 0))

    inboundRate.tryTake = original

    const last = responses[responses.length - 1]!
    assert.equal(last.id, 'req-rate-1')
    const payload = last.payload as {
      code?: string
      retryAfter?: number
      success?: boolean
    }
    assert.equal(payload.success, false)
    assert.equal(payload.code, 'RATE_LIMITED')
    assert.equal(payload.retryAfter, 1234)
  })
})
