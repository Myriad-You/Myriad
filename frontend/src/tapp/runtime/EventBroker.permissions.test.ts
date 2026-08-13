/**
 * EventBroker system.theme.changed permission gating.
 *
 * `system.theme.changed` is a host-produced browser-local topic whose producer
 * re-emits the *current* theme immediately on registration (subscribeToTheme
 * initial callback). Forwarding it must therefore require the granted
 * `ui:theme:subscribe` permission — `event:subscribe` alone (or a mere
 * `ui:theme:read` grant) must not leak the current theme nor later switches.
 *
 *   pnpm exec tsx --test src/tapp/runtime/EventBroker.permissions.test.ts
 */

import type { TappInstance } from '../types'
import type { TappBridge } from './TappBridge'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { registerEventHandlers } from './EventBroker.ts'

interface EmittedEvent {
  action: string
  payload: unknown
}

function createMockBridge(): {
  bridge: TappBridge
  emitted: EmittedEvent[]
} {
  const emitted: EmittedEvent[] = []
  const bridge = {
    getRuntimeId: async () => 'test-runtime',
    getRuntimeGrant: async () => ({}),
    registerHandler: () => undefined,
    emit: (action: string, payload: unknown) => {
      emitted.push({ action, payload })
    },
  } as unknown as TappBridge
  return { bridge, emitted }
}

function instanceWith(
  grantedPermissions: string[],
  subscribe: string[],
): TappInstance {
  return {
    id: 'com.example.theme-broker',
    manifest: {
      id: 'com.example.theme-broker',
      name: 'Theme Broker',
      version: '1.0.0',
      main: 'main.js',
      permissions: grantedPermissions,
      events: { subscribe },
      category: 'utility',
    },
    status: 'running',
    installedAt: '2026-08-14T00:00:00Z',
    grantedPermissions,
    userRole: 'admin',
  }
}

function themeChangedEvents(emitted: EmittedEvent[]): unknown[] {
  return emitted
    .filter((event) => event.action === 'tappEvent')
    .map((event) => (event.payload as { topic?: string }).topic)
    .filter((topic): topic is string => topic === 'system.theme.changed')
}

describe('EventBroker system.theme.changed gating', () => {
  it('does not register/forward theme changes with event:subscribe but without ui:theme:subscribe', () => {
    const { bridge, emitted } = createMockBridge()
    const instance = instanceWith(
      ['event:subscribe'],
      ['system.theme.changed'],
    )

    const cleanup = registerEventHandlers(bridge, instance)
    // subscribeToTheme re-emits the current theme synchronously on
    // registration — with the gate in place nothing may be forwarded.
    assert.deepEqual(themeChangedEvents(emitted), [])
    cleanup()
  })

  it('forwards system.theme.changed when ui:theme:subscribe is granted', () => {
    const { bridge, emitted } = createMockBridge()
    const instance = instanceWith(
      ['event:subscribe', 'ui:theme:subscribe'],
      ['system.theme.changed'],
    )

    const cleanup = registerEventHandlers(bridge, instance)
    const forwarded = emitted.filter(
      (event) => event.action === 'tappEvent',
    )
    assert.equal(forwarded.length, 1, 'expected one initial theme event')
    const envelope = forwarded[0]!.payload as {
      topic: string
      payload: { theme: string }
    }
    assert.equal(envelope.topic, 'system.theme.changed')
    assert.ok(
      envelope.payload.theme === 'light' || envelope.payload.theme === 'dark',
      `unexpected theme payload: ${envelope.payload.theme}`,
    )
    cleanup()
  })

  it('does not allow subscription via ui:theme:read alone (read does not imply subscribe)', () => {
    const { bridge, emitted } = createMockBridge()
    const instance = instanceWith(
      ['event:subscribe', 'ui:theme:read'],
      ['system.theme.changed'],
    )

    const cleanup = registerEventHandlers(bridge, instance)
    assert.deepEqual(themeChangedEvents(emitted), [])
    cleanup()
  })

  it('forwards nothing when the topic is not declared even with the permission', () => {
    const { bridge, emitted } = createMockBridge()
    const instance = instanceWith(['ui:theme:subscribe'], [])

    const cleanup = registerEventHandlers(bridge, instance)
    assert.deepEqual(themeChangedEvents(emitted), [])
    cleanup()
  })
})
