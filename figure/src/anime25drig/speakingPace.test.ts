import assert from 'node:assert/strict'
import test from 'node:test'
import { IDLE_MOTION_POLICY } from '../motion/policy'
import { ClosedEyePresentation } from './closedEyePresentation'
import { IDENTITY_DRIVER } from './driver'
import { Anime25DIrisRebound } from './irisRebound'
import { Anime25DMotionComposer } from './motionComposer'
import { deriveAnime25DMotionEnvelopeProfile } from './motionEnvelope'
import { headRollRadians } from './poseScale'

const HZ = 60
const SECONDS = 120

/**
 * Two minutes of the production composer, her head roll in degrees.
 * `Math.random` is seeded for the run so the figures are the same each time.
 */
function headRoll(speaking: boolean, seed: number): number[] {
  const random = Math.random
  let state = seed >>> 0
  Math.random = () => {
    state = (state * 1_664_525 + 1_013_904_223) >>> 0
    return state / 0x1_0000_0000
  }
  try {
    const current = { ...IDENTITY_DRIVER }
    const target = { ...IDENTITY_DRIVER, talk: speaking, blink: true, rand: true }
    const composer = new Anime25DMotionComposer(current, new ClosedEyePresentation(), new Anime25DIrisRebound())
    const envelope = deriveAnime25DMotionEnvelopeProfile({ layers: [{ role: 'handwear' }] })
    if (speaking) composer.speechMotion.enqueueText('诶？你说那家店啊，我上次路过的时候还在排队呢。不过说真的，我觉得他们家的拉面没有传说中那么好吃。', 'zh-CN')
    const roll: number[] = []
    for (let frame = 1; frame <= HZ * SECONDS; frame += 1) {
      composer.step(1 / HZ, { time: frame / HZ, target, policy: IDLE_MOTION_POLICY, mouse: { x: 0, y: 0, inside: false },
        speechActive: false, musicSignal: null, motionEnvelopeProfile: envelope })
      if (frame > HZ * 2) roll.push((headRollRadians(current.angleZ) * 180) / Math.PI)
    }
    return roll
  } finally {
    Math.random = random
  }
}

/** Range between the 5th and 95th percentile. */
function range(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b)
  return sorted[Math.floor(sorted.length * 0.95)] - sorted[Math.floor(sorted.length * 0.05)]
}

/** Times a minute the head starts tilting again after it had settled (over 3°/s, smoothed). */
function posesPerMinute(roll: number[]): number {
  const window = HZ / 15
  const smooth = roll.map((_, i) => {
    const slice = roll.slice(Math.max(0, i - window / 2), i + window / 2)
    return slice.reduce((a, b) => a + b, 0) / slice.length
  })
  let moves = 0
  let moving = false
  for (let i = 1; i < smooth.length; i += 1) {
    const now = Math.abs(smooth[i] - smooth[i - 1]) * HZ > 3
    if (now && !moving) moves += 1
    moving = now
  }
  return (moves / (smooth.length / HZ)) * 60
}

// People talking on camera (three interviews, eye line tracked frame by frame)
// tilt their heads over 12–16° within ten seconds, in visible moves of 2–4°
// many times a minute. Hers was fitted to them (about 10° over two minutes);
// before that she held 5–6°, and before ec853f944 she drifted over 2.5°. The
// bounds keep her a talker, not a statue or a jitter.
test('while she talks her head changes pose at a talking person\'s pace and range', () => {
  for (const seed of [3, 5]) {
    const roll = headRoll(true, seed)
    const tilt = range(roll)
    const perMinute = posesPerMinute(roll)
    assert.ok(tilt > 7 && tilt < 16, `seed ${seed}: tilt range ${tilt.toFixed(1)}°`)
    assert.ok(perMinute > 35 && perMinute < 90, `seed ${seed}: ${perMinute.toFixed(0)} poses a minute`)
  }
})

test('at rest she keeps her slow idle pace', () => {
  const resting = posesPerMinute(headRoll(false, 3))
  const talking = posesPerMinute(headRoll(true, 3))
  assert.ok(resting < talking / 2, `${resting.toFixed(0)} at rest vs ${talking.toFixed(0)} talking a minute`)
})
