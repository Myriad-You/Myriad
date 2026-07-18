/* eslint-disable test/no-import-node-test -- node:test is the repository test runner */

import type { TappInstance } from '../../types'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { PERMISSION_MAP } from '../permissionConfig.ts'
import { generateFullSDK } from './sdkGenerator.ts'

const instance: TappInstance = {
  id: 'com.example.profile-test',
  manifest: {
    id: 'com.example.profile-test',
    name: 'Profile Test',
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
  // The SDK is generated JavaScript; executing it is the contract under test.
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
      [...sdk.matchAll(/sendRequest\('([^']+)',\s*'([^']+)'/g)].map(
        ([, namespace, operation]) => `${namespace}.${operation}`,
      ),
    )

    const permissionActions = new Set(PERMISSION_MAP.keys())
    assert.deepEqual(
      [...sdkActions].filter(action => !permissionActions.has(action)),
      [],
    )
    assert.deepEqual(
      [...permissionActions].filter(action => !sdkActions.has(action)).sort(),
      ['widget.instanceSettings.update', 'widget.invalidate'],
      'only Widget-SDK-specific actions may be absent from the Page SDK',
    )
  })

  it('keeps the full Page control surface', () => {
    const tapp = evaluateSdk('page')
    assert.ok(tapp.widget)
    assert.ok(tapp.tappList)
    assert.ok(tapp.component)
    assert.ok(tapp.dynamicContent)
    assert.equal(typeof (tapp.ui as Record<string, unknown>).confirm, 'function')
  })

  it('keeps background APIs but removes visible/control-plane APIs in headless core', () => {
    const tapp = evaluateSdk('headless')
    assert.ok(tapp.storage)
    assert.ok(tapp.scheduler)
    assert.ok(tapp.event)
    assert.ok(tapp.federation)
    assert.equal(tapp.widget, undefined)
    assert.equal(tapp.tappList, undefined)
    assert.equal(tapp.component, undefined)
    assert.equal(tapp.dynamicContent, undefined)
    assert.equal(tapp.dom, undefined)
    assert.equal(tapp.file, undefined)
    const ui = tapp.ui as Record<string, unknown>
    assert.equal(typeof ui.showNotification, 'function')
    assert.equal(ui.confirm, undefined)
    assert.equal(ui.fullscreen, undefined)
  })
})
