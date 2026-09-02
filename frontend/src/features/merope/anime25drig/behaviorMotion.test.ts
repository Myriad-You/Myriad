import type { Anime25DMotionUnit } from './behaviorMotion'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  Anime25DBehaviorMotionController,
  completeBehaviorQuality,
} from './behaviorMotion'

function unit(
  family: Anime25DMotionUnit['family'] = 'co-speech',
): Anime25DMotionUnit {
  return {
    behaviorId: `${family}:unit`,
    family,
    form: family === 'music' ? 'listen' : 'accent',
    kind: family === 'music' ? 'rhythmic' : 'oneShot',
    timing: {
      startMs: 1_100,
      readyMs: 1_200,
      strokeStartMs: 1_300,
      strokePeakMs: 1_400,
      strokeEndMs: 1_500,
      relaxMs: 1_600,
      endMs: 1_800,
    },
    intensity: 1,
    quality: completeBehaviorQuality({ extent: 1.2, power: 1.1 }),
  }
}

test('samples seven-stage motion on the player clock and prunes at end', () => {
  const controller = new Anime25DBehaviorMotionController()
  controller.replace([unit()], 1_000, 5)
  assert.equal(controller.sample(5.05).coSpeech, 0)
  const preparing = controller.sample(5.25).coSpeech
  const peak = controller.sample(5.4).coSpeech
  assert.ok(preparing > 0)
  assert.ok(peak > preparing)
  assert.equal(controller.sample(5.8).coSpeech, 0)
  assert.equal(controller.sample(6).coSpeech, 0)
})

test('sustained rhythm remains active until explicitly replaced', () => {
  const controller = new Anime25DBehaviorMotionController()
  const sustained = unit('music')
  sustained.timing.relaxMs = null
  sustained.timing.endMs = null
  controller.replace([sustained], 1_000, 2)
  assert.ok(controller.sample(4).music > 0.7)
  controller.clear()
  assert.equal(controller.sample(4.1).music, 0)
})

test('replacement preserves wall-clock age across different player origins', () => {
  const controller = new Anime25DBehaviorMotionController()
  controller.replace([unit()], 1_350, 9)
  const nearPeak = controller.sample(9.05).coSpeech
  assert.ok(nearPeak > 0.8)
})

test('tempo, fluidity, rebound, and density shape the scheduled envelope', () => {
  const fastUnit = unit()
  fastUnit.quality = completeBehaviorQuality({
    tempo: 1.6,
    fluidity: 0.35,
    directness: 1.2,
    rebound: 1.2,
    density: 1.4,
  })
  const slowUnit = unit()
  slowUnit.quality = completeBehaviorQuality({
    tempo: 0.5,
    fluidity: 1.3,
    directness: 0.3,
    rebound: 0,
    density: 0.3,
  })
  const fast = new Anime25DBehaviorMotionController()
  const slow = new Anime25DBehaviorMotionController()
  fast.replace([fastUnit], 1_000, 5)
  slow.replace([slowUnit], 1_000, 5)

  assert.ok(fast.sample(5.15).coSpeech > slow.sample(5.15).coSpeech)
  assert.ok(fast.sample(5.525).coSpeech > slow.sample(5.525).coSpeech)
  assert.equal(fast.sample(5.55).coSpeechQuality.rebound, 1.2)
})

test('quality completion bounds untrusted planner values', () => {
  assert.deepEqual(
    completeBehaviorQuality({
      extent: 99,
      tempo: -2,
      power: Number.NaN,
      fluidity: 8,
      directness: -1,
      rebound: 4,
      asymmetry: -3,
      density: 7,
    }),
    {
      extent: 1.6,
      tempo: 0.45,
      power: 1,
      fluidity: 1.4,
      directness: 0.2,
      rebound: 1.4,
      asymmetry: 0,
      density: 1.5,
    },
  )
})
