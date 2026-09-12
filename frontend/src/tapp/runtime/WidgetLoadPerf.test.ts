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

  it('evicts oldest records past the cap', () => {
    for (let i = 0; i < 81; i += 1) {
      widgetPerfMark(`com.cap`, `w${i}`, 'host-load-start', '2x2')
    }
    const summary = getWidgetPerfSummary()
    assert.equal(summary.count, 80)
    const keys = summary.records.map((r) => r.widgetId)
    assert.equal(keys.includes('w0'), false)
    assert.equal(keys.includes('w80'), true)
  })
})
