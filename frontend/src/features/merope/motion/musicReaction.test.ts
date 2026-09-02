import assert from 'node:assert/strict'
import test from 'node:test'
import { musicSignalAt } from '../singing/musicSignal.test-support'
import { MusicReactionPlanner } from './musicReaction'

test('listening, phrase preparation, release, and stilling are distinct decisions', () => {
  const planner = new MusicReactionPlanner()
  const phrase = { start: 2, end: 4, confidence: 0.95 }
  assert.equal(planner.sample(musicSignalAt(1), 1000, false, true), 'listen')
  assert.equal(
    planner.sample(musicSignalAt(1.8, { phrase }), 1800, false, true),
    'sing',
  )
  assert.equal(
    planner.sample(musicSignalAt(4.2, { phrase }), 4200, false, true),
    'listen',
  )
  const quiet = musicSignalAt(5, {
    audio: { energy: 0, bass: 0, pulse: 0, presence: 0 },
  })
  assert.equal(planner.sample(quiet, 5000, false, true), 'listen')
  assert.equal(planner.sample(quiet, 5400, false, true), 'settle')
  assert.equal(planner.sample(musicSignalAt(5.5), 5500, false, true), 'listen')
})

test('unavailable analysis does not impersonate silence, singing evidence, or a beat', () => {
  const planner = new MusicReactionPlanner()
  for (let t = 0; t < 20; t++) {
    assert.equal(
      planner.sample(
        musicSignalAt(t, { audio: null, bpm: 0 }),
        t * 1000,
        false,
        false,
      ),
      'listen',
    )
}
})

test('humming alternates with listening in reproducible non-fixed bouts', () => {
  const planner = new MusicReactionPlanner()
  planner.reset('song-a')
  const transitions: number[] = []
  let last = ''
  for (let t = 0; t < 60; t += 0.1) {
    const mode = planner.sample(musicSignalAt(t), t * 1000, false, false)
    if (mode !== last) transitions.push(t)
    last = mode
  }
  assert.ok(transitions.length > 5)
  assert.ok(
    new Set(
      transitions.slice(1).map((t, i) => Math.round((t - transitions[i]) * 10)),
    ).size > 3,
  )
})
