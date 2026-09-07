import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  getStandardWidgetDimensionsForBand,
  LIBRARY_DOCK_PREVIEW_INSET_SCALE,
  libraryDockPreviewDisplayScale,
  resolveWidgetContentScale,
  STANDARD_CELL_BY_BAND,
  STANDARD_CELL_SIZE,
  WIDGET_COMPACT_SCALE,
  WIDGET_SCALE_MAX,
  WIDGET_SCALE_MIN,
  widgetSizeSpan,
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

describe('widgetSizeSpan', () => {
  it('is the single grid span table', () => {
    assert.deepEqual(widgetSizeSpan('4x2'), { w: 4, h: 2 })
    assert.deepEqual(widgetSizeSpan('unknown'), { w: 2, h: 2 })
  })
})

describe('libraryDockPreviewDisplayScale', () => {
  it('is 0.95 of the design cell when the live cell matches 80px', () => {
    assert.equal(LIBRARY_DOCK_PREVIEW_INSET_SCALE, 0.95)
    assert.equal(
      libraryDockPreviewDisplayScale(STANDARD_CELL_SIZE),
      LIBRARY_DOCK_PREVIEW_INSET_SCALE,
    )
  })

  it('tracks a capped desktop cell (~79px) so previews sit under placed tiles', () => {
    const cell = 79
    const scale = libraryDockPreviewDisplayScale(cell)
    assert.equal(
      scale,
      (cell / STANDARD_CELL_SIZE) * LIBRARY_DOCK_PREVIEW_INSET_SCALE,
    )
    assert.ok(scale < 1)
    assert.ok(scale > 0.9)
  })
})
