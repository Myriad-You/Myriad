import type { TappInstance } from '../../types'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { generateWidgetSDK } from './sdkGenerator.ts'

const baseInstance: TappInstance = {
  id: 'com.example.widget-cache',
  manifest: {
    id: 'com.example.widget-cache',
    name: 'Widget Cache',
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

describe('generateWidgetSDK session token isolation', () => {
  it('embeds distinct session tokens per call without sharing across sandboxes', () => {
    const a = generateWidgetSDK(baseInstance, 'token-aaa-111')
    const b = generateWidgetSDK(baseInstance, 'token-bbb-222')
    assert.match(a, /token-aaa-111/)
    assert.match(b, /token-bbb-222/)
    assert.doesNotMatch(a, /token-bbb-222/)
    assert.doesNotMatch(b, /token-aaa-111/)
    // Placeholder must never leak into the final SDK source
    assert.doesNotMatch(a, /__TAPP_WIDGET_SESSION_TOKEN__/)
    assert.doesNotMatch(b, /__TAPP_WIDGET_SESSION_TOKEN__/)
  })

  it('still freezes Tapp and exposes storage/lifecycle for the hot path', () => {
    const sdk = generateWidgetSDK(baseInstance, 'tok')
    assert.match(sdk, /Object\.freeze\(Tapp\)/)
    assert.match(sdk, /storage:\s*\{/)
    assert.match(sdk, /lifecycle:\s*\{/)
  })
})
