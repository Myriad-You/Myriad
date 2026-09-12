import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  HOME_GRID_COLS_DESKTOP,
  HOME_GRID_COLS_PHONE,
  HOME_GRID_COLS_TABLET,
  resolveHomeGridColumns,
  resolveViewportBand,
  VIEWPORT_DESKTOP_MIN,
  VIEWPORT_PHONE_MAX,
  VIEWPORT_TABLET_MAX,
  VIEWPORT_TABLET_MIN,
} from './viewportBands'

describe('resolveViewportBand', () => {
  it('hard-cuts phone / tablet / desktop on exact integers', () => {
    assert.equal(resolveViewportBand(VIEWPORT_PHONE_MAX), 'phone')
    assert.equal(resolveViewportBand(VIEWPORT_TABLET_MIN), 'tablet')
    assert.equal(resolveViewportBand(VIEWPORT_TABLET_MAX), 'tablet')
    assert.equal(resolveViewportBand(VIEWPORT_DESKTOP_MIN), 'desktop')
    assert.equal(resolveViewportBand(1024), 'tablet')
    assert.equal(resolveViewportBand(1077), 'tablet')
    assert.equal(resolveViewportBand(1078), 'desktop')
  })
})

describe('resolveHomeGridColumns', () => {
  it('maps bands to 4 / 8 / 16 with no dead zone at edges', () => {
    assert.equal(resolveHomeGridColumns(390, 16), HOME_GRID_COLS_PHONE)
    assert.equal(resolveHomeGridColumns(767, 16), HOME_GRID_COLS_PHONE)
    assert.equal(resolveHomeGridColumns(768, 4), HOME_GRID_COLS_TABLET)
    assert.equal(resolveHomeGridColumns(1077, 16), HOME_GRID_COLS_TABLET)
    assert.equal(resolveHomeGridColumns(1078, 8), HOME_GRID_COLS_DESKTOP)
    assert.equal(resolveHomeGridColumns(1200, 8), HOME_GRID_COLS_DESKTOP)
  })

  it('ignores previous at the desktop edge (no ±16 hysteresis)', () => {
    // Hard cut at 1078.
    assert.equal(
      resolveHomeGridColumns(1077, HOME_GRID_COLS_TABLET),
      HOME_GRID_COLS_TABLET,
    )
    assert.equal(
      resolveHomeGridColumns(1078, HOME_GRID_COLS_TABLET),
      HOME_GRID_COLS_DESKTOP,
    )
    // Hard cut at 1078.
    assert.equal(
      resolveHomeGridColumns(1078, HOME_GRID_COLS_DESKTOP),
      HOME_GRID_COLS_DESKTOP,
    )
    assert.equal(
      resolveHomeGridColumns(1077, HOME_GRID_COLS_DESKTOP),
      HOME_GRID_COLS_TABLET,
    )
  })

  it('ignores previous at the phone edge (no ±12 hysteresis)', () => {
    assert.equal(
      resolveHomeGridColumns(767, HOME_GRID_COLS_TABLET),
      HOME_GRID_COLS_PHONE,
    )
    assert.equal(
      resolveHomeGridColumns(768, HOME_GRID_COLS_PHONE),
      HOME_GRID_COLS_TABLET,
    )
  })
})
