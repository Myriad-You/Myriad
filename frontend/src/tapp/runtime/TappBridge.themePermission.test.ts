/**
 * TappBridge permission gating for one-shot theme reads.
 *
 * `ui.getTheme` / `ui.getPrimaryColor` require the granted `ui:theme:read`
 * permission. A subscription-only grant (`ui:theme:subscribe`) must not
 * unlock either read — the two halves are independent.
 *
 *   pnpm exec tsx --test src/tapp/runtime/TappBridge.themePermission.test.ts
 */

import type { TappInstance } from '../types'
import assert from 'node:assert/strict'
import { afterEach, beforeEach, describe, it } from 'node:test'
import { TappBridge } from './TappBridge.ts'

function instanceWith(grantedPermissions: string[]): TappInstance {
  return {
    id: 'com.example.theme-bridge',
    manifest: {
      id: 'com.example.theme-bridge',
      name: 'Theme Bridge',
      version: '1.0.0',
      main: 'main.js',
      permissions: grantedPermissions,
      category: 'utility',
    },
    status: 'running',
    installedAt: '2026-08-14T00:00:00Z',
    grantedPermissions,
    userRole: 'admin',
  }
}

describe('TappBridge ui:theme:read gating for one-shot theme reads', () => {
  let bridge: TappBridge
  let iframe: HTMLIFrameElement
  let instance: TappInstance
  const SESSION = 'session-token-theme-read'

  beforeEach(() => {
    instance = instanceWith(['ui:theme:read'])
    bridge = new TappBridge()
    const contentWindow = {} as Window
    iframe = { contentWindow } as HTMLIFrameElement
    bridge.initialize(iframe, instance, SESSION)
    bridge.attachSource()
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

  function dispatchRequest(
    action: string,
    method: string,
  ): void {
    const event = {
      source: iframe.contentWindow,
      data: {
        type: 'request',
        id: `req-${method}`,
        action,
        payload: { api: action.split('.')[0], method, args: [] },
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

  it('allows ui.getTheme when ui:theme:read is granted', async () => {
    const responses = captureResponses()
    bridge.registerHandler('ui.getTheme', async () => ({
      success: true,
      data: 'dark',
    }))

    dispatchRequest('ui.getTheme', 'getTheme')
    await new Promise((r) => setTimeout(r, 0))

    assert.equal(responses.length, 1)
    assert.equal(responses[0]!.type, 'response')
    assert.equal((responses[0]!.payload as { success?: boolean }).success, true)
  })

  it('allows ui.getPrimaryColor when ui:theme:read is granted', async () => {
    const responses = captureResponses()
    bridge.registerHandler('ui.getPrimaryColor', async () => ({
      success: true,
      data: '#6366f1',
    }))

    dispatchRequest('ui.getPrimaryColor', 'getPrimaryColor')
    await new Promise((r) => setTimeout(r, 0))

    assert.equal(responses.length, 1)
    assert.equal(responses[0]!.type, 'response')
    assert.equal((responses[0]!.payload as { success?: boolean }).success, true)
  })

  it('rejects ui.getTheme with a subscription-only grant', async () => {
    instance.grantedPermissions = ['ui:theme:subscribe']
    const responses = captureResponses()
    // Handler must not even be reachable — the permission gate rejects first.
    bridge.registerHandler('ui.getTheme', async () => ({
      success: true,
      data: 'dark',
    }))

    dispatchRequest('ui.getTheme', 'getTheme')
    await new Promise((r) => setTimeout(r, 0))

    assert.equal(responses.length, 1)
    const payload = responses[0]!.payload as {
      success?: boolean
      code?: string
      error?: string
    }
    assert.equal(payload.success, false)
    assert.equal(payload.code, 'PERMISSION_DENIED')
    assert.match(payload.error ?? '', /Missing permission: ui:theme:read/)
  })

  it('rejects ui.getPrimaryColor with a subscription-only grant', async () => {
    instance.grantedPermissions = ['ui:theme:subscribe']
    const responses = captureResponses()
    bridge.registerHandler('ui.getPrimaryColor', async () => ({
      success: true,
      data: '#6366f1',
    }))

    dispatchRequest('ui.getPrimaryColor', 'getPrimaryColor')
    await new Promise((r) => setTimeout(r, 0))

    assert.equal(responses.length, 1)
    const payload = responses[0]!.payload as {
      success?: boolean
      code?: string
      error?: string
    }
    assert.equal(payload.success, false)
    assert.equal(payload.code, 'PERMISSION_DENIED')
    assert.match(payload.error ?? '', /Missing permission: ui:theme:read/)
  })
})
