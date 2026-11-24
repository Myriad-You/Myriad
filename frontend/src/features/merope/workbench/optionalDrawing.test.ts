import assert from 'node:assert/strict'
import test from 'node:test'
import { optionalDrawing } from './optionalDrawing'

test('a drawing that fails beside the split does not throw the split away', async () => {
  const split = new File([new Uint8Array([1])], 'see-through.psd')
  const told: unknown[] = []
  const refused = new Error('the image model refused')
  const [file] = await Promise.all([
    Promise.resolve(split),
    optionalDrawing(Promise.reject(refused), (reason) => told.push(reason)),
    optionalDrawing(Promise.resolve(), (reason) => told.push(reason)),
    optionalDrawing(undefined, (reason) => told.push(reason)),
  ])
  assert.equal(file, split)
  assert.deepEqual(told, [refused], 'the failure is told once, the drawings that worked say nothing')
})

test('a split that fails is still a failure', async () => {
  const down = new Error('the Space is down')
  await assert.rejects(
    Promise.all([Promise.reject(down), optionalDrawing(Promise.resolve(), () => {})]),
    down,
  )
})
