/**
 *   pnpm exec tsx --test src/tapp/runtime/widgetInvalidateTarget.test.ts
 */

import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import {
  isLocalWidgetIdOfTapp,
  isSafeLocalWidgetId,
  parseWidgetInvalidateTargetArgs,
  resetWidgetInvalidateTargetRateLimitForTests,
  tryAcceptWidgetInvalidateTarget,
  WIDGET_INVALIDATE_TARGET_COOLDOWN_MS,
  WIDGET_INVALIDATE_TARGET_TAPP_MAX_PER_MINUTE,
} from './widgetInvalidateTarget.ts'

afterEach(() => {
  resetWidgetInvalidateTargetRateLimitForTests()
})

describe('parseWidgetInvalidateTargetArgs', () => {
  it('requires an explicit local widgetId', () => {
    const parsed = parseWidgetInvalidateTargetArgs([
      'config-saved',
      { target: { widgetId: 'stats' } },
    ])
    assert.deepEqual(parsed, {
      ok: true,
      reason: 'config-saved',
      widgetId: 'stats',
    })
  })

  it('rejects omitted options instead of defaulting to all', () => {
    const parsed = parseWidgetInvalidateTargetArgs(['data-ready'])
    assert.equal(parsed.ok, false)
    if (parsed.ok) return
    assert.match(parsed.error, /requires \{ target: \{ widgetId \} \}/)
  })

  it('rejects target all', () => {
    const parsed = parseWidgetInvalidateTargetArgs([
      'sync',
      { target: 'all' },
    ])
    assert.equal(parsed.ok, false)
    if (parsed.ok) return
    assert.match(parsed.error, /not supported/)
  })

  it('rejects a full registered widget id shape used as a cross-tapp probe', () => {
    assert.equal(isSafeLocalWidgetId('tapp.other.stats'), true)
    assert.equal(
      isLocalWidgetIdOfTapp('tapp.other.stats', ['stats'], ['stats']),
      false,
    )
  })

  it('truncates reason and rejects unsafe ids', () => {
    const parsed = parseWidgetInvalidateTargetArgs([
      'x'.repeat(300),
      { target: { widgetId: '../x' } },
    ])
    assert.equal(parsed.ok, false)
    const long = parseWidgetInvalidateTargetArgs([
      'x'.repeat(300),
      { target: { widgetId: 'stats' } },
    ])
    assert.equal(long.ok, true)
    if (long.ok) assert.equal(long.reason.length, 256)
  })
})

describe('tryAcceptWidgetInvalidateTarget', () => {
  it('allows one poke then enforces the 15s per-widget cooldown', () => {
    const first = tryAcceptWidgetInvalidateTarget('com.example.a', 'stats', 1_000)
    assert.equal(first.ok, true)
    const second = tryAcceptWidgetInvalidateTarget(
      'com.example.a',
      'stats',
      1_000 + WIDGET_INVALIDATE_TARGET_COOLDOWN_MS - 1,
    )
    assert.equal(second.ok, false)
    if (second.ok) return
    assert.equal(second.reason, 'cooldown')
    assert.equal(second.retryAfterMs, 1)
    const third = tryAcceptWidgetInvalidateTarget(
      'com.example.a',
      'stats',
      1_000 + WIDGET_INVALIDATE_TARGET_COOLDOWN_MS,
    )
    assert.equal(third.ok, true)
  })

  it('caps a Tapp at two targeted invalidations per minute', () => {
    assert.equal(WIDGET_INVALIDATE_TARGET_TAPP_MAX_PER_MINUTE, 2)
    assert.equal(
      tryAcceptWidgetInvalidateTarget('com.example.b', 'one', 10_000).ok,
      true,
    )
    assert.equal(
      tryAcceptWidgetInvalidateTarget('com.example.b', 'two', 20_000).ok,
      true,
    )
    const blocked = tryAcceptWidgetInvalidateTarget(
      'com.example.b',
      'three',
      30_000,
    )
    assert.equal(blocked.ok, false)
    if (blocked.ok) return
    assert.equal(blocked.reason, 'tapp-budget')
    const later = tryAcceptWidgetInvalidateTarget(
      'com.example.b',
      'three',
      10_000 + 60_000,
    )
    assert.equal(later.ok, true)
  })

  it('does not share budget across Tapps', () => {
    assert.equal(
      tryAcceptWidgetInvalidateTarget('com.example.c', 'stats', 1).ok,
      true,
    )
    assert.equal(
      tryAcceptWidgetInvalidateTarget('com.example.d', 'stats', 1).ok,
      true,
    )
  })
})
