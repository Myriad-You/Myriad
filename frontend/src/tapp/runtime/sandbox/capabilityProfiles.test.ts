import type { TappInstance } from '../../types'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { PERMISSION_MAP } from '../permissionConfig.ts'
import { TappBridge } from '../TappBridge.ts'
import {
  applySandboxCapabilityProfile,
  HEADLESS_DENIED_ACTIONS,
} from './capabilityProfiles.ts'
import { generateFullSDK } from './sdkGenerator.ts'

const instance: TappInstance = {
  id: 'com.example.profile-test',
  manifest: {
    id: 'com.example.profile-test',
    name: 'Profile Test',
    version: '1.0.0',
    core: { entry: 'core.js' },
    permissions: [],
    category: 'utility',
  },
  status: 'running',
  installedAt: '2026-07-18T00:00:00Z',
  grantedPermissions: [],
  userRole: 'admin',
}

function evaluateSdk(profile: 'page' | 'headless'): Record<string, unknown> {
  const sandboxWindow: Record<string, any> = {
    _TAPP_I18N: {},
    _TAPP_LOCALE: 'en-US',
    addEventListener: () => undefined,
    parent: { postMessage: () => undefined },
  }
  const sandboxDocument = {
    addEventListener: () => undefined,
    createElement: () => ({ style: {}, appendChild: () => undefined }),
  }
  // eslint-disable-next-line no-new-func -- isolated test sandbox
  const run = new Function(
    'window',
    'document',
    'crypto',
    'setTimeout',
    'URL',
    'Blob',
    'atob',
    generateFullSDK(instance, 'session-token', profile),
  )
  run(
    sandboxWindow,
    sandboxDocument,
    globalThis.crypto,
    () => 0,
    URL,
    Blob,
    globalThis.atob,
  )
  return sandboxWindow.Tapp as Record<string, unknown>
}

