import assert from 'node:assert/strict'
import test from 'node:test'
import {
  restSingingArticulation,
  sampleSingingCue,
  singingArticulation,
} from './singingClock'

const cues = [
  { start: 1, end: 1.2, viseme: 'closed' as const, emphasis: false },
  { start: 1.2, end: 1.8, viseme: 'open' as const, emphasis: true },
  { start: 2.4, end: 2.7, viseme: 'round' as const, emphasis: false },
]

test('samples the viseme covering the audio clock including seeks', () => {
  assert.equal(sampleSingingCue(cues, 0.5), null)
  assert.equal(sampleSingingCue(cues, 1.1)?.viseme, 'closed')
  assert.equal(sampleSingingCue(cues, 1.2)?.viseme, 'open')
  assert.equal(sampleSingingCue(cues, 1.79)?.viseme, 'open')
  assert.equal(sampleSingingCue(cues, 2.0), null)
  assert.equal(sampleSingingCue(cues, 2.5)?.viseme, 'round')
  assert.equal(sampleSingingCue(cues, 2.7), null)
})

test('keeps lip seals closed and rests during lyric gaps', () => {
  assert.deepEqual(
    singingArticulation({
      cue: cues[0],
      energy: 0.9,
      humming: false,
    }),
    { energy: 0.9, viseme: 'closed', amount: 1 },
  )
  assert.deepEqual(
    singingArticulation({ cue: null, energy: 0.9, humming: false }),
    restSingingArticulation(),
  )
})

test('scales voiced visemes by vocal energy without inventing shapes', () => {
  const loud = singingArticulation({
    cue: cues[1],
    energy: 1,
    humming: false,
  })
  const quiet = singingArticulation({
    cue: cues[1],
    energy: 0,
    humming: false,
  })
  assert.equal(loud.viseme, 'open')
  assert.equal(quiet.viseme, 'open')
  assert.ok(loud.amount > quiet.amount)
  assert.equal(quiet.amount, 0.28)
})

test('hums from energy only when there is no lyric timeline', () => {
  assert.deepEqual(
    singingArticulation({ cue: null, energy: 0.02, humming: true }),
    restSingingArticulation(),
  )
  const hum = singingArticulation({
    cue: null,
    energy: 0.3,
    humming: true,
  })
  assert.equal(hum.viseme, 'narrow')
  assert.ok(hum.amount > 0)
  const belt = singingArticulation({
    cue: null,
    energy: 0.8,
    humming: true,
  })
  assert.equal(belt.viseme, 'open')
})
