import assert from 'node:assert/strict'
import test from 'node:test'
import { IDENTITY_DRIVER } from './driver'
import { Anime25DPoseInertia } from './poseInertia'

const FPS = 60

function follow(inertia: Anime25DPoseInertia, targets: Partial<typeof IDENTITY_DRIVER>, seconds: number) {
  const frames: Array<typeof IDENTITY_DRIVER> = []
  for (let i = 0; i < seconds * FPS; i += 1) {
    const pose = { ...IDENTITY_DRIVER, ...targets }
    inertia.step(pose, 1 / FPS)
    frames.push(pose)
  }
  return frames
}

test('a jump in the composed pose becomes a carried movement that arrives', () => {
  const inertia = new Anime25DPoseInertia()
  follow(inertia, {}, 0.1)
  // A cue or a hand-off that tilts the head 22° at once.
  const frames = follow(inertia, { angleZ: 0.75, angleY: -0.5 }, 1)
  const roll = frames.map((pose) => pose.angleZ)
  const fastest = Math.max(...roll.slice(1).map((value, i) => Math.abs(value - roll[i]!) * FPS))
  assert.ok(fastest <= 6 + 1e-9, `roll speed ${fastest} units/s stays under the cap`)
  assert.ok(roll[0]! <= 0.1 + 1e-9, 'not there at once')
  assert.ok(Math.abs(roll.at(-1)! - 0.75) < 0.01 && Math.abs(frames.at(-1)!.angleY + 0.5) < 0.01, 'but it gets there')
})

test('a held pose passes through untouched, and only head and torso are carried', () => {
  const inertia = new Anime25DPoseInertia()
  follow(inertia, { angleX: 0.4 }, 0.1)
  const [first] = follow(inertia, { angleX: 0.4, eyeX: 1, mouthOpen: 1 }, 1 / FPS)
  assert.equal(first!.angleX, 0.4)
  assert.equal(first!.eyeX, 1)
  assert.equal(first!.mouthOpen, 1)
})
