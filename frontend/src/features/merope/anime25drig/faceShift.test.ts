import type { BodyControl } from '../../../services/agent/types'
import type { Anime25DDriver } from './driver'
import assert from 'node:assert/strict'
import test from 'node:test'
import { BODY_CONTROLS, FACE_PRESETS, tierExtent } from '../events/performanceContract'
import { DirectedPoseController } from './directedPose'
import { IDENTITY_DRIVER } from './driver'

const FPS = 60

function controller(): DirectedPoseController {
  const value = new DirectedPoseController()
  value.setCapabilities([...new Set(Object.values(BODY_CONTROLS).map((control) => control.capability))])
  return value
}

function look(face: string) {
  const targets = Object.fromEntries(
    Object.entries(FACE_PRESETS[face].targets).map(([control, tier]) => [control, tierExtent(control as BodyControl, tier)!]),
  )
  return { targets, transitionMs: 350, holdMs: 0 }
}

/** Plays from `from` to `to` seconds, the base driver still each frame; the composed drivers per frame. */
function play(pose: DirectedPoseController, from: number, to: number): Anime25DDriver[] {
  const frames: Anime25DDriver[] = []
  for (let i = 1; i <= Math.round((to - from) * FPS); i += 1) {
    const target = { ...IDENTITY_DRIVER }
    pose.apply(1 / FPS, from + i / FPS, target, false)
    frames.push(target)
  }
  return frames
}

test('a change of face carries on from where she is: nothing jumps, and the last look\'s life does not play on', () => {
  const random = Math.random
  Math.random = () => 0.5
  try {
    const pose = controller()
    pose.set(look('sleepy'), 0, 'sleepy', null)
    const before = play(pose, 0, 2.6)
    // Nodding off, part way down.
    assert.ok(before.at(-1)!.angleY < -0.5)
    pose.set(look('excited'), 2.6, 'excited', 'ease')
    const after = play(pose, 2.6, 4.6)
    const step = (frames: Anime25DDriver[], index: number, key: keyof Anime25DDriver) =>
      Math.abs((frames[index][key] as number) - (index ? (frames[index - 1][key] as number) : (before.at(-1)![key] as number)))
    for (const key of ['angleY', 'eyeOpenL', 'eyeWide'] as const) {
      assert.ok(step(after, 0, key) < 0.05, `${key} jumped at the change: ${step(after, 0, key)}`)
    }
    // The head comes up from the nod within a second (before the new look's own nod at 0.9 s),
    // not after the old nod would have run its course.
    const settled = after[Math.round(FPS * 0.8)]
    assert.ok(settled.angleY > -0.1, `${settled.angleY}`)
    assert.ok(settled.eyeOpenL > 0.9, `${settled.eyeOpenL}`)
  } finally {
    Math.random = random
  }
})

test('a change of face goes through her in order: the eyes first, the head after; a pause holds her first', () => {
  const started = (shift: string, control: keyof Anime25DDriver) => {
    const pose = controller()
    pose.set({ targets: {}, transitionMs: 350, holdMs: 0 }, 0, null, null)
    play(pose, 0, 0.5)
    pose.set({ targets: { gazeHorizontal: 0.8, headTurn: 0.6 }, transitionMs: 350, holdMs: 0 }, 0.5, null, shift)
    const frames = play(pose, 0.5, 2.5)
    return frames.findIndex((frame) => Math.abs(frame[control] as number) > 0.05) / FPS
  }
  assert.ok(started('ease', 'eyeX') < started('ease', 'angleX'))
  assert.ok(started('pause', 'eyeX') > started('ease', 'eyeX') + 0.25)
  assert.ok(started('snap', 'angleX') < started('slow', 'angleX'))
})

test('a change may pass through looks on the way, bending through them without stopping', () => {
  const random = Math.random
  Math.random = () => 0.5
  try {
    const pose = controller()
    pose.set(look('drowsy'), 0, 'drowsy', null)
    play(pose, 0, 1.5)
    // Dozing to delight, waking up surprised on the way: the mouth falls open and the eyes go wider than delight's.
    pose.set(look('excited'), 1.5, 'excited', 'ease', ['surprised'])
    const frames = play(pose, 1.5, 4)
    const wide = frames.map((frame) => frame.eyeWide)
    const mouth = frames.map((frame) => frame.mouthOpen)
    const peak = Math.max(...wide)
    assert.ok(peak > 0.75, `surprised on the way: ${peak}`)
    assert.ok(Math.max(...mouth) > 0.2 && mouth.at(-1)! < 0.05, `the mouth opened on the way and closed: ${Math.max(...mouth)} ${mouth.at(-1)}`)
    assert.ok(Math.abs(wide.at(-1)! - 0.65) < 0.05, `then excited: ${wide.at(-1)}`)
    // Not held there: it is near its peak only briefly.
    const near = wide.filter((value) => value > peak - 0.03).length / FPS
    assert.ok(near < 0.3, `passed, not held: ${near}s near the peak`)
    // Two looks in turn, and a newer change on the way is the one that holds.
    pose.set(look('content'), 4, 'content', 'ease', ['plain', 'surprised'])
    pose.set(look('shy'), 4.1, 'shy', 'ease')
    const after = play(pose, 4.1, 7.2)
    assert.ok(after.at(-1)!.eyeX > 0.3, 'shy, not content, at the end')
  } finally {
    Math.random = random
  }
})
