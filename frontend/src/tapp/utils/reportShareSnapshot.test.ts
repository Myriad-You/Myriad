/**
 * Unit tests for report share snapshot helpers.
 *
 *   node --experimental-strip-types --test src/tapp/utils/reportShareSnapshot.test.ts
 */
/* eslint-disable test/no-import-node-test -- node:test; project has no vitest dep */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  buildReportShareSnapshot,
  formatReportContentBody,
  REPORT_SHARE_SNAPSHOT_FIELDS,
  wireReportSharePayload,
} from './reportShareSnapshot.ts'

describe('REPORT_SHARE_SNAPSHOT_FIELDS', () => {
  it('coordinates Aro + federation field names', () => {
    assert.deepEqual(REPORT_SHARE_SNAPSHOT_FIELDS, [
      'report_id',
      'summary',
      'platform',
      'content_preview',
    ])
  })
})

describe('buildReportShareSnapshot', () => {
  it('builds Aro field names from catalog report', () => {
    const snap = buildReportShareSnapshot({
      id: 42,
      platform: 'github',
      summary: 'Active OSS contributor',
      type: 'platform',
      content: {
        summary: 'Active OSS contributor',
        insights: ['Many stars', 'Steady commits'],
      },
    })
    assert.equal(snap.report_id, '42')
    assert.equal(snap.summary, 'Active OSS contributor')
    assert.equal(snap.platform, 'github')
    assert.equal(snap.content_preview, 'Active OSS contributor')
  })

  it('falls back to first insight when summary missing', () => {
    const snap = buildReportShareSnapshot({
      id: 'rpt-1',
      platform: 'mal',
      content: { insights: ['追番 12 部', '完成 8 部'] },
    })
    assert.equal(snap.report_id, 'rpt-1')
    assert.equal(snap.platform, 'mal')
    assert.match(snap.content_preview, /追番 12 部/)
    assert.ok(snap.summary.length > 0)
  })

  it('never returns id-only empty snapshot for partial input', () => {
    const snap = buildReportShareSnapshot({ id: 7 })
    assert.equal(snap.report_id, '7')
    assert.equal(snap.summary, 'Report')
    assert.equal(snap.platform, '')
    assert.equal(snap.content_preview, '')
  })

  it('truncates long previews to 500 chars', () => {
    const long = 'x'.repeat(800)
    const snap = buildReportShareSnapshot({ id: 1, summary: long })
    assert.equal(snap.content_preview.length, 500)
  })
})

describe('formatReportContentBody', () => {
  it('formats summary + insights as structured text', () => {
    const text = formatReportContentBody({
      summary: 'Overall profile',
      insights: ['Insight A', 'Insight B'],
    })
    assert.match(text, /Overall profile/)
    assert.match(text, /• Insight A/)
    assert.match(text, /• Insight B/)
    assert.doesNotMatch(text, /\[object Object\]/)
  })

  it('never stringifies objects as [object Object]', () => {
    const text = formatReportContentBody({ nested: { a: 1 }, count: 3 })
    assert.doesNotMatch(text, /\[object Object\]/)
    assert.match(text, /count: 3/)
  })

  it('uses fallback when content empty', () => {
    assert.equal(formatReportContentBody(null, 'snap preview'), 'snap preview')
    assert.equal(formatReportContentBody({}, 'snap preview'), 'snap preview')
  })

  it('strips simple HTML from string content', () => {
    assert.equal(
      formatReportContentBody('<p>Hello<br>world</p>'),
      'Hello\nworld',
    )
  })
})

describe('wireReportSharePayload', () => {
  it('always sets report_id, summary, platform, content_preview', () => {
    const payload = wireReportSharePayload(
      { content_type: 'report', text: 'hi' },
      {
        reportId: 99,
        summary: 'Winter report',
        platform: 'mal',
        contentPreview: '追番 12 部',
      },
    )
    assert.equal(payload.report_id, '99')
    assert.equal(payload.summary, 'Winter report')
    assert.equal(payload.platform, 'mal')
    assert.equal(payload.content_preview, '追番 12 部')
    assert.equal(payload.title, 'Winter report')
    assert.match(String(payload.description), /mal/)
    assert.match(String(payload.description), /追番/)
  })

  it('fills summary from name when attach.summary missing', () => {
    const payload = wireReportSharePayload(
      {},
      { reportId: 'x', name: 'Fallback title', platform: 'bili' },
    )
    assert.equal(payload.summary, 'Fallback title')
    assert.equal(payload.platform, 'bili')
    assert.equal(payload.report_id, 'x')
  })
})
