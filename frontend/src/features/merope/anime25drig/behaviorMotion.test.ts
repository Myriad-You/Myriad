import type { Anime25DMotionUnit } from './behaviorMotion'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  Anime25DBehaviorMotionController,
  completeBehaviorQuality,
} from './behaviorMotion'
import { predictedControlTime } from './motionPrediction'

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
  controller.clear(4)
  // Stopping the plan is not teardown. This used to assert an immediate zero,
  // which is the hard cut the paired expression controller never made: it has
  // always released its cues from their current value.
  assert.ok(controller.sample(4.02).music > 0)
  assert.equal(controller.sample(4.5).music, 0)
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

test('a unit that leaves the plan retreats from the level it was drawn at', () => {
  const controller = new Anime25DBehaviorMotionController()
  controller.replace([unit()], 1_000, 5)
  const held = controller.sample(5.55).coSpeech
  assert.ok(held > 0.5)

  controller.replace([], 1_550, 5.55)
  const retreat = [5.56, 5.6, 5.64].map((at) => controller.sample(at).coSpeech)
  // The first retreating frame continues from what was on screen. Stepping
  // straight to zero dropped a half-finished gesture on the frame the plan
  // changed, while the face eased out of the very same beat.
  assert.ok(
    Math.abs(retreat[0]! - held) < held * 0.35,
    `retreat starts at ${retreat[0]}, drawn level was ${held}`,
  )
  for (let index = 1; index < retreat.length; index += 1) {
    assert.ok(retreat[index]! < retreat[index - 1]!)
  }
  assert.equal(controller.sample(6.2).coSpeech, 0)
})

test('restating a plan does not restart a retreat', () => {
  const controller = new Anime25DBehaviorMotionController()
  controller.replace([unit()], 1_000, 5)
  controller.sample(5.55)
  controller.replace([], 1_550, 5.55)
  const first = controller.sample(5.62).coSpeech

  // A plan revision arrives on nearly every frame while speech streams. Each
  // one is the same single release, not permission to start it over.
  controller.replace([], 1_620, 5.62)
  const second = controller.sample(5.64).coSpeech
  assert.ok(second < first, `${second} should keep falling below ${first}`)
})

test('a returning behavior plays again instead of finishing its retreat', () => {
  const controller = new Anime25DBehaviorMotionController()
  controller.replace([unit()], 1_000, 5)
  controller.sample(5.55)
  controller.replace([], 1_550, 5.55)
  controller.sample(5.58)
  controller.replace([unit()], 1_580, 5.58)
  assert.ok(controller.sample(5.6).coSpeech > 0.5)
})

test('a retreat loses one frame to the read clock, not one whole lead', () => {
  // The player writes on its own clock and reads one `predictedControlTime`
  // ahead of it. Starting a retreat on the write clock put the entire lead
  // into the fade before its first frame: a short retreat arrived already
  // 42% gone, which is a snap toward rest rather than a retreat from it.
  const controller = new Anime25DBehaviorMotionController()
  const quick = {
    ...unit(),
    quality: completeBehaviorQuality({ extent: 0.4, tempo: 1.7 }),
  }
  let at = 5
  controller.replace([quick], 1_000, at)
  for (let frame = 0; frame < 20; frame += 1) {
    at += 1 / 60
    controller.sample(predictedControlTime(at))
  }
  const drawn = controller.sample(predictedControlTime(at)).coSpeech
  assert.ok(drawn > 0.2)

  controller.replace([], 1_000 + (at - 5) * 1_000, at)
  at += 1 / 60
  const next = controller.sample(predictedControlTime(at)).coSpeech
  assert.ok(next > drawn * 0.8, `retreat opened at ${next} from ${drawn}`)
  assert.ok(next < drawn)
})

test('a preparing speech revision starts at the drawn shape on the prediction read clock', () => {
  const controller = new Anime25DBehaviorMotionController()
  const original = { ...unit(), form: 'question' }
  controller.replace([original], 1_000, 5)
  const readAt = predictedControlTime(5.2)
  const before = { ...controller.sample(readAt).coSpeechGesture }
  const extent = controller.sample(readAt).coSpeech
  const revised = { ...original, form: 'hesitate' }
  controller.replace([revised], 1_200, 5.2)
  assert.deepEqual(controller.sample(readAt).coSpeechGesture, before)
  assert.equal(controller.sample(readAt).coSpeech, extent)
  const arriving = { ...controller.sample(readAt + 0.03).coSpeechGesture }
  assert.ok(arriving.question > 0 && arriving.hesitate > 0)
  // Streaming republishes may continue during the arrival; never restart it.
  controller.replace([revised], 1_230, 5.23)
  const restated = controller.sample(readAt + 0.03).coSpeechGesture
  for (const form of ['question', 'hesitate'] as const) {
    assert.ok(Math.abs(restated[form] - arriving[form]) < 1e-12)
  }
  assert.equal(controller.sample(5.4).coSpeechGesture.hesitate, 1)
  const reference = new Anime25DBehaviorMotionController()
  reference.replace([original], 1_000, 5)
  assert.equal(controller.sample(5.4).coSpeech, reference.sample(5.4).coSpeech)
})

test('cancelling a shape revision releases what was drawn, without finishing the new pose', () => {
  const controller = new Anime25DBehaviorMotionController()
  const original = { ...unit(), form: 'question' }
  controller.replace([original], 1_000, 5)
  controller.sample(5.2)
  controller.replace([{ ...original, form: 'tease' }], 1_200, 5.2)
  const drawn = { ...controller.sample(5.24).coSpeechGesture }
  controller.clear(5.24)
  assert.deepEqual(controller.sample(5.26).coSpeechGesture, drawn)
  assert.ok(controller.sample(5.26).coSpeech > 0)
  controller.replace([], 1_280, 5.28)
  assert.equal(controller.sample(6).coSpeech, 0)
  assert.equal(controller.sample(6).coSpeechGesture.tease, 0)
})

test('cancellation between a restatement and the next frame keeps the last drawn level', () => {
  for (const family of ['co-speech', 'music'] as const) {
    const controller = new Anime25DBehaviorMotionController()
    const original = unit(family)
    controller.replace([original], 1_000, 5)
    const drawn = controller.sample(5.35)
    const extent = family === 'music' ? drawn.music : drawn.coSpeech
    controller.replace([original], 1_350, 5.35)
    // No render sample occurs between these two events.
    controller.clear(5.35)
    const releasing = controller.sample(5.35)
    assert.equal(
      family === 'music' ? releasing.music : releasing.coSpeech,
      extent,
    )
    const ended = controller.sample(6)
    assert.equal(family === 'music' ? ended.music : ended.coSpeech, 0)
  }
})

test('an unseen future beat adopts its correction directly, with no obsolete pose or extra delay', () => {
  const controller = new Anime25DBehaviorMotionController()
  const original = { ...unit(), form: 'question' }
  controller.replace([original], 1_000, 5)
  assert.equal(controller.sample(5.05).coSpeech, 0)
  controller.replace([{ ...original, form: 'hesitate' }], 1_050, 5.05)
  const first = controller.sample(5.11)
  assert.ok(first.coSpeech > 0)
  assert.equal(first.coSpeechGesture.hesitate, 1)
  assert.equal(first.coSpeechGesture.question, 0)
})
