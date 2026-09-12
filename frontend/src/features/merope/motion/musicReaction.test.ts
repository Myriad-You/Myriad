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

test('music riding the participation line is not a decision twice a bar', () => {
  const planner = new MusicReactionPlanner()
  planner.reset('threshold-track')
  const modes: string[] = []
  for (let frame = 0; frame < 120; frame += 1) {
    const beat = ((frame % 30) / 30) * Math.PI * 2
    modes.push(
      planner.sample(
        musicSignalAt(frame / 60, {
          audio: {
            energy: 0.12 + 0.02 * Math.sin(beat),
            bass: 0.4,
            pulse: 0.4,
            presence: 0.4,
          },
        }),
        frame * 16.7,
        false,
        false,
      ),
    )
  }
  const flips = modes.filter(
    (mode, index) => index > 0 && mode !== modes[index - 1],
  ).length
  assert.equal(flips, 0, `participation changed ${flips} times in two seconds`)
})

test('a sustained drop still becomes listening', () => {
  const planner = new MusicReactionPlanner()
  planner.reset('fading-track')
  for (let frame = 0; frame < 40; frame += 1) {
    planner.sample(musicSignalAt(frame / 60), frame * 16.7, false, false)
  }
  const quiet = { energy: 0.06, bass: 0.1, pulse: 0.1, presence: 0.1 }
  const held = Array.from({ length: 90 }, (_, frame) =>
    planner.sample(
      musicSignalAt(1 + frame / 60, { audio: quiet }),
      (40 + frame) * 16.7,
      false,
      false,
    ),
  )
  assert.equal(held.at(-1), 'listen')
})
