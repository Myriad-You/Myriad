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
    assert.match(sdk, /addEventListener\('settingsChanged'/)
    assert.match(sdk, /addEventListener\('privateChanged'/)
    assert.match(sdk, /eventListeners.get\(message.action\)/)
    assert.match(sdk, /lifecycle:\s*\{/)
    assert.match(sdk, /sendRequest\('persona'/)
    assert.match(sdk, /\bai:\s*\{/)
    assert.doesNotMatch(sdk, /\bmodel3d:\s*\{/)
    assert.match(sdk, /\bmedia:\s*\{/)
    assert.match(sdk, /\banalytics:\s*\{/)
    assert.match(sdk, /_denied\(/)
    assert.match(sdk, /Missing permission/)
    assert.match(
      sdk,
      /ai:generate, ai:analyze, ai:chat, ai:image, ai:search/,
    )
    assert.doesNotMatch(
      sdk,
      /create: _denied\('ai:generate'\)/,
    )
    // 无权限时不应出现完整 AI subscribe / media sendRequest。
    assert.doesNotMatch(sdk, /sendRequest\('ai'/)
    assert.doesNotMatch(sdk, /sendRequest\('media'/)
    assert.doesNotMatch(sdk, /sendRequest\('scheduler'/)
  })

  it('includes live analytics when analytics:read is granted', () => {
    const sdk = generateWidgetSDK(makeInstance(['analytics:read']), 'tok')
    assert.match(sdk, /analytics:\s*\{/)
    assert.match(sdk, /getVisitorCard/)
    assert.match(sdk, /sendRequest\('analytics'/)
    assert.doesNotMatch(sdk, /sendRequest\('media'/)
    assert.match(sdk, /_denied\('media:/)
  })

  it('evaluates denied AI stubs while KV methods still postMessage', async () => {
    const sdk = generateWidgetSDK(makeInstance([]), 'tok')
    const posted: Array<Record<string, unknown>> = []
    const sandboxWindow: Record<string, unknown> = {
      parent: {
        postMessage(message: Record<string, unknown>) {
          posted.push(message)
        },
      },
      addEventListener() {},
      _TAPP_I18N: {},
      _TAPP_LOCALE: 'en-US',
    }
    const sandboxDocument = {
      readyState: 'complete',
      addEventListener() {},
      createElement: () => ({ style: {}, appendChild() {} }),
      body: {
        style: {},
        classList: { toggle() {} },
        offsetHeight: 0,
      },
      documentElement: {
        style: { setProperty() {} },
        classList: { toggle() {} },
        lang: 'en-US',
      },
    }
    // eslint-disable-next-line no-new-func -- isolated widget SDK eval
    const run = new Function(
      'window',
      'document',
      'crypto',
      'setTimeout',
      'URL',
      'Blob',
      'atob',
      sdk,
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
    const tapp = sandboxWindow.Tapp as {
      ai: { tasks: { create: (request: unknown) => Promise<unknown> } }
      private: { get: (key: string) => Promise<unknown> }
    }
    await assert.rejects(
      () => tapp.ai.tasks.create({}),
      /Missing permission/,
    )
    assert.equal(posted.length, 0)
    const pending = tapp.private.get('token')
    assert.equal(posted[0]?.action, 'private.get')
    pending.catch(() => {})
  })

  it('replaces session token without rebuilding the cached body', () => {
    const instance = makeInstance(['analytics:read'])
    const a = generateWidgetSDK(instance, 'tok-a')
    const b = generateWidgetSDK(instance, 'tok-b')
    assert.match(a, /tok-a/)
    assert.match(b, /tok-b/)
    assert.doesNotMatch(a, /tok-b/)
    assert.doesNotMatch(b, /tok-a/)
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
    // Shared message-loop bootstrap is the same, so the delta is the live namespaces.
    assert.ok(
      full.length > minimal.length + 4000,
      `expected full (${full.length}) to exceed minimal (${minimal.length}) by ≥4KB`,
    )
  })
})
