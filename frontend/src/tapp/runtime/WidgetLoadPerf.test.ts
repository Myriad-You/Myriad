/**
 * Widget load perf marks (host-side User Timing helpers).
 *
 *   pnpm exec tsx --test src/tapp/runtime/WidgetLoadPerf.test.ts
 */

import assert from 'node:assert/strict'
import { beforeEach, describe, it } from 'node:test'
import {
  clearWidgetPerf,
  getWidgetPerfSummary,
  widgetPerfMark,
} from './WidgetLoadPerf.ts'

describe('WidgetLoadPerf', () => {
  beforeEach(() => {
    clearWidgetPerf()
  })

  it('records host-to-ready timeline for multi-widget keys', () => {
    widgetPerfMark('com.a', 'w1', 'host-load-start', '2x2')
    widgetPerfMark('com.a', 'w1', 'resources-ready', '2x2')
    widgetPerfMark('com.a', 'w1', 'sandbox-mount', '2x2')
    widgetPerfMark('com.a', 'w1', 'iframe-ready', '2x2')

    widgetPerfMark('com.a', 'w2', 'host-load-start', '4x2')
    widgetPerfMark('com.a', 'w2', 'resources-ready', '4x2')
    widgetPerfMark('com.a', 'w2', 'sandbox-mount', '4x2')
    widgetPerfMark('com.a', 'w2', 'iframe-ready', '4x2')

    const summary = getWidgetPerfSummary()
    assert.equal(summary.count, 2)
    assert.equal(summary.readyCount, 2)
    assert.ok(summary.avgTotalHostToReadyMs !== null)
    assert.ok((summary.avgTotalHostToReadyMs as number) >= 0)
    assert.ok(summary.maxTotalHostToReadyMs !== null)
    assert.ok(summary.p95TotalHostToReadyMs !== null)
  })
})
