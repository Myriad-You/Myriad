import type { PerformanceProfile } from './usePerformanceProfile'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { resolveAnimationConfig } from './useAnimationLevel'

function profile(partial: Partial<PerformanceProfile>): PerformanceProfile {
  return {
    isMobile: true,
    reduceMotion: false,
    highHardware: false,
    hardwareUncertain: false,
    os: 'android',
    hardwareConcurrency: 8,
    deviceMemory: null,
    ...partial,
  }
}

describe('resolveAnimationConfig', () => {
  it('honors an explicit high choice when the hardware tier is only a guess', () => {
    // #624：安卓 Firefox 读不到内存，选了「中高性能」却一直落在 light。
    const unknown = profile({ hardwareUncertain: true })
    assert.equal(resolveAnimationConfig('standard', unknown).level, 'standard')
    assert.equal(resolveAnimationConfig('auto', unknown).level, 'light')
    assert.equal(resolveAnimationConfig('light', unknown).level, 'exlight')
  })

  it('keeps measured low hardware off the standard tier', () => {
    const measured = profile({ deviceMemory: 4 })
    assert.equal(resolveAnimationConfig('standard', measured).level, 'light')
    assert.equal(resolveAnimationConfig('light', measured).level, 'exlight')
  })

  it('leaves capable hardware and reduced motion as they were', () => {
    const capable = profile({ highHardware: true, deviceMemory: 8 })
    assert.equal(resolveAnimationConfig('auto', capable).level, 'standard')
    assert.equal(resolveAnimationConfig('light', capable).level, 'light')
    const reduced = profile({ hardwareUncertain: true, reduceMotion: true })
    assert.equal(resolveAnimationConfig('standard', reduced).level, 'exlight')
  })
})
