import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  decideHomeBandMorph,
  homeGridGeometryMotion,
  lockedHomeGridColumns,
  shouldHardCutHomeBand,
} from './widgetGridBand'

describe('shouldHardCutHomeBand', () => {
  it('hard-cuts first paint, exlight, and any phone-column edge', () => {
    assert.equal(
      shouldHardCutHomeBand({
        hardCut: false,
        settledOnce: true,
        from: 8,
        desired: 16,
      }),
      false,
    )
    assert.equal(
      shouldHardCutHomeBand({
        hardCut: true,
        settledOnce: true,
        from: 8,
        desired: 16,
      }),
      true,
    )
    assert.equal(
      shouldHardCutHomeBand({
        hardCut: false,
        settledOnce: false,
        from: 8,
        desired: 16,
      }),
      true,
    )
    assert.equal(
      shouldHardCutHomeBand({
        hardCut: false,
        settledOnce: true,
        from: 4,
        desired: 8,
      }),
      true,
    )
    assert.equal(
      shouldHardCutHomeBand({
        hardCut: false,
        settledOnce: true,
        from: 16,
        desired: 4,
      }),
      true,
    )
  })
})

describe('decideHomeBandMorph', () => {
  it('noops when columns already match and marks settle', () => {
    assert.deepEqual(
      decideHomeBandMorph({
        desired: 16,
        previous: 16,
        switching: false,
        settledOnce: false,
        hardCut: false,
      }),
      { type: 'already-current' },
    )
  })

  it('does not start a second morph while switching', () => {
    assert.deepEqual(
      decideHomeBandMorph({
        desired: 16,
        previous: 8,
        switching: true,
        settledOnce: true,
        hardCut: false,
      }),
      { type: 'busy' },
    )
  })

  it('hard-applies phone or first-paint changes', () => {
    assert.deepEqual(
      decideHomeBandMorph({
        desired: 4,
        previous: 16,
        switching: false,
        settledOnce: true,
        hardCut: false,
      }),
      { type: 'hard-apply', columns: 4 },
    )
  })

  it('fades 8↔16 only after the grid has settled once', () => {
    assert.deepEqual(
      decideHomeBandMorph({
        desired: 16,
        previous: 8,
        switching: false,
        settledOnce: true,
        hardCut: false,
      }),
      { type: 'fade-out' },
    )
  })
})

describe('lockedHomeGridColumns', () => {
  it('locks free layout and custom columns before any morph', () => {
    assert.deepEqual(lockedHomeGridColumns({ isFreeLayout: true }), {
      locked: true,
    })
    assert.deepEqual(
      lockedHomeGridColumns({ isFreeLayout: false, customGridColumns: 12 }),
      { locked: true, columns: 12 },
    )
    assert.deepEqual(lockedHomeGridColumns({ isFreeLayout: false }), {
      locked: false,
    })
  })
})

describe('homeGridGeometryMotion', () => {
  it('keeps geometry easing off across a band fade or layout-mode swap', () => {
    assert.equal(
      homeGridGeometryMotion({
        exlight: false,
        bandSwitch: null,
        motionMode: 'standard',
        layoutMode: 'standard',
      }),
      true,
    )
    assert.equal(
      homeGridGeometryMotion({
        exlight: false,
        bandSwitch: 'out',
        motionMode: 'standard',
        layoutMode: 'standard',
      }),
      false,
    )
    assert.equal(
      homeGridGeometryMotion({
        exlight: false,
        bandSwitch: null,
        motionMode: 'standard',
        layoutMode: 'free',
      }),
      false,
    )
    assert.equal(
      homeGridGeometryMotion({
        exlight: true,
        bandSwitch: null,
        motionMode: 'standard',
        layoutMode: 'standard',
      }),
      false,
    )
  })
})
