import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  navigateAfterStageLeave,
  setStageLeaveHandler,
} from './stageLeaveGate.ts'

describe('navigateAfterStageLeave', () => {
  it('lets the caller navigate when no stage is open', () => {
    setStageLeaveHandler(null)
    assert.equal(navigateAfterStageLeave(() => {}), false)
  })

  it('goes to the last target clicked while the stage is leaving', () => {
    let finishLeave: (() => void) | null = null
    let leaves = 0
    setStageLeaveHandler((proceed) => {
      leaves++
      finishLeave = proceed
      // 真实 handler 关舞台后会注销自己。
      setStageLeaveHandler(null)
    })

    const went: string[] = []
    assert.equal(navigateAfterStageLeave(() => went.push('/library')), true)
    assert.equal(navigateAfterStageLeave(() => went.push('/journal')), true)
    assert.equal(navigateAfterStageLeave(() => went.push('/tapp')), true)
    assert.equal(leaves, 1)
    assert.deepEqual(went, [])

    finishLeave!()
    assert.deepEqual(went, ['/tapp'])

    // 退完后回到放行状态。
    assert.equal(navigateAfterStageLeave(() => {}), false)
  })
})
