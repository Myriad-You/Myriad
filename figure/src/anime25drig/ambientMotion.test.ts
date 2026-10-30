import assert from 'node:assert/strict'
import test from 'node:test'
import {
  AMBIENT_HEAD_GAZE_SHARE,
  AmbientMotionController,
  EYE_SACCADE_FLOOR_SECONDS,
  EYE_SACCADE_SECONDS_PER_UNIT,
} from './ambientMotion'

function magnitude(pose: {
  angleX: number
  angleY: number
  angleZ: number
  body: number
  eyeX: number
  eyeY: number
}): number {
  return Math.max(...Object.values(pose).map(Math.abs))
}

test('starts neutral and waits before selecting its first ambient pose', () => {
  const motion = new AmbientMotionController(() => 0.5)
  assert.equal(magnitude(motion.sample(0, true)), 0)
  assert.equal(magnitude(motion.sample(1.04, true)), 0)
  assert.ok(magnitude(motion.sample(1.06, true)) < 0.001)
  assert.ok(magnitude(motion.sample(1.4, true)) > 0)
})

test('moves the gaze before the head and the body', () => {
  const motion = new AmbientMotionController(() => 0.8)
  motion.sample(0, true)
  const early = { ...motion.sample(1.39, true) }
  const eyeTravel = Math.max(Math.abs(early.eyeX), Math.abs(early.eyeY))
  const headTravel = Math.max(
    Math.abs(early.angleX),
    Math.abs(early.angleY),
    Math.abs(early.angleZ),
  )
  assert.ok(eyeTravel > headTravel)
  assert.ok(Math.abs(early.body) < headTravel)
})

test('uses broad head/body range without exceeding rig space or jumping frames', () => {
  let seed = 0x1234_5678
  const random = () => {
    seed = (seed * 1_664_525 + 1_013_904_223) >>> 0
    return seed / 0x1_0000_0000
  }
  const motion = new AmbientMotionController(random)
  let previous = { ...motion.sample(0, true) }
  let largestHeadStep = 0
  let yaw = 0
  let pitch = 0
  let body = 0
  for (let frame = 1; frame <= 60 * 600; frame += 1) {
    const current = { ...motion.sample(frame / 60, true) }
    largestHeadStep = Math.max(
      largestHeadStep,
      Math.abs(current.angleX - previous.angleX),
      Math.abs(current.angleY - previous.angleY),
      Math.abs(current.angleZ - previous.angleZ),
    )
    yaw = Math.max(yaw, Math.abs(current.angleX))
    pitch = Math.max(pitch, Math.abs(current.angleY))
    body = Math.max(body, Math.abs(current.body))
    for (const value of Object.values(current)) assert.ok(Math.abs(value) <= 1)
    previous = current
  }
  assert.ok(largestHeadStep < 0.06)
  assert.ok(yaw > 0.85, `yaw ${yaw}`)
  assert.ok(pitch > 0.5, `pitch ${pitch}`)
  assert.ok(body > 0.6, `body ${body}`)
})

test('releases an active random pose smoothly when automation is disabled', () => {
  const motion = new AmbientMotionController(() => 0.5)
  motion.sample(0, true)
  motion.sample(1.81, true)
  const active = { ...motion.sample(2.8, true) }
  const releaseStart = { ...motion.sample(2.8, false) }
  assert.deepEqual(releaseStart, active)
  assert.ok(magnitude(motion.sample(3.15, false)) < magnitude(active))
  assert.ok(magnitude(motion.sample(3.7, false)) < 1e-8)
})

