import type { TouchSample } from './touchGesture'
import assert from 'node:assert/strict'
import test from 'node:test'
import { TouchGestureTracker } from './touchGesture'

function sample(atMs: number,
  x = 0,
  region: TouchSample['region'] = 'hair',
  pointerId = 1): TouchSample {
  return { atMs, x, y: 0, region, pointerId }
}

test('empty background does not start contact; press is immediate and release becomes a tap', () => {
  const tracker = new TouchGestureTracker()
  assert.equal(tracker.begin(sample(0, 0, null)), null)
  const start = tracker.begin(sample(10))!
  assert.equal(start.gesture, 'contact')
  const end = tracker.end(sample(100))!
  assert.equal(end.gesture, 'tap')
  assert.equal(start.id, end.id)
  assert.equal(end.repeatCount, 1)
  assert.equal(tracker.end(sample(110)), null)
})

test('stationary contact becomes hold using supplied time, without pointermove counts', () => {
  const tracker = new TouchGestureTracker()
  tracker.begin(sample(0))
  assert.equal(tracker.update(sample(399))?.gesture, 'contact')
  assert.equal(tracker.update(sample(400))?.gesture, 'hold')
  assert.equal(tracker.end(sample(5000))?.gesture, 'hold')
})

test('recent completed taps remain evidence during the next press and hold, but not across regions or pauses', () => {
  const tracker = new TouchGestureTracker()
  for (let i = 0; i < 3; i++) {
    tracker.begin(sample(i * 200, 0, 'face'))
    tracker.end(sample(i * 200 + 80, 0, 'face'))
  }
  assert.equal(tracker.begin(sample(600, 0, 'face'))?.repeatCount, 3)
  assert.equal(tracker.update(sample(1200, 0, 'face'))?.repeatCount, 3)
  assert.equal(tracker.update(sample(1250, 0, 'hair'))?.repeatCount, 0)
  tracker.end(sample(1300, 0, 'hair'))
  tracker.begin(sample(1500, 0, 'face'))
  tracker.end(sample(1580, 0, 'face'))
  assert.equal(tracker.begin(sample(2500, 0, 'face'))?.repeatCount, 0)
  tracker.reset(2600)
})

test('linear strokes are sampling-rate independent and keep the same contact identity', () => {
  for (const hz of [30, 60, 120, 240]) {
    const tracker = new TouchGestureTracker()
    const start = tracker.begin(sample(0))!
    for (let i = 1; i < hz; i++) {
      const observation = tracker.update(sample((i * 1000) / hz, i / hz))!
      assert.equal(observation.id, start.id)
    }
    const end = tracker.end(sample(1000, 1))!
    assert.equal(end.gesture, 'stroke')
    assert.ok(Math.abs(end.distance - 1) < 1e-10)
    assert.ok(Math.abs(end.speed - (1 - Math.exp(-1000 / 120))) < 1e-10)
  }
})

test('crossing regions updates one contact; leaving the character cancels and does not reenter automatically', () => {
  const tracker = new TouchGestureTracker()
  const id = tracker.begin(sample(0))!.id
  const face = tracker.update(sample(200, 0.2, 'face'))!
  assert.equal(face.id, id)
  assert.equal(face.region, 'face')
  assert.equal(tracker.update(sample(300, 0.3, null))?.phase, 'cancel')
  assert.equal(tracker.update(sample(400, 0.4)), null)
})

test('another pointer, invalid samples and stale timestamps cannot change the active contact', () => {
  const tracker = new TouchGestureTracker()
  tracker.begin(sample(100))
  assert.equal(tracker.begin(sample(101, 0, 'face', 2)), null)
  for (const invalid of [
    sample(99),
    sample(110, NaN),
    sample(Infinity),
    sample(110, 0, 'face', 2),
  ]) {
    assert.equal(tracker.update(invalid), null)
    assert.equal(tracker.end(invalid), null)
  }
  assert.equal(tracker.update(sample(100, 100)), null)
  assert.equal(tracker.end(sample(200))?.distance, 0)
})

test('repeat taps are region- and time-bounded; reset clears them without reusing identity', () => {
  const tracker = new TouchGestureTracker()
  let previousId = 0
  for (let i = 0; i < 12; i++) {
    const start = tracker.begin(sample(i * 200))!
    assert.ok(start.id > previousId)
    previousId = start.id
    assert.equal(
      tracker.end(sample(i * 200 + 100))?.repeatCount,
      Math.min(8, i + 1),
    )
  }
  tracker.begin(sample(2500, 0, 'face'))
  assert.equal(tracker.end(sample(2600, 0, 'face'))?.repeatCount, 1)
  tracker.begin(sample(4000, 0, 'face'))
  assert.equal(tracker.end(sample(4100, 0, 'face'))?.repeatCount, 1)
  tracker.reset(4150)
  tracker.begin(sample(4200, 0, 'face'))
  assert.equal(tracker.end(sample(4300, 0, 'face'))?.repeatCount, 1)
})

test('cancellation is idempotent and late release cannot create a tap', () => {
  const tracker = new TouchGestureTracker()
  tracker.begin(sample(10))
  assert.equal(tracker.cancel(100)?.phase, 'cancel')
  assert.equal(tracker.cancel(200), null)
  assert.equal(tracker.end(sample(300)), null)
})

test('small pointer tremor does not turn a stationary hold into repeated strokes', () => {
  const tracker = new TouchGestureTracker()
  tracker.begin(sample(0))
  for (let i = 1; i <= 1000; i++)
    tracker.update(sample(i * 8, i % 2 ? 0.001 : 0))
  const end = tracker.end(sample(8010))!
  assert.ok(end.distance > 0.12)
  assert.equal(end.gesture, 'hold')
})

test('returning along a stroke is not mistaken for a tap with matching endpoints', () => {
  const tracker = new TouchGestureTracker()
  tracker.begin(sample(0))
  tracker.update(sample(150, 0.2))
  const end = tracker.end(sample(300, 0))!
  assert.equal(end.gesture, 'stroke')
  assert.equal(end.distance, 0.4)
})
