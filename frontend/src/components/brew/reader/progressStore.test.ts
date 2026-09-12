import assert from 'node:assert/strict'
import { it } from 'node:test'
import { clampProgress, createReadingProgress } from './progressStore'

it('ignores same-percent writes and notifies only on change', () => {
  const progress = createReadingProgress(10)
  const seen: number[] = []
  const stop = progress.subscribe(() => seen.push(progress.get()))
  progress.set(10)
  progress.set(10.4)
  progress.set(11)
  progress.set(200)
  progress.set(-3)
  stop()
  progress.set(40)
  assert.deepEqual(seen, [11, 100, 0])
  assert.equal(clampProgress(Number.NaN), 0)
})
