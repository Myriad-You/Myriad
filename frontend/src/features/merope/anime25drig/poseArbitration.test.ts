import type { MotionChannelPolicy } from '../motion/policy'
import type { PoseGate } from './poseArbitration'
import type { PoseOccupancy } from './poseOccupancy'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { IDLE_MOTION_POLICY } from '../motion/policy'
import { completeBehaviorQuality } from './behaviorMotion'
import {
  applyBehaviorMotionGate,
  behaviorMotionScale,
  PoseGateController,
  resolvePoseGate,
  UNOWNED_POSE_KEEP,
} from './poseArbitration'

const busy: PoseOccupancy = {
  glance: 1,
  random: 1,
  coSpeech: 1,
  groove: 1,
  thinking: 1,
  speechMouth: 1,
  grooveMouth: 1,
}

const unscaled = { performance: 1, stylized: 1, randomAmbient: 1 }

function policy(over: Partial<MotionChannelPolicy> = {}): MotionChannelPolicy {
  return { ...IDLE_MOTION_POLICY, ...over }
}

test('an idle rig gives every local source its full occupancy', () => {
  const gate = resolvePoseGate(policy(), busy, unscaled)
  assert.equal(gate.ambient.gaze, 1)
  assert.equal(gate.random.headBody, 1)
  assert.equal(gate.thinking.expression, 1)
  assert.equal(gate.stylized.expression, UNOWNED_POSE_KEEP)
  assert.equal(gate.coSpeech.expression, 1)
  assert.equal(gate.coSpeech.headBody, 1)
})

// This is the whole point: before the arbiter, the lease table described the
// rig without governing it, and every source wrote at full occupancy no matter
// who held the channel.
test('losing a channel attenuates a source only on that channel', () => {
  const gate = resolvePoseGate(
    policy({ expression: 'performance' }),
    busy,
    unscaled,
  )
  assert.equal(gate.random.expression, UNOWNED_POSE_KEEP)
  assert.equal(gate.random.gaze, 1)
  assert.equal(gate.random.headBody, 1)
  assert.equal(gate.coSpeech.expression, UNOWNED_POSE_KEEP)
  assert.equal(gate.coSpeech.headBody, 1)
})

// Losing the lease means stop competing, not stop breathing — occupancy's own
// rule is that weights tilt rather than exclusive-zero a living source.
test('an unowned source is attenuated rather than silenced', () => {
  const gate = resolvePoseGate(
    policy({
      expression: 'performance',
      gaze: 'performance',
      headBody: 'music',
    }),
    busy,
    unscaled,
  )
  for (const weight of [gate.ambient, gate.random, gate.thinking]) {
    assert.ok(weight.gaze > 0)
    assert.ok(weight.headBody > 0)
    assert.ok(weight.expression > 0)
  }
})

test('the workbench is the one owner that takes a channel outright', () => {
  const gate = resolvePoseGate(policy({ gaze: 'preview' }), busy, unscaled)
  assert.equal(gate.ambient.gaze, 0)
  assert.equal(gate.random.gaze, 0)
  assert.equal(gate.thinking.gaze, 0)
  assert.equal(gate.performance.gaze, 0)
  assert.equal(gate.stylized.gaze, 1)
  assert.equal(gate.ambient.headBody, 1)
})

test('the groove never impersonates a facial or gaze reaction', () => {
  const unleased = resolvePoseGate(policy(), busy, unscaled)
  assert.equal(unleased.groove.gaze, 0)
  assert.equal(unleased.groove.expression, 0)

  const singing = resolvePoseGate(
    policy({
      mouth: 'music',
      headBody: 'music',
      gaze: 'music',
      expression: 'music',
    }),
    busy,
    unscaled,
  )
  assert.equal(singing.groove.gaze, 0)
  assert.equal(singing.groove.expression, 0)
  assert.equal(singing.groove.headBody, 1)
})

test('scales multiply into the idle sources, not into music or speech', () => {
  const gate = resolvePoseGate(policy(), busy, {
    performance: 0.5,
    stylized: 0.5,
    randomAmbient: 0.5,
  })
  assert.ok(Math.abs(gate.ambient.gaze - 0.125) < 1e-9)
  assert.ok(Math.abs(gate.random.gaze - 0.25) < 1e-9)
  assert.equal(gate.coSpeech.expression, 1)

  // Director attention and sticker holds quiet the rig's own idling; they are
  // not a volume knob on the song or the voice.
  const singing = resolvePoseGate(policy({ headBody: 'music' }), busy, {
    performance: 0.5,
    stylized: 0.5,
    randomAmbient: 0.5,
  })
  assert.equal(singing.groove.headBody, 1)
})

