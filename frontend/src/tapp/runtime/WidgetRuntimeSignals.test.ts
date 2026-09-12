import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  bindAllTappKvChanges,
  emitTappPrivateChange,
  emitTappSettingsChange,
  emitTappSharedChange,
  emitTappStorageChange,
  TAPP_KV_BRIDGES,
} from './WidgetRuntimeSignals.ts'

describe('TAPP_KV_BRIDGES', () => {
  it('remounts only storage and shared', () => {
    assert.deepEqual(
      TAPP_KV_BRIDGES.map((row) => ({
        action: row.action,
        remount: row.remount,
      })),
      [
        { action: 'storageChanged', remount: 'storage-changed' },
        { action: 'sharedChanged', remount: 'shared-changed' },
        { action: 'settingsChanged', remount: undefined },
        { action: 'privateChanged', remount: undefined },
      ],
    )
  })
})

describe('bindAllTappKvChanges', { concurrency: false }, () => {
  it('forwards foreign same-tapp events and remounts only storage/shared', () => {
    const bridge = {
      emit(action: string, payload: unknown) {
        events.push({ action, payload })
      },
    }
    const other = { kind: 'other-bridge' }
    const events: Array<{ action: string; payload: unknown }> = []
    const remounts: string[] = []
    const off = bindAllTappKvChanges(() => bridge, 'com.example.app', (reason) => {
      remounts.push(reason)
    })

    emitTappStorageChange({
      tappId: 'com.example.app',
      key: 'ready',
      operation: 'set',
      source: other,
    })
    emitTappSharedChange({
      tappId: 'com.example.app',
      key: 'posts',
      operation: 'remove',
      source: other,
    })
    emitTappPrivateChange({
      tappId: 'com.example.app',
      key: 'token',
      operation: 'set',
      source: other,
    })
    emitTappSettingsChange({
      tappId: 'com.example.app',
      key: 'theme',
      operation: 'set',
      source: other,
    })
    emitTappPrivateChange({
      tappId: 'com.example.other',
      key: 'token',
      operation: 'set',
      source: other,
    })
    emitTappPrivateChange({
      tappId: 'com.example.app',
      key: 'token',
      operation: 'set',
      source: bridge,
    })

    assert.deepEqual(events, [
      { action: 'storageChanged', payload: { key: 'ready', operation: 'set' } },
      { action: 'sharedChanged', payload: { key: 'posts', operation: 'remove' } },
      { action: 'privateChanged', payload: { key: 'token', operation: 'set' } },
      {
        action: 'settingsChanged',
        payload: { key: 'theme', operation: 'set' },
      },
    ])
    assert.deepEqual(remounts, ['storage-changed', 'shared-changed'])
    for (const event of events) {
      assert.equal(
        Object.hasOwn(event.payload as object, 'value'),
        false,
      )
    }
    off()
  })
})
