import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  placeStylePanel,
  STYLE_PANEL_GAP,
  STYLE_PANEL_MARGIN,
} from './titleFontSelectorPlacement'

const PANEL = { width: 320, height: 280 }
const DESKTOP = { width: 1440, height: 900 }

describe('placeStylePanel', () => {
  it('opens below a top header button and left-aligns to it', () => {
    const button = { top: 80, left: 120, right: 188, bottom: 114 }
    const pos = placeStylePanel(button, PANEL.width, PANEL.height, DESKTOP)
    assert.equal(pos.placement, 'below')
    assert.equal(pos.left, button.left)
    assert.equal(pos.top, button.bottom + STYLE_PANEL_GAP)
  })

  it('opens above a free-layout bottom-rail button instead of covering it', () => {
    const button = { top: 844, left: 700, right: 770, bottom: 878 }
    const pos = placeStylePanel(button, PANEL.width, PANEL.height, DESKTOP)
    assert.equal(pos.placement, 'above')
    assert.equal(pos.left, button.left)
    assert.equal(pos.top, button.top - STYLE_PANEL_GAP - PANEL.height)
    assert.ok(pos.top + PANEL.height <= button.top - STYLE_PANEL_GAP)
    assert.ok(pos.top + PANEL.height < DESKTOP.height - 40)
  })

  it('shifts left when a bottom-right button would overflow the viewport', () => {
    const button = { top: 844, left: 1100, right: 1170, bottom: 878 }
    const viewport = { width: 1280, height: 900 }
    const pos = placeStylePanel(button, PANEL.width, PANEL.height, viewport)
    assert.equal(pos.placement, 'above')
    assert.equal(pos.left, button.right - PANEL.width)
    assert.ok(pos.left + PANEL.width + STYLE_PANEL_MARGIN <= viewport.width)
  })

  it('clamps into the viewport when the panel is taller than either side', () => {
    const button = { top: 40, left: 16, right: 80, bottom: 72 }
    const viewport = { width: 400, height: 200 }
    const pos = placeStylePanel(button, 360, 180, viewport)
    assert.ok(pos.left >= STYLE_PANEL_MARGIN)
    assert.ok(pos.left + 360 + STYLE_PANEL_MARGIN <= viewport.width)
    assert.ok(pos.top >= STYLE_PANEL_MARGIN)
    assert.ok(pos.top + 180 + STYLE_PANEL_MARGIN <= viewport.height)
  })
})