// idleBreath writes the head's three axes and the torso, and the director's
// own plan writes the eyes. Both used to sit outside the arbiter, so a
// "unified" composition still had two sources writing at full weight.
test('breath and the director answer to the same leases as everything else', () => {
  const music = resolvePoseGate(policy({ headBody: 'music' }), busy, unscaled)
  assert.equal(music.ambient.headBody, UNOWNED_POSE_KEEP)

  const directed = resolvePoseGate(
    policy({ gaze: 'performance', expression: 'performance' }),
    busy,
    unscaled,
  )
  assert.equal(directed.performance.gaze, 1)
  assert.equal(directed.performance.expression, 1)
  assert.equal(directed.ambient.gaze, UNOWNED_POSE_KEEP)

  // With no plan running the director has no claim on the eyes either.
  const idle = resolvePoseGate(policy(), busy, unscaled)
  assert.equal(idle.performance.gaze, UNOWNED_POSE_KEEP)
})

test('an ownership handoff preserves a continuous mix instead of stepping', () => {
  const controller = new PoseGateController()
  controller.sample(1 / 60, resolvePoseGate(policy(), busy, unscaled))
  const target = resolvePoseGate(policy({ headBody: 'music' }), busy, unscaled)
  const first = controller.sample(1 / 60, target)
  assert.ok(first.groove.headBody > UNOWNED_POSE_KEEP)
  assert.ok(first.groove.headBody < 1)
})

test('critically damped gate handoff is stable across render rates', () => {
  const sample = (fps: number): number => {
    const controller = new PoseGateController()
    controller.sample(1 / fps, resolvePoseGate(policy(), busy, unscaled))
    const target = resolvePoseGate(
      policy({ headBody: 'music' }),
      busy,
      unscaled,
    )
    for (let frame = 0; frame < fps / 5; frame += 1) {
      controller.sample(1 / fps, target)
    }
    return controller.sample(0, target).groove.headBody
  }
  const values = [sample(30), sample(60), sample(120)]
  assert.ok(Math.max(...values) - Math.min(...values) < 1e-9)
})

test('the new owner is mostly present within eighty milliseconds', () => {
  const controller = new PoseGateController()
  controller.sample(0, resolvePoseGate(policy(), busy, unscaled))
  const target = resolvePoseGate(policy({ headBody: 'music' }), busy, unscaled)
  let current = controller.sample(1 / 60, target)
  for (let frame = 2; frame <= 5; frame += 1) {
    current = controller.sample(1 / 60, target)
  }
  assert.ok(current.groove.headBody > 0.9)
  assert.ok(current.groove.headBody < 1)
})

test('a missing behavior unit leaves the occupancy gate alone, never zeroes it', () => {
  const quality = completeBehaviorQuality(undefined)
  const speaking: PoseGate = fullGate()
  applyBehaviorMotionGate(speaking, {
    coSpeech: 0,
    coSpeechPower: 0,
    coSpeechQuality: quality,
    music: 0,
    musicPower: 0,
    musicQuality: quality,
  })
  // Talking with no realized co-speech behavior must still move the face: a
  // moving mouth on a frozen body is the failure, not the fallback.
  assert.equal(speaking.coSpeech.expression, 1)
  assert.equal(speaking.coSpeech.headBody, 1)
  assert.equal(speaking.groove.headBody, 1)

  const withUnit: PoseGate = fullGate()
  applyBehaviorMotionGate(withUnit, {
    coSpeech: 0.5,
    coSpeechPower: 1,
    coSpeechQuality: quality,
    music: 0,
    musicPower: 0,
    musicQuality: quality,
  })
  assert.equal(withUnit.coSpeech.expression, 0.5)
  assert.ok(withUnit.coSpeech.expression < speaking.coSpeech.expression)
})

test('behaviorMotionScale modulates but never inverts a live unit', () => {
  assert.equal(behaviorMotionScale(0, 0), 1)
  assert.equal(behaviorMotionScale(1, 1), 1)
  assert.ok(behaviorMotionScale(0.2, 0) < 1)
})

test('the gate stays a weight, so no strength is silently clamped away', () => {
  // The compositor treats these as weights and clamps them to [0, 1]. A
  // ceiling above that is not a boost, it is a number the next stage throws
  // away — which is what 1.55 was doing.
  const compositor = readFileSync(
    new URL('./poseCompositor.ts', import.meta.url),
    'utf8',
  )
  assert.match(compositor, /Math\.max\(0, Math\.min\(1, value\)\)/)
  for (const extent of [0.35, 1, 1.96, 2.31, 4]) {
    for (const power of [0, 0.35, 1, 1.4]) {
      const scale = behaviorMotionScale(extent, power)
      assert.ok(scale <= 1, `${extent}/${power} -> ${scale}`)
      assert.ok(scale > 0, `${extent}/${power} -> ${scale}`)
    }
  }
  // A unit at full authored strength opens its channel and stops there.
  assert.equal(behaviorMotionScale(1.4, 1.4), 1)
})

function fullGate(): PoseGate {
  const weights = () => ({ mouth: 1, expression: 1, gaze: 1, headBody: 1 })
  return {
    ambient: weights(),
    coSpeech: weights(),
    groove: weights(),
    performance: weights(),
    randomAmbient: weights(),
    stylized: weights(),
    thinking: weights(),
  } as unknown as PoseGate
}