test('re-enabling after speech does not snap into a new glance', () => {
  const motion = new AmbientMotionController(() => 0.5)
  motion.sample(0, true)
  motion.sample(1.81, true)
  const active = { ...motion.sample(2.8, true) }
  motion.sample(2.8, false)
  const resumed = { ...motion.sample(3.5, true) }
  assert.ok(magnitude(resumed) < magnitude(active))
  let previous = resumed
  let largestHeadStep = 0
  for (let frame = 1; frame <= 90; frame += 1) {
    const current = { ...motion.sample(3.5 + frame / 60, true) }
    largestHeadStep = Math.max(
      largestHeadStep,
      Math.abs(current.angleX - previous.angleX),
      Math.abs(current.angleY - previous.angleY),
      Math.abs(current.angleZ - previous.angleZ),
    )
    previous = current
  }
  assert.ok(largestHeadStep < 0.02)
})

test('keeps time-based transitions consistent at 30 and 60 fps', () => {
  const at30 = new AmbientMotionController(() => 0.5)
  const at60 = new AmbientMotionController(() => 0.5)
  let pose30 = { ...at30.sample(0, true) }
  let pose60 = { ...at60.sample(0, true) }
  for (let frame = 1; frame <= 30 * 12; frame += 1) {
    pose30 = { ...at30.sample(frame / 30, true) }
  }
  for (let frame = 1; frame <= 60 * 12; frame += 1) {
    pose60 = { ...at60.sample(frame / 60, true) }
  }
  for (const key of Object.keys(pose30) as Array<keyof typeof pose30>) {
    assert.ok(Math.abs(pose30[key] - pose60[key]) < 0.01)
  }
})

test('keeps the gaze target stable while the head catches up', () => {
  const motion = new AmbientMotionController(() => 0.8)
  motion.sample(0, true)
  const early = { ...motion.sample(1.65, true) }
  const late = { ...motion.sample(2.4, true) }
  assert.ok(Math.abs(late.angleX) > Math.abs(early.angleX) + 0.1)
  const worldX = (pose: typeof early) =>
    pose.eyeX + pose.angleX * AMBIENT_HEAD_GAZE_SHARE.x
  assert.ok(Math.abs(worldX(early) - worldX(late)) < 1e-10)
})

test('scanpaths differ across seeds but not render frequencies and do not keep returning to zero', () => {
  const run = (seed: number, fps: number) => {
    let draws = 0
    const motion = new AmbientMotionController(() => {
      draws += 1
      seed = (Math.imul(seed, 1_664_525) + 1_013_904_223) >>> 0
      return seed / 0x1_0000_0000
    })
    let neutral = 0
    let positive = 0
    let negative = 0
    const samples = []
    for (let frame = 0; frame <= fps * 90; frame += 1) {
      const pose = motion.sample(frame / fps, true)
      if (frame % fps !== 0) continue
      if (frame > fps * 2 && magnitude(pose) < 0.01) neutral += 1
      if (pose.angleX > 0.4) positive += 1
      if (pose.angleX < -0.4) negative += 1
      samples.push({ ...pose })
    }
    return { samples, draws, neutral, positive, negative }
  }
  const a = run(42, 30)
  assert.deepEqual(a, run(42, 60))
  assert.deepEqual(a, run(42, 120))
  assert.notDeepEqual(a.samples, run(43, 60).samples)
  assert.ok(a.positive > 2 && a.negative > 2)
  assert.ok(a.neutral < 5)
})

test('a small glance is given long enough to look like a glance', () => {
  const duration = (travel: number) =>
    EYE_SACCADE_FLOOR_SECONDS +
    Math.min(1.8, travel) * EYE_SACCADE_SECONDS_PER_UNIT

  assert.ok(
    EYE_SACCADE_FLOOR_SECONDS >= 0.15,
    `the smallest glance gets ${EYE_SACCADE_FLOOR_SECONDS}s, which is what made them read as flicks`,
  )
  assert.ok(
    duration(1.8) <= 0.27,
    `a full sweep now takes ${duration(1.8)}s against 0.264s before`,
  )
  assert.ok(duration(0.15) < duration(0.65))
  assert.ok(duration(0.65) < duration(1.8))
})

