import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  applyGuideDragDelta,
  clamp,
  clampPanelToViewport,
  computeGuidePosition,
  GUIDE_GAP,
  GUIDE_VIEWPORT_PAD,
  isGuideAnchorVisible,
  shouldAllowGuideClose,
  shouldToggleCloseGuide,
} from './settingTitleGuideLogic'

describe('clamp', () => {
  it('clamps into [min, max]', () => {
    assert.equal(clamp(5, 0, 10), 5)
    assert.equal(clamp(-1, 0, 10), 0)
    assert.equal(clamp(99, 0, 10), 10)
  })
})

describe('clampPanelToViewport', () => {
  const pad = GUIDE_VIEWPORT_PAD
  const vw = 1000
  const vh = 800

  it('keeps in-bounds panel unchanged', () => {
    const next = clampPanelToViewport(100, 200, 300, 200, vw, vh)
    assert.deepEqual(next, { top: 100, left: 200 })
  })

  it('clamps left/top below pad', () => {
    const next = clampPanelToViewport(-50, -20, 200, 100, vw, vh)
    assert.equal(next.left, pad)
    assert.equal(next.top, pad)
  })

  it('clamps right/bottom overflow', () => {
    const panelW = 300
    const panelH = 200
    const next = clampPanelToViewport(900, 900, panelW, panelH, vw, vh)
    assert.equal(next.left, vw - panelW - pad)
    assert.equal(next.top, vh - panelH - pad)
  })

  it('when panel larger than viewport, sticks to pad', () => {
    const next = clampPanelToViewport(50, 50, 2000, 1500, vw, vh)
    assert.equal(next.left, pad)
    assert.equal(next.top, pad)
  })
})

describe('applyGuideDragDelta', () => {
  it('applies pointer delta then clamps', () => {
    const next = applyGuideDragDelta(
      100,
      100,
      40,
      -30,
      200,
      150,
      1000,
      800,
    )
    assert.deepEqual(next, { top: 70, left: 140 })
  })

  it('does not leave viewport after large drag', () => {
    const panelW = 250
    const panelH = 180
    const vw = 1000
    const vh = 800
    const next = applyGuideDragDelta(10, 10, 5000, 5000, panelW, panelH, vw, vh)
    assert.equal(next.left, vw - panelW - GUIDE_VIEWPORT_PAD)
    assert.equal(next.top, vh - panelH - GUIDE_VIEWPORT_PAD)
  })
})

describe('shouldAllowGuideClose', () => {
  it('allows close when not pinned', () => {
    assert.equal(shouldAllowGuideClose(false), true)
    assert.equal(shouldAllowGuideClose(false, false), true)
    assert.equal(shouldAllowGuideClose(false, true), true)
  })

  it('blocks non-force close when pinned', () => {
    assert.equal(shouldAllowGuideClose(true), false)
    assert.equal(shouldAllowGuideClose(true, false), false)
  })

  it('allows force close when pinned (close button)', () => {
    assert.equal(shouldAllowGuideClose(true, true), true)
  })
})

describe('shouldToggleCloseGuide', () => {
  it('never closes via toggle when pinned', () => {
    assert.equal(shouldToggleCloseGuide(true, true), false)
  })

  it('closes via toggle when open and not pinned', () => {
    assert.equal(shouldToggleCloseGuide(true, false), true)
  })

  it('does not close when already closed', () => {
    assert.equal(shouldToggleCloseGuide(false, false), false)
    assert.equal(shouldToggleCloseGuide(false, true), false)
  })
})

describe('computeGuidePosition', () => {
  const panelW = 280
  const panelH = 200
  const vw = 1200
  const vh = 900

  it('prefers top when there is room above', () => {
    const trigger = {
      top: 400,
      left: 100,
      right: 220,
      bottom: 430,
      width: 120,
      height: 30,
    }
    const pos = computeGuidePosition(trigger, panelW, panelH, vw, vh)
    assert.equal(pos.placement, 'top')
    assert.equal(pos.top, trigger.top - GUIDE_GAP - panelH)
    assert.equal(pos.left, trigger.left)
  })

  it('falls back to left when top is tight but left is open', () => {
    const trigger = {
      top: 20,
      left: 500,
      right: 620,
      bottom: 50,
      width: 120,
      height: 30,
    }
    const pos = computeGuidePosition(trigger, panelW, panelH, vw, vh)
    assert.equal(pos.placement, 'left')
    assert.equal(pos.left, trigger.left - GUIDE_GAP - panelW)
    assert.equal(pos.top, trigger.top)
  })

  it('result stays within viewport padding', () => {
    const trigger = {
      top: 10,
      left: 10,
      right: 80,
      bottom: 40,
      width: 70,
      height: 30,
    }
    const pos = computeGuidePosition(trigger, panelW, panelH, vw, vh)
    assert.ok(pos.left >= GUIDE_VIEWPORT_PAD)
    assert.ok(pos.top >= GUIDE_VIEWPORT_PAD)
    assert.ok(pos.left + panelW <= vw - GUIDE_VIEWPORT_PAD + 0.001)
    assert.ok(pos.top + panelH <= vh - GUIDE_VIEWPORT_PAD + 0.001)
  })
})

describe('isGuideAnchorVisible', () => {
  const vw = 1000
  const vh = 800

  it('true when fully in viewport', () => {
    assert.equal(
      isGuideAnchorVisible(
        { top: 100, left: 100, right: 200, bottom: 140, width: 100, height: 40 },
        vw,
        vh,
      ),
      true,
    )
  })

  it('false when fully above viewport', () => {
    assert.equal(
      isGuideAnchorVisible(
        { top: -80, left: 100, right: 200, bottom: -20, width: 100, height: 60 },
        vw,
        vh,
      ),
      false,
    )
  })

  it('false when only a sliver remains visible', () => {
    assert.equal(
      isGuideAnchorVisible(
        { top: -30, left: 100, right: 200, bottom: 5, width: 100, height: 35 },
        vw,
        vh,
      ),
      false,
    )
  })

  it('true when partially visible above min edge', () => {
    assert.equal(
      isGuideAnchorVisible(
        { top: -20, left: 100, right: 200, bottom: 30, width: 100, height: 50 },
        vw,
        vh,
      ),
      true,
    )
  })
})
