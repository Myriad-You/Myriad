import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  edgeRevealShouldShow,
  isNearNavEdge,
  NAV_EDGE_THRESHOLD,
  NAV_HOT_EDGE_THRESHOLD,
  navScrollDecision,
} from './navAutoHide'

describe('isNearNavEdge', () => {
  it('desktop uses the left band', () => {
    assert.equal(isNearNavEdge('desktop', 0, 400, 800), true)
    assert.equal(
      isNearNavEdge('desktop', NAV_EDGE_THRESHOLD - 1, 400, 800),
      true,
    )
    assert.equal(isNearNavEdge('desktop', NAV_EDGE_THRESHOLD, 400, 800), false)
    assert.equal(isNearNavEdge('desktop', 400, 790, 800), false)
  })

  it('mobile uses the bottom band', () => {
    assert.equal(isNearNavEdge('mobile', 400, 750, 800), true)
    assert.equal(isNearNavEdge('mobile', 400, 701, 800), true)
    assert.equal(isNearNavEdge('mobile', 400, 700, 800), false)
    assert.equal(isNearNavEdge('mobile', 10, 400, 800), false)
  })

  it('hot edge is the same check with a tighter threshold', () => {
    assert.equal(
      isNearNavEdge('desktop', 0, 400, 800, NAV_HOT_EDGE_THRESHOLD),
      true,
    )
    assert.equal(
      isNearNavEdge(
        'desktop',
        NAV_HOT_EDGE_THRESHOLD,
        400,
        800,
        NAV_HOT_EDGE_THRESHOLD,
      ),
      false,
    )
    assert.equal(
      isNearNavEdge('mobile', 400, 800, 800, NAV_HOT_EDGE_THRESHOLD),
      true,
    )
    assert.equal(
      isNearNavEdge(
        'mobile',
        400,
        800 - NAV_HOT_EDGE_THRESHOLD,
        800,
        NAV_HOT_EDGE_THRESHOLD,
      ),
      false,
    )
  })
})

describe('edgeRevealShouldShow', () => {
  it('first sample only seeds occupancy and never reveals', () => {
    const first = edgeRevealShouldShow({
      visible: false,
      primed: false,
      wasInsideProximity: false,
      isInsideProximity: true,
      wasInsideHot: false,
      isInsideHot: true,
    })
    assert.equal(first.show, false)
    assert.equal(first.primed, true)
    assert.equal(first.insideProximity, true)
    assert.equal(first.insideHot, true)
  })

  it('does not treat sitting in the proximity band as a reveal while visible', () => {
    const parked = edgeRevealShouldShow({
      visible: true,
      primed: true,
      wasInsideProximity: true,
      isInsideProximity: true,
      wasInsideHot: false,
      isInsideHot: false,
    })
    assert.equal(parked.show, false)
    assert.equal(parked.insideProximity, true)
  })

  it('does not re-show from jitter after hide while still in the band', () => {
    const jitter = edgeRevealShouldShow({
      visible: false,
      primed: true,
      wasInsideProximity: true,
      isInsideProximity: true,
      wasInsideHot: false,
      isInsideHot: false,
    })
    assert.equal(jitter.show, false)
  })

  it('shows when the pointer re-enters the proximity band', () => {
    const reenter = edgeRevealShouldShow({
      visible: false,
      primed: true,
      wasInsideProximity: false,
      isInsideProximity: true,
      wasInsideHot: false,
      isInsideHot: false,
    })
    assert.equal(reenter.show, true)
  })

  it('shows when the pointer pushes into the hot edge from inside the band', () => {
    const slam = edgeRevealShouldShow({
      visible: false,
      primed: true,
      wasInsideProximity: true,
      isInsideProximity: true,
      wasInsideHot: false,
      isInsideHot: true,
    })
    assert.equal(slam.show, true)
  })

  it('does not show on hot-edge jitter after already sitting on it', () => {
    const parkedHot = edgeRevealShouldShow({
      visible: false,
      primed: true,
      wasInsideProximity: true,
      isInsideProximity: true,
      wasInsideHot: true,
      isInsideHot: true,
    })
    assert.equal(parkedHot.show, false)
  })
})

describe('navScrollDecision', () => {
  it('ignores trackpad noise and rubber-band at the page top', () => {
    assert.equal(navScrollDecision(0, 0), 'none')
    assert.equal(navScrollDecision(8, 0), 'none')
    assert.equal(navScrollDecision(0, 8), 'none')
    assert.equal(navScrollDecision(40, 90), 'none')
  })

  it('hides only on a real downward flick past the top zone', () => {
    assert.equal(navScrollDecision(160, 100), 'hide')
    assert.equal(navScrollDecision(120, 90), 'none')
  })

  it('shows on a real upward flick', () => {
    assert.equal(navScrollDecision(80, 140), 'show')
    assert.equal(navScrollDecision(200, 210), 'none')
  })
})
