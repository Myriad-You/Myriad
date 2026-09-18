import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  shouldAutoHideTourHint,
  shouldShowTourHint,
  TOUR_HINT_AUTO_HIDE_MS,
} from './tourHintLogic'

describe('tour hint visibility', () => {
  it('hides on phone and shows on wider viewports', () => {
    assert.equal(shouldShowTourHint(true), false)
    assert.equal(shouldShowTourHint(false), true)
  })
})

describe('tour hint auto-hide', () => {
  it('is thirty seconds', () => {
    assert.equal(TOUR_HINT_AUTO_HIDE_MS, 30_000)
  })

  it('runs in production and stays in development', () => {
    assert.equal(shouldAutoHideTourHint(false), true)
    assert.equal(shouldAutoHideTourHint(true), false)
  })
})