describe('sandbox capability profiles', () => {
  it('keeps every generated SDK action governed by PERMISSION_MAP', () => {
    const sdk = generateFullSDK(instance, 'session-token', 'page')
    const sdkActions = new Set(
      Iterator.from(sdk.matchAll(/sendRequest\(\s*'([^']+)',\s*'([^']+)'/g)).map(
        ([, namespace, operation]) => `${namespace}.${operation}`,
      ),
    )

    const permissionActions = new Set(PERMISSION_MAP.keys())
    assert.deepEqual(
      Iterator.from(sdkActions.difference(permissionActions)).toArray(),
      [],
    )
    assert.deepEqual(
      Iterator.from(permissionActions.difference(sdkActions)).toArray().toSorted(),
      ['widget.instanceSettings.update', 'widget.invalidate'],
      'only Widget-SDK-specific actions may be absent from the Page SDK',
    )
  })

  it('keeps the full Page control surface', () => {
    const tapp = evaluateSdk('page')
    assert.ok(tapp.widget)
    assert.ok(tapp.private)
    assert.equal(
      typeof (tapp.settings as Record<string, unknown>).onChanged,
      'function',
    )
    assert.equal(
      typeof (tapp.widget as Record<string, unknown>).invalidate,
      'function',
    )
    assert.ok(tapp.tappList)
    assert.ok(tapp.component)
    assert.ok(tapp.dynamicContent)
    assert.ok(tapp.model3d)
    assert.ok(tapp.persona)
    assert.equal(typeof (tapp.ui as Record<string, unknown>).confirm, 'function')
    assert.equal(
      (tapp.ui as Record<string, unknown>).requestFullscreen,
      undefined,
    )
    assert.equal(
      typeof ((tapp.ui as Record<string, unknown>).fullscreen as Record<string, unknown>)
        .request,
      'function',
    )
  })

  it('exposes targeted invalidate on Page and headless, not Widget self-invalidate', () => {
    const pageSdk = generateFullSDK(instance, 'session-token', 'page')
    const headlessSdk = generateFullSDK(instance, 'session-token', 'headless')
    assert.match(pageSdk, /sendRequest\('widget', 'invalidateTarget'/)
    assert.match(headlessSdk, /sendRequest\('widget', 'invalidateTarget'/)
    assert.doesNotMatch(pageSdk, /sendRequest\('widget', 'invalidate'(?!Target)/)
    assert.doesNotMatch(
      headlessSdk,
      /sendRequest\('widget', 'invalidate'(?!Target)/,
    )
  })

  it('freezes KV namespaces and still lets Page widgets/pages register', () => {
    const tapp = evaluateSdk('page')
    for (const name of ['storage', 'shared', 'private', 'settings']) {
      assert.equal(Object.isFrozen(tapp[name]), true, name)
    }
    const widgets = tapp.widgets as Record<string, unknown>
    widgets.demo = { render() {} }
    assert.equal(typeof (widgets.demo as { render: unknown }).render, 'function')
    assert.match(
      generateFullSDK(instance, 'session-token', 'page'),
      /\)\(window\.Tapp\)/,
    )
  })

  it('keeps background APIs but removes visible/control-plane APIs in headless core', () => {
    const tapp = evaluateSdk('headless')
    assert.ok(tapp.storage)
    assert.ok(tapp.private)
    assert.equal(
      typeof (tapp.settings as Record<string, unknown>).onChanged,
      'function',
    )
    assert.ok(tapp.scheduler)
    assert.ok(tapp.event)
    assert.ok(tapp.federation)
    assert.ok(tapp.persona)
    const widget = tapp.widget as Record<string, unknown>
    assert.equal(typeof widget.invalidate, 'function')
    assert.equal(widget.register, undefined)
    assert.equal(tapp.tappList, undefined)
    assert.equal(tapp.component, undefined)
    assert.equal(tapp.dynamicContent, undefined)
    assert.equal(tapp.dom, undefined)
    assert.equal(tapp.file, undefined)
    assert.equal(tapp.model3d, undefined)
    const ui = tapp.ui as Record<string, unknown>
    assert.equal(typeof ui.showNotification, 'function')
    assert.equal(ui.confirm, undefined)
    assert.equal(ui.fullscreen, undefined)
  })

  it('does not emit headless-denied sendRequest in the headless SDK source', () => {
    const sdk = generateFullSDK(instance, 'session-token', 'headless')
    for (const action of HEADLESS_DENIED_ACTIONS) {
      const dot = action.indexOf('.')
      const api = action.slice(0, dot)
      const method = action.slice(dot + 1)
      assert.equal(
        sdk.includes(`sendRequest('${api}', '${method}'`),
        false,
        action,
      )
    }
    assert.match(sdk, /sendRequest\('widget', 'invalidateTarget'/)
    assert.match(sdk, /sendRequest\('federation', 'getFeed'/)
    assert.match(sdk, /sendRequest\('ui', 'showNotification'/)
  })

  it('unregisters only HEADLESS_DENIED_ACTIONS and keeps KV handlers', () => {
    const bridge = new TappBridge()
    const handlers = (
      bridge as unknown as { messageHandlers: Map<string, unknown> }
    ).messageHandlers
    const kv = [
      'storage.get',
      'shared.get',
      'private.get',
      'settings.get',
      'private.set',
    ]
    for (const action of [...HEADLESS_DENIED_ACTIONS, ...kv]) {
      bridge.registerHandler(action, async () => ({ success: true, data: null }))
    }
    applySandboxCapabilityProfile(bridge, 'page')
    assert.equal(handlers.has('ui.confirm'), true)
    applySandboxCapabilityProfile(bridge, 'headless')
    for (const action of HEADLESS_DENIED_ACTIONS) {
      assert.equal(handlers.has(action), false, action)
    }
    for (const action of kv) {
      assert.equal(handlers.has(action), true, action)
    }
    bridge.destroy()
  })
})
