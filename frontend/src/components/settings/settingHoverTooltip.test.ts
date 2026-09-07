import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  computeHoverTooltipPosition,
  HOVER_TOOLTIP_GAP,
  HOVER_TOOLTIP_VIEWPORT_PAD,
} from './settingHoverTooltip'

const pad = HOVER_TOOLTIP_VIEWPORT_PAD
const gap = HOVER_TOOLTIP_GAP
const viewport = { width: 1000, height: 800 }

function box(left: number, top: number, width: number, height: number) {
  return { left, top, width, bottom: top + height }
}

describe('computeHoverTooltipPosition', () => {
  it('prefers bottom when there is room below', () => {
    const trigger = box(200, 100, 40, 22)
    const result = computeHoverTooltipPosition(
      trigger,
      160,
      48,
      'bottom',
      viewport,
    )
    assert.equal(result.placement, 'bottom')
    assert.equal(result.top, trigger.bottom + gap)
    assert.equal(result.left, trigger.left + trigger.width / 2 - 80)
  })

  it('flips to top when bottom is tight and above has more room', () => {
    const trigger = box(200, 760, 40, 22)
    const result = computeHoverTooltipPosition(
      trigger,
      160,
      48,
      'bottom',
      viewport,
    )
    assert.equal(result.placement, 'top')
    assert.equal(result.top, trigger.top - gap - 48)
  })

  it('clamps horizontally into the viewport padding', () => {
    const trigger = box(0, 100, 20, 22)
    const result = computeHoverTooltipPosition(
      trigger,
      400,
      40,
      'bottom',
      viewport,
    )
    assert.equal(result.left, pad)
  })
})
