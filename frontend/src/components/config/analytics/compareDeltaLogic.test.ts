/**
 * Run from frontend/:
 *   pnpm test:unit -- src/components/config/analytics/compareDeltaLogic.test.ts
 */

import type { CompareLabels } from './compareDeltaLogic.ts'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  compareColorPalette,
  compareKindLabel,
  compareTone,
  formatComparePct,
  formatCompareValue,
} from './compareDeltaLogic.ts'

const labels: CompareLabels = {
  day: '日环比',
  week: '周环比',
  month: '月环比',
  period: '较上期',
  new: '新',
  vsPrevious: '上期 {n}',
}

describe('compareDelta', () => {
  it('maps kind labels', () => {
    assert.equal(compareKindLabel('day', labels), '日环比')
    assert.equal(compareKindLabel('week', labels), '周环比')
    assert.equal(compareKindLabel('month', labels), '月环比')
    assert.equal(compareKindLabel('period', labels), '较上期')
    assert.equal(compareKindLabel(undefined, labels), '较上期')
  })

  it('tones from pct', () => {
    assert.equal(compareTone({ pct: 12 }), 'up')
    assert.equal(compareTone({ pct: -3 }), 'down')
    assert.equal(compareTone({ pct: 0 }), 'flat')
    assert.equal(compareTone({ pct: null, current: 5 }), 'new')
    assert.equal(compareTone({ pct: null, current: 0 }), 'flat')
    assert.equal(compareTone(null), 'none')
  })

  it('formats signed percent', () => {
    assert.equal(formatComparePct(12.34, 'en-US'), '+12.3%')
    assert.equal(formatComparePct(-5, 'en-US'), '−5%')
    assert.equal(formatComparePct(0, 'en-US'), '0%')
    assert.equal(formatComparePct(null, 'en-US'), '—')
  })

  it('formats value with new baseline', () => {
    assert.equal(
      formatCompareValue({ pct: null, current: 3 }, 'zh-CN', labels),
      '新',
    )
    assert.equal(formatCompareValue({ pct: 10 }, 'zh-CN', labels), '+10%')
  })

  it('picks regional rise/fall color palette from locale', () => {
    assert.equal(compareColorPalette('zh-CN'), 'red-up')
    assert.equal(compareColorPalette('zh-TW'), 'red-up')
    assert.equal(compareColorPalette('ja-JP'), 'red-up')
    assert.equal(compareColorPalette('ko-KR'), 'red-up')
    assert.equal(compareColorPalette('en-US'), 'green-up')
    assert.equal(compareColorPalette('de-DE'), 'green-up')
  })
})
