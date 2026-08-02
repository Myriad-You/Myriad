import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  getStandardWidgetDimensionsForBand,
  resolveWidgetContentScale,
  STANDARD_CELL_BY_BAND,
  WIDGET_COMPACT_SCALE,
  WIDGET_SCALE_MAX,
  WIDGET_SCALE_MIN,
} from './widgetSizeScale'

describe('band-aware widget design size', () => {
  it('uses larger resting cells on tablet than desktop', () => {
    assert.ok(STANDARD_CELL_BY_BAND.tablet > STANDARD_CELL_BY_BAND.desktop)
    const d = getStandardWidgetDimensionsForBand('2x2', 'desktop')
    const t = getStandardWidgetDimensionsForBand('2x2', 'tablet')
    assert.equal(d.width, 2 * STANDARD_CELL_BY_BAND.desktop)
    assert.equal(t.width, 2 * STANDARD_CELL_BY_BAND.tablet)
    assert.ok(t.width > d.width)
  })
})

describe('resolveWidgetContentScale', () => {
  it('stays near 1 on typical desktop 16-col 2x2 (~160px)', () => {
    const scale = resolveWidgetContentScale({
      measuredWidth: 160,
      measuredHeight: 160,
      widgetSize: '2x2',
      band: 'desktop',
    })
    assert.ok(scale >= 0.95 && scale <= 1.05, `scale=${scale}`)
  })

  it('stays near 1 on typical tablet 8-col 2x2 (~224–230px)', () => {
    const scale = resolveWidgetContentScale({
      measuredWidth: 230,
      measuredHeight: 230,
      widgetSize: '2x2',
      band: 'tablet',
    })
    assert.ok(scale >= 0.95 && scale <= 1.06, `scale=${scale}`)
  })

  it('keeps tablet large cells out of compact mode', () => {
    const tabletScale = resolveWidgetContentScale({
      measuredWidth: 240,
      measuredHeight: 240,
      widgetSize: '2x2',
      band: 'tablet',
    })
    assert.ok(tabletScale > WIDGET_COMPACT_SCALE, `tablet=${tabletScale}`)
  })

  it('clamps extreme shrink/grow', () => {
    const tiny = resolveWidgetContentScale({
      measuredWidth: 40,
      measuredHeight: 40,
      widgetSize: '2x2',
      band: 'desktop',
    })
    const huge = resolveWidgetContentScale({
      measuredWidth: 800,
      measuredHeight: 800,
      widgetSize: '2x2',
      band: 'desktop',
    })
    assert.equal(tiny, WIDGET_SCALE_MIN)
    assert.equal(huge, WIDGET_SCALE_MAX)
  })
})
