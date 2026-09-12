import type { TappInstance, TappMessage } from '../types'
import type { TappBridge } from './TappBridge.ts'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { createContext, runInContext } from 'node:vm'
import { registerPlaygroundPreviewHandlers } from './sandbox/handlers/playgroundPreviewHandlers.ts'
import { generateFullSDK, generateWidgetSDK } from './sandbox/sdkGenerator.ts'
import {
  emitHostSettingsChange,
  emitTappPrivateChange,
  emitTappSettingsChange,
  emitTappStorageChange,
  HOST_SETTINGS_WRITE_SOURCE,
  isForeignTappKvChange,
  onTappPrivateChange,
  onTappSettingsChange,
  onTappStorageChange,
} from './WidgetRuntimeSignals.ts'

const instance: TappInstance = {
  id: 'com.example.app',
  manifest: {
    id: 'com.example.app',
    name: 'KV Test',
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

function evaluateSdk(source: string): {
  tapp: Record<string, any>
  dispatchHostEvent: (action: string, payload: unknown) => void
} {
  const host = { postMessage() {} }
  const messageListeners: Array<(event: unknown) => void> = []
  const sandboxWindow: Record<string, any> = {
    _TAPP_I18N: {},
    _TAPP_LOCALE: 'en-US',
    parent: host,
    addEventListener: (type: string, handler: (event: unknown) => void) => {
      if (type === 'message') messageListeners.push(handler)
    },
    crypto: globalThis.crypto,
    setTimeout: () => 0,
    URL,
    Blob,
    atob: globalThis.atob,
    console,
  }
  sandboxWindow.window = sandboxWindow
  sandboxWindow.document = {
    addEventListener: () => undefined,
    createElement: () => ({ style: {}, appendChild: () => undefined }),
  }
  runInContext(source, createContext(sandboxWindow))
  return {
    tapp: sandboxWindow.Tapp,
    dispatchHostEvent(action, payload) {
      for (const listener of messageListeners) {
        listener({
          source: host,
          data: { type: 'event', action, payload },
        })
      }
    },
  }
}

describe('isForeignTappKvChange', () => {
  const writer = { id: 'bridge-a' }
  const other = { id: 'bridge-b' }
  const change = {
    tappId: 'com.example.app',
    key: 'compact',
    operation: 'set' as const,
    source: writer,
  }

  it('drops the writing sandbox and missing bridges', () => {
    assert.equal(isForeignTappKvChange(change, 'com.example.app', writer), false)
    assert.equal(isForeignTappKvChange(change, 'com.example.app', null), false)
    assert.equal(isForeignTappKvChange(change, 'com.other.app', other), false)
  })

  it('relays to another sandbox of the same Tapp', () => {
    assert.equal(isForeignTappKvChange(change, 'com.example.app', other), true)
  })

  it('treats host settings persist as foreign to every sandbox', () => {
    const hostChange = {
      ...change,
      source: HOST_SETTINGS_WRITE_SOURCE,
    }
    assert.equal(
      isForeignTappKvChange(hostChange, 'com.example.app', writer),
      true,
    )
  })
})

describe('settings change bus', () => {
  it('does not leak onto the storage bus', () => {
    const settingsHits: string[] = []
    const storageHits: string[] = []
    const offSettings = onTappSettingsChange((change) => {
      settingsHits.push(change.key ?? '')
    })
    const offStorage = onTappStorageChange((change) => {
      storageHits.push(change.key ?? '')
    })
    try {
      emitTappSettingsChange({
        tappId: 'com.example.app',
        key: 'theme',
        operation: 'set',
        source: { id: 'writer' },
      })
      emitHostSettingsChange('com.example.app', 'compact')
      emitTappStorageChange({
        tappId: 'com.example.app',
        key: 'cache',
        operation: 'set',
        source: { id: 'writer' },
      })
      assert.deepEqual(settingsHits, ['theme', 'compact'])
      assert.deepEqual(storageHits, ['cache'])
    } finally {
      offSettings()
      offStorage()
    }
  })
})

describe('private change bus', () => {
  it('does not leak onto the storage bus', () => {
    const privateHits: string[] = []
    const storageHits: string[] = []
    const offPrivate = onTappPrivateChange((change) => {
      privateHits.push(change.key ?? '')
    })
    const offStorage = onTappStorageChange((change) => {
      storageHits.push(change.key ?? '')
    })
    try {
      emitTappPrivateChange({
        tappId: 'com.example.app',
        key: 'token',
        operation: 'set',
        source: { id: 'writer' },
      })
      emitTappStorageChange({
        tappId: 'com.example.app',
        key: 'cache',
        operation: 'set',
        source: { id: 'writer' },
      })
      assert.deepEqual(privateHits, ['token'])
      assert.deepEqual(storageHits, ['cache'])
    } finally {
      offPrivate()
      offStorage()
    }
  })
})

describe('settingsChanged reaches sandbox onChanged', () => {
  it('delivers once on Page and Widget, and ignores storageChanged', () => {
    const page = evaluateSdk(generateFullSDK(instance, 'session-token', 'page'))
    const widget = evaluateSdk(generateWidgetSDK(instance, 'session-token'))
    const pageHits: unknown[] = []
    const widgetHits: unknown[] = []
    const pageStorageHits: unknown[] = []

    page.tapp.settings.onChanged((event: unknown) => pageHits.push(event))
    page.tapp.storage.onChanged((event: unknown) => pageStorageHits.push(event))
    widget.tapp.settings.onChanged((event: unknown) => widgetHits.push(event))

    const payload = { key: 'compact', operation: 'set' }
    page.dispatchHostEvent('settingsChanged', payload)
    widget.dispatchHostEvent('settingsChanged', payload)
    page.dispatchHostEvent('storageChanged', { key: 'cache', operation: 'set' })
    widget.dispatchHostEvent('storageChanged', { key: 'cache', operation: 'set' })

    assert.deepEqual(pageHits, [payload])
    assert.deepEqual(widgetHits, [payload])
    assert.deepEqual(pageStorageHits, [{ key: 'cache', operation: 'set' }])
  })

  it('keeps settings.onChanged on headless', () => {
    const headless = evaluateSdk(
      generateFullSDK(instance, 'session-token', 'headless'),
    )
    const hits: unknown[] = []
    headless.tapp.settings.onChanged((event: unknown) => hits.push(event))
    headless.dispatchHostEvent('settingsChanged', {
      key: 'compact',
      operation: 'set',
    })
    assert.deepEqual(hits, [{ key: 'compact', operation: 'set' }])
  })
})

describe('host remount policy', () => {
  it('remounts on storage/shared, not on settings or private', () => {
    const sandbox = readFileSync(
      fileURLToPath(new URL('./TappWidgetSandbox.tsx', import.meta.url)),
      'utf8',
    )
    const pageSandbox = readFileSync(
      fileURLToPath(new URL('./TappPageSandbox.tsx', import.meta.url)),
      'utf8',
    )
    const host = readFileSync(
      fileURLToPath(new URL('../../components/widgets/TappWidget.tsx', import.meta.url)),
      'utf8',
    )
    assert.match(sandbox, /bindAllTappKvChanges/)
    assert.match(pageSandbox, /bindAllTappKvChanges/)
    const signals = readFileSync(
      fileURLToPath(new URL('./WidgetRuntimeSignals.ts', import.meta.url)),
      'utf8',
    )
    assert.match(signals, /remount: 'storage-changed'/)
    assert.match(signals, /remount: 'shared-changed'/)
    const privateRow = signals.match(
      /subscribe: onTappPrivateChange, action: 'privateChanged'[^}]*/,
    )?.[0]
    assert.ok(privateRow, 'private must be in TAPP_KV_BRIDGES')
    assert.doesNotMatch(privateRow, /remount/)
    const settingsRow = signals.match(
      /subscribe: onTappSettingsChange, action: 'settingsChanged'[^}]*/,
    )?.[0]
    assert.ok(settingsRow, 'settings must be in TAPP_KV_BRIDGES')
    assert.doesNotMatch(settingsRow, /remount/)
    assert.doesNotMatch(host, /onTappSettingsChange/)
    assert.match(host, /onTappWidgetInvalidate/)
  })
})

