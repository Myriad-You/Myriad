/**
 * Permission-trimmed Widget SDK surface.
 *
 *   pnpm exec tsx --test src/tapp/runtime/sandbox/widgetSdkTrim.test.ts
 */

import type { TappInstance } from '../../types'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  generateWidgetSDK,
  resolveWidgetSdkCaps,
} from './sdkGenerator.ts'

function makeInstance(permissions: string[]): TappInstance {
  return {
    id: 'com.example.trim',
    manifest: {
      id: 'com.example.trim',
      name: 'Trim Test',
      version: '1.0.0',
      core: { entry: 'core.js' },
      permissions: permissions as never[],
      category: 'utility',
    },
    status: 'running',
    installedAt: '2026-07-18T00:00:00Z',
    grantedPermissions: permissions as never[],
    userRole: 'admin',
  }
}

describe('resolveWidgetSdkCaps', () => {
  it('defaults all optional namespaces off', () => {
    const caps = resolveWidgetSdkCaps([])
    assert.equal(caps.ai, false)
    assert.equal(caps.media, false)
    assert.equal(caps.analytics, false)
  })

  it('enables namespaces from granted permissions', () => {
    const caps = resolveWidgetSdkCaps([
      'analytics:read',
      'media:read',
      'ai:chat',
    ])
    assert.equal(caps.analytics, true)
    assert.equal(caps.media, true)
    assert.equal(caps.ai, true)
    assert.equal(caps.platform, false)
  })
})

describe('generateWidgetSDK permission trim', () => {
  it('keeps namespace shape with denied stubs when permissions are empty', () => {
    const sdk = generateWidgetSDK(makeInstance([]), 'tok')
    assert.match(sdk, /storage:\s*\{/)
    assert.match(sdk, /lifecycle:\s*\{/)
    // Shape preserved for DX; heavy sendRequest bodies omitted
    assert.match(sdk, /\bai:\s*\{/)
    assert.match(sdk, /\bmedia:\s*\{/)
    assert.match(sdk, /\banalytics:\s*\{/)
    assert.match(sdk, /_denied\(/)
    assert.match(sdk, /Missing permission/)
    // Full AI subscribe / media sendRequest plumbing should not be present without perms
    assert.doesNotMatch(sdk, /sendRequest\('ai'/)
    assert.doesNotMatch(sdk, /sendRequest\('media'/)
    assert.doesNotMatch(sdk, /sendRequest\('scheduler'/)
  })

  it('includes live analytics when analytics:read is granted', () => {
    const sdk = generateWidgetSDK(makeInstance(['analytics:read']), 'tok')
    assert.match(sdk, /analytics:\s*\{/)
    assert.match(sdk, /getVisitorCard/)
    assert.match(sdk, /sendRequest\('analytics'/)
    // media still stubbed (no live sendRequest)
    assert.doesNotMatch(sdk, /sendRequest\('media'/)
    assert.match(sdk, /_denied\('media:/)
  })

  it('minimal SDK is meaningfully smaller than full-permission SDK', () => {
    const minimal = generateWidgetSDK(makeInstance([]), 'tok')
    const full = generateWidgetSDK(
      makeInstance([
        'ai:chat',
        'ai:generate',
        'platform:read',
        'analytics:read',
        'report:read',
        'media:read',
        'media:control',
        'speech:tts',
        'event:publish',
        'component:agent',
        'scheduler:register',
      ]),
      'tok',
    )
    // Full media/ai/scheduler bodies are large; stubs stay compact.
    assert.ok(
      full.length > minimal.length + 4000,
      `expected full (${full.length}) to exceed minimal (${minimal.length}) by ≥4KB`,
    )
    assert.ok(
      full.length > minimal.length * 1.15,
      `expected full (${full.length}) ≥ 1.15× minimal (${minimal.length})`,
    )
  })
})
