import assert from 'node:assert/strict'
import test from 'node:test'
import { TranscriptionQueue } from './transcriptionQueue'

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (error: unknown) => void
  const promise = new Promise<T>((yes, no) => {
    resolve = yes
    reject = no
  })
  return { promise, resolve, reject }
}

const tick = () => new Promise((resolve) => setImmediate(resolve))

test('recognition runs two requests ahead but commits in spoken order', async () => {
  const results: string[] = []
  const busy: boolean[] = []
  const first = deferred<string>()
  const second = deferred<string>()
  let thirdStarted = false
  const queue = new TranscriptionQueue<string>({
    onResult: (text) => results.push(text),
    onError: assert.fail,
    onBusy: (value) => busy.push(value),
  })
  queue.enqueue(() => first.promise)
  queue.enqueue(() => second.promise)
  queue.enqueue(async () => {
    thirdStarted = true
    return 'third'
  })
  assert.equal(thirdStarted, false)
  second.resolve('second')
  await tick()
  assert.deepEqual(results, [])
  assert.equal(thirdStarted, true)
  first.resolve('first')
  await tick()
  assert.deepEqual(results, ['first', 'second', 'third'])
  assert.equal(busy.at(-1), false)
})

test('stopping a listener rejects late results without blocking the next listener', async () => {
  const results: string[] = []
  const busy: boolean[] = []
  const old = deferred<string>()
  let oldSignal: AbortSignal | undefined
  const queue = new TranscriptionQueue<string>({
    onResult: (text) => results.push(text),
    onError: assert.fail,
    onBusy: (value) => busy.push(value),
  })
  queue.enqueue((signal) => {
    oldSignal = signal
    return old.promise
  })
  queue.reset()
  queue.enqueue(async () => 'new')
  old.resolve('old')
  await tick()
  assert.equal(oldSignal!.aborted, true)
  assert.deepEqual(results, ['new'])
  assert.equal(busy.at(-1), false)
})

test('an earlier recognition failure does not strand later results', async () => {
  const results: string[] = []
  const errors: unknown[] = []
  const old = deferred<string>()
  const queue = new TranscriptionQueue<string>({
    onResult: (text) => results.push(text),
    onError: (error) => errors.push(error),
    onBusy() {},
  })
  queue.enqueue(() => old.promise)
  queue.enqueue(async () => 'second')
  await tick()
  old.reject(new Error('provider unavailable'))
  await tick()
  assert.equal(errors.length, 1)
  assert.deepEqual(results, ['second'])
})