describe('playground settings.set', () => {
  it('emits the settings bus after persist, not storage', async () => {
    const handlers = new Map<
      string,
      (message: TappMessage) => Promise<{ success: boolean }>
    >()
    const bridge = {
      registerHandler(
        action: string,
        handler: (message: TappMessage) => Promise<{ success: boolean }>,
      ) {
        handlers.set(action, handler)
      },
    } as unknown as TappBridge

    const settingsHits: string[] = []
    const storageHits: string[] = []
    const offSettings = onTappSettingsChange((change) => {
      settingsHits.push(`${change.key}:${change.operation}`)
    })
    const offStorage = onTappStorageChange((change) => {
      storageHits.push(`${change.key}:${change.operation}`)
    })

    registerPlaygroundPreviewHandlers(
      bridge,
      instance,
      new Map(),
      new Map(),
    )

    try {
      const handler = handlers.get('settings.set')
      assert.ok(handler)
      const result = await handler({
        type: 'request',
        id: '1',
        action: 'settings.set',
        payload: { args: ['compact', true] },
        timestamp: 0,
      } as TappMessage)
      assert.equal(result.success, true)
      assert.deepEqual(settingsHits, ['compact:set'])
      assert.deepEqual(storageHits, [])
    } finally {
      offSettings()
      offStorage()
    }
  })
})
