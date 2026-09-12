import assert from 'node:assert/strict'
import test from 'node:test'
import { BeatClock, MAX_BEAT_PERIOD } from './beatClock'

function play(
  clock: BeatClock,
  bpm: number,
  seconds: number,
  startAt = 0,
): ReturnType<BeatClock['sample']> {
  const period = 60 / bpm
  const step = 1 / 60
  let frame = clock.sample(startAt, 0, true)
  for (let t = startAt; t < startAt + seconds; t += step) {
    const intoBeat = (t - startAt) % period
    const bass = intoBeat < 2 * step ? 0.9 : Math.max(0, 0.3 - intoBeat)
    frame = clock.sample(t, bass, true)
  }
  return frame
}

test('locks onto a steady tempo and reports it in bpm', () => {
  const clock = new BeatClock()
  const frame = play(clock, 120, 12)
  assert.ok(Math.abs(frame.bpm - 120) < 6, `got ${frame.bpm}`)
  assert.ok(frame.confidence > 0.7, `confidence ${frame.confidence}`)
})

test('tracks a different tempo without carrying the old one over', () => {
  const slow = play(new BeatClock(), 80, 12)
  assert.ok(Math.abs(slow.bpm - 80) < 6, `got ${slow.bpm}`)
  const fast = play(new BeatClock(), 150, 12)
  assert.ok(Math.abs(fast.bpm - 150) < 8, `got ${fast.bpm}`)
})

test('counts pulses without claiming to know the meter or bar downbeat', () => {
  const clock = new BeatClock()
  const frame = play(clock, 120, 12)
  assert.ok(frame.beatCount >= 18, `counted ${frame.beatCount}`)
  assert.equal(Object.hasOwn(frame, 'barPhase'), false)
  assert.ok(frame.beatPhase >= 0 && frame.beatPhase < 1)
})

// Silence is not a slow tempo.
test('noise never earns confidence', () => {
  const clock = new BeatClock()
  let random = 7
  let frame = clock.sample(0, 0, true)
  for (let t = 0; t < 12; t += 1 / 60) {
    random = (random * 1103515245 + 12345) % 2147483648
    frame = clock.sample(t, (random / 2147483648) * 0.9, true)
  }
  assert.ok(frame.confidence < 0.55, `confidence ${frame.confidence}`)
})

test('an interval outside the musical range is not a tempo', () => {
  const clock = new BeatClock()
  let frame = clock.sample(0, 0, true)
  for (let t = 0; t < 30; t += 1 / 60) {
    frame = clock.sample(t, t % 3 < 2 / 60 ? 0.95 : 0, true)
  }
  assert.equal(frame.bpm, 0)
  assert.equal(frame.confidence, 0)
  assert.ok(60 / MAX_BEAT_PERIOD >= 60)
})

test('stopping the music drops the tempo instead of freezing it', () => {
  const clock = new BeatClock()
  play(clock, 120, 12)
  const stopped = clock.sample(20, 0, false)
  assert.equal(stopped.bpm, 0)
  assert.equal(stopped.confidence, 0)
})

test('a seek invalidates the old phase rather than counting skipped beats', () => {
  const clock = new BeatClock()
  play(clock, 120, 12)
  assert.equal(clock.sample(2, 0, true).confidence, 0)
  play(clock, 120, 12, 2)
  const forward = clock.sample(90, 0, true)
  assert.equal(forward.confidence, 0)
  assert.equal(forward.beatCount, 0)
})

test('a track switch expires the old tempo before the next song starts', () => {
  const clock = new BeatClock()
  const locked = play(clock, 120, 12)
  assert.ok(Math.abs(locked.bpm - 120) < 6)
  assert.ok(locked.confidence > 0.7)

  let quiet = locked
  for (let t = 12; t < 24; t += 1 / 60) {
    quiet = clock.sample(t, 0, true)
  }
  assert.equal(quiet.bpm, 0)
  assert.equal(quiet.confidence, 0)

  const next = play(clock, 150, 12, 24)
  assert.ok(Math.abs(next.bpm - 150) < 8, `carried over: ${next.bpm}`)
})

test('a short quiet patch loosens the lock instead of cutting the groove', () => {
  const clock = new BeatClock()
  const locked = play(clock, 120, 12)
  let brief = locked
  for (let t = 12; t < 13; t += 1 / 60) brief = clock.sample(t, 0, true)
  assert.ok(brief.confidence > 0)
  assert.ok(brief.confidence <= locked.confidence)
  assert.ok(brief.bpm > 0)
})

test('changing track identity clears tempo evidence immediately', () => {
  const clock = new BeatClock()
  clock.setTrack('song-a')
  const locked = play(clock, 120, 12)
  assert.ok(locked.confidence > 0.7)

  clock.setTrack('song-b')
  const switched = clock.sample(12.05, 0, true)
  assert.equal(switched.bpm, 0)
  assert.equal(switched.confidence, 0)
  assert.equal(switched.beatCount, 0)

  // Re-publishing metadata for the same media item must not reset the lock.
  const next = play(clock, 150, 12, 12.05)
  clock.setTrack('song-b')
  assert.equal(clock.sample(24.06, 0, true).bpm, next.bpm)
})