test('a held look keeps settling; the head never parks between looks', () => {
  let seed = 0x2468_ACE0
  const random = () => {
    seed = (seed * 1_664_525 + 1_013_904_223) >>> 0
    return seed / 0x1_0000_0000
  }
  const motion = new AmbientMotionController(random)
  let previous = { ...motion.sample(0, true) }
  let parked = 0
  let longestParked = 0
  for (let frame = 1; frame <= 60 * 300; frame += 1) {
    const current = { ...motion.sample(frame / 60, true) }
    const moved = Math.max(
      Math.abs(current.angleX - previous.angleX),
      Math.abs(current.angleY - previous.angleY),
      Math.abs(current.angleZ - previous.angleZ),
      Math.abs(current.body - previous.body),
    )
    parked = frame > 120 && moved < 2e-5 ? parked + 1 : 0
    longestParked = Math.max(longestParked, parked)
    previous = current
  }
  assert.ok(longestParked / 60 < 0.3, `parked for ${longestParked / 60}s`)
})

test('while she talks her looks come quicker and each tilts her head; at rest the pace is unchanged', () => {
  const run = (speaking: boolean) => {
    let seed = 0x2468_ACE0
    const motion = new AmbientMotionController(() => {
      seed = (seed * 1_664_525 + 1_013_904_223) >>> 0
      return seed / 0x1_0000_0000
    })
    let tiltTurns = 0
    let previousSpeed = 0
    let previous = motion.sample(0, true, speaking).angleZ
    let tilt = 0
    for (let frame = 1; frame <= 60 * 120; frame += 1) {
      const z = motion.sample(frame / 60, true, speaking).angleZ
      const speed = Math.abs(z - previous) * 60
      // A tilt begins when the head starts rolling again after it had settled.
      if (speed > 0.15 && previousSpeed <= 0.15) tiltTurns += 1
      tilt = Math.max(tilt, Math.abs(z))
      previousSpeed = speed
      previous = z
    }
    return { perMinute: tiltTurns / 2, tilt }
  }
  const idle = run(false)
  const talking = run(true)
  assert.ok(talking.perMinute > idle.perMinute * 2, `${talking.perMinute} vs ${idle.perMinute}`)
  assert.ok(talking.tilt > idle.tilt, `${talking.tilt} vs ${idle.tilt}`)
  assert.ok(talking.tilt <= 1)
})

test('she looks away as she starts to speak and back at them as she finishes', () => {
  let seed = 0x1357_9BDF
  const motion = new AmbientMotionController(() => {
    seed = (seed * 1_664_525 + 1_013_904_223) >>> 0
    return seed / 0x1_0000_0000
  })
  const gaze = (pose: { eyeX: number, eyeY: number, angleX: number, angleY: number }) => ({
    x: pose.eyeX + pose.angleX * AMBIENT_HEAD_GAZE_SHARE.x,
    y: pose.eyeY + pose.angleY * AMBIENT_HEAD_GAZE_SHARE.y,
  })
  let t = 0
  for (; t < 5; t += 1 / 60) motion.sample(t, true, false)
  for (const end = t + 0.7; t < end; t += 1 / 60) motion.sample(t, true, true)
  const away = gaze(motion.sample(t, true, true))
  assert.ok(Math.abs(away.x) > 0.3 && away.y < -0.05, `away at the start: ${away.x.toFixed(2)}, ${away.y.toFixed(2)}`)
  for (const end = t + 4; t < end; t += 1 / 60) motion.sample(t, true, true)
  for (const end = t + 0.8; t < end; t += 1 / 60) motion.sample(t, true, false)
  const back = gaze(motion.sample(t, true, false))
  assert.ok(Math.abs(back.x) < 0.1 && Math.abs(back.y) < 0.1, `back at them at the end: ${back.x.toFixed(2)}, ${back.y.toFixed(2)}`)
  for (const end = t + 0.6; t < end; t += 1 / 60) motion.sample(t, true, false)
  const held = gaze(motion.sample(t, true, false))
  assert.ok(Math.abs(held.x) < 0.1 && Math.abs(held.y) < 0.1, 'and holds it a moment')
})
