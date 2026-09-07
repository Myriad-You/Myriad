import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  intersectionKeepsTappWidgetMounted,
  TAPP_WIDGET_VIEWPORT_OFFSCREEN_RECHECKS,
} from './tappWidgetViewport'

describe('intersectionKeepsTappWidgetMounted', () => {
  it('keeps the sandbox mounted when the first box is 0×0', () => {
    assert.equal(
      intersectionKeepsTappWidgetMounted({
        isIntersecting: false,
        boundingClientRect: { width: 0, height: 0 },
      }),
      true,
    )
  })

  it('keeps the sandbox mounted while only one axis has laid out', () => {
    assert.equal(
      intersectionKeepsTappWidgetMounted({
        isIntersecting: false,
        boundingClientRect: { width: 240, height: 0 },
      }),
      true,
    )
  })

  it('adopts a real off-screen box so far-away tiles can hold', () => {
    assert.equal(
      intersectionKeepsTappWidgetMounted({
        isIntersecting: false,
        boundingClientRect: { width: 240, height: 160 },
      }),
      false,
    )
  })

  it('keeps an intersecting laid-out box mounted', () => {
    assert.equal(
      intersectionKeepsTappWidgetMounted({
        isIntersecting: true,
        boundingClientRect: { width: 240, height: 160 },
      }),
      true,
    )
  })

  it('retries a few times before holding so entrance motion can settle', () => {
    assert.ok(TAPP_WIDGET_VIEWPORT_OFFSCREEN_RECHECKS >= 3)
  })
})
