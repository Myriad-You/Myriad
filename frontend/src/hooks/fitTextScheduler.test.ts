import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { FIT_TEXT_SLICE_MS, scheduleFitText } from './fitTextScheduler'

describe('scheduleFitText', () => {
  it('keeps the slice budget at 8ms', () => {
    assert.equal(FIT_TEXT_SLICE_MS, 8)
  })

  it('runs several nodes in one frame', () => {
    const frames: FrameRequestCallback[] = []
    const raf = (cb: FrameRequestCallback) => {
      frames.push(cb)
      return frames.length
    }
    const savedRaf = globalThis.requestAnimationFrame
    const savedCaf = globalThis.cancelAnimationFrame
    globalThis.requestAnimationFrame = raf as typeof requestAnimationFrame
    globalThis.cancelAnimationFrame = (() => {}) as typeof cancelAnimationFrame
    try {
      const ran: string[] = []
      const a = { isConnected: true } as HTMLElement
      const b = { isConnected: true } as HTMLElement
      scheduleFitText(a, () => ran.push('a'))
      scheduleFitText(b, () => ran.push('b'))
      assert.deepEqual(ran, [])
      const queued = frames.splice(0)
      for (const cb of queued) cb(0)
      assert.deepEqual(ran, ['a', 'b'])
    } finally {
      globalThis.requestAnimationFrame = savedRaf
      globalThis.cancelAnimationFrame = savedCaf
    }
  })
})
