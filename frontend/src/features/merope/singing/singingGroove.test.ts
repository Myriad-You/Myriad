import assert from 'node:assert/strict'
import test from 'node:test'
import { completeBehaviorQuality } from '../anime25drig/behaviorMotion'
import {
  MIN_SINGING_NOD_INTERVAL_SECONDS,
  SingingGrooveController,
  singingNodBeatStride,
  singingSpectrumDrive,
} from './singingGroove'

test('maps bass to beat and mids to vocal without letting kick own the voice', () => {
  const kick = singingSpectrumDrive([1, 0, 0, 0, 0, 0, 0, 0])
  assert.ok(kick.beat > 0.5)
  assert.equal(kick.vocal, 0)
  const voice = singingSpectrumDrive([0, 1, 1, 0.4, 0, 0, 0, 0])
  assert.ok(voice.vocal > 0.5)
})

test('groove realizes rhythmic body motion without continuous face or gaze signals', () => {
  const controller = new SingingGrooveController()
  const pose = controller.sample(1, true, { bass: 1, beat: 1, vocal: 1 })
  assert.equal(pose.eyeX, 0)
  assert.equal(pose.brow, 0)
})

test('behavior quality reaches groove response, density, and asymmetry', () => {
  const restrained = new SingingGrooveController()
  const open = new SingingGrooveController()
  const drive = { bass: 0.4, beat: 0.4, vocal: 0.5 }
  const restrainedQuality = completeBehaviorQuality({
    tempo: 0.8,
    fluidity: 1.2,
    directness: 1.1,
    rebound: 0.1,
    asymmetry: 0.05,
    density: 0.3,
  })
  const openQuality = completeBehaviorQuality({
    tempo: 1.2,
    fluidity: 0.7,
    directness: 0.55,
    rebound: 1.1,
    asymmetry: 1.2,
    density: 1.4,
  })
  let restrainedRange = 0
  let openRange = 0
  for (let frame = 0; frame <= 60 * 8; frame += 1) {
    const time = frame / 60
    const restrainedPose = restrained.sample(
      time,
      true,
      drive,
      restrainedQuality,
    )
    const openPose = open.sample(time, true, drive, openQuality)
    if (frame < 90) continue
    restrainedRange = Math.max(
      restrainedRange,
      Math.abs(restrainedPose.angleZ) + Math.abs(restrainedPose.body),
    )
    openRange = Math.max(
      openRange,
      Math.abs(openPose.angleZ) + Math.abs(openPose.body),
    )
  }
  assert.ok(openRange > restrainedRange)
})

test('reads faster music in half-time instead of nodding on every beat', () => {
  assert.equal(singingNodBeatStride(72), 1)
  assert.equal(singingNodBeatStride(120), 2)
  assert.equal(singingNodBeatStride(180), 4)
  for (const bpm of [60, 72, 86, 100, 120, 150, 180, 200]) {
    const nodInterval = (60 / bpm) * singingNodBeatStride(bpm)
    assert.ok(nodInterval >= MIN_SINGING_NOD_INTERVAL_SECONDS - 1e-9)
  }
})

test('a brief disable keeps the leaned pose instead of yanking back to center', () => {
  const controller = new SingingGrooveController()
  const drive = { bass: 0.4, beat: 0.4, vocal: 0.5 }
  let pose = controller.sample(0, true, drive)
  for (let step = 1; step <= 60 * 4; step += 1) {
    pose = controller.sample(step / 60, true, drive)
  }
  const leaned = { ...pose }
  assert.ok(Math.abs(leaned.angleZ) > 0.04)
  const held = { ...controller.sample(4 + 1 / 60, false, null) }
  assert.ok(Math.abs(held.angleZ) > Math.abs(leaned.angleZ) * 0.85)
  const resumed = { ...controller.sample(4 + 2 / 60, true, drive) }
  assert.ok(Math.abs(resumed.angleZ) > 0.03)
  assert.equal(Math.sign(resumed.angleZ), Math.sign(leaned.angleZ))
})

test('turning singing off eases the lean back instead of collapsing it', () => {
  const controller = new SingingGrooveController()
  const drive = { bass: 0.4, beat: 0.4, vocal: 0.5 }
  let pose = controller.sample(0, true, drive)
  let start = 0
  for (let step = 1; step <= 60 * 12; step += 1) {
    pose = controller.sample(step / 60, true, drive)
    if (Math.abs(pose.angleZ) > 0.06) {
      start = step / 60
      break
    }
  }
  const leaned = { ...pose }
  assert.ok(Math.abs(leaned.angleZ) > 0.06)

  const first = { ...controller.sample(start + 1 / 60, false, null) }
  assert.ok(Math.abs(first.angleZ) > Math.abs(leaned.angleZ) * 0.9)

  let afterOne = first
  for (let frame = 2; frame <= 60; frame += 1) {
    afterOne = { ...controller.sample(start + frame / 60, false, null) }
  }
  assert.ok(Math.abs(afterOne.angleZ) > 0.015)

  let afterFour = afterOne
  for (let frame = 61; frame <= 60 * 4; frame += 1) {
    afterFour = { ...controller.sample(start + frame / 60, false, null) }
  }
  assert.ok(Math.abs(afterFour.angleZ) < 0.025)
})

test('weight keeps drifting both ways without parking', () => {
  const controller = new SingingGrooveController()
  const drive = { bass: 0.4, beat: 0.4, vocal: 0.5 }
  let minZ = 0
  let maxZ = 0
  let minBody = 0
  let maxBody = 0
  let longestStill = 0
  let stillRun = 0
  let previous = 0
  let previousDelta = 0
  let harshReversals = 0
  for (let step = 0; step <= 60 * 20; step += 1) {
    const pose = controller.sample(step / 60, true, drive)
    minZ = Math.min(minZ, pose.angleZ)
    maxZ = Math.max(maxZ, pose.angleZ)
    minBody = Math.min(minBody, pose.body)
    maxBody = Math.max(maxBody, pose.body)
    const delta = pose.angleZ - previous
    if (step > 90 && Math.abs(delta) < 0.00035) {
      stillRun += 1
      longestStill = Math.max(longestStill, stillRun)
    } else {
      stillRun = 0
    }
    if (
      step > 90 &&
      previousDelta !== 0 &&
      Math.sign(delta) !== 0 &&
      Math.sign(delta) !== Math.sign(previousDelta) &&
      Math.abs(delta) > 0.0028 &&
      Math.abs(previousDelta) > 0.0028
    ) {
      harshReversals += 1
    }
    previousDelta = delta
    previous = pose.angleZ
  }
  assert.ok(minZ < -0.04)
  assert.ok(maxZ > 0.04)
  assert.ok(minBody < -0.04)
  assert.ok(maxBody > 0.04)
  assert.ok(longestStill < 80)
  assert.equal(harshReversals, 0)
})

test('leaning out drops the head and returning to center lifts it', () => {
  const controller = new SingingGrooveController()
  const drive = { bass: 0.4, beat: 0.5, vocal: 0.6 }
  let edgeSum = 0
  let edgeCount = 0
  let centerSum = 0
  let centerCount = 0
  for (let step = 0; step <= 60 * 20; step += 1) {
    const pose = controller.sample(step / 60, true, drive)
    assert.ok(Math.abs(pose.angleY) <= 1)
    const span = Math.abs(pose.angleZ)
    if (span > 0.08) {
      edgeSum += pose.angleY
      edgeCount += 1
    } else if (step > 180 && span < 0.04) {
      centerSum += pose.angleY
      centerCount += 1
    }
  }
  assert.ok(edgeCount > 20)
  assert.ok(centerCount > 10)
  assert.ok(edgeSum / edgeCount < centerSum / centerCount)
})

function pulseBeat(time: number, amount: number): number {
  const wave = Math.max(0, Math.sin(time * Math.PI * 4))
  return 0.08 + amount * wave * wave
}

test('kick-heavy mix nods down more than a vocal phrase', () => {
  const kickCtl = new SingingGrooveController()
  const voiceCtl = new SingingGrooveController()
  let kickSum = 0
  let voiceSum = 0
  let kickMin = 0
  let count = 0
  for (let step = 0; step <= 60 * 8; step += 1) {
    const time = step / 60
    const hit = pulseBeat(time, 0.88)
    const down = kickCtl.sample(time, true, {
      bass: hit,
      beat: hit,
      vocal: 0.12,
    })
    const up = voiceCtl.sample(time, true, {
      bass: 0.08,
      beat: 0.1,
      vocal: 0.9,
    })
    if (step <= 90) continue
    kickSum += down.angleY
    voiceSum += up.angleY
    kickMin = Math.min(kickMin, down.angleY)
    count += 1
  }
  assert.ok(count > 0)
  assert.ok(kickSum / count < voiceSum / count - 0.08)
  assert.ok(voiceSum / count > 0.08)
  assert.ok(kickMin < -0.12)
  assert.ok(kickMin > -0.27, 'the downbeat no longer makes the head dive')
})

test('a loud beat dips deeper than a soft beat', () => {
  const loudCtl = new SingingGrooveController()
  const softCtl = new SingingGrooveController()
  let loudMin = 0
  let softMin = 0
  for (let step = 0; step <= 60 * 8; step += 1) {
    const time = step / 60
    const heavy = loudCtl.sample(time, true, {
      bass: pulseBeat(time, 0.86),
      beat: pulseBeat(time, 0.86),
      vocal: 0.3,
    })
    const light = softCtl.sample(time, true, {
      bass: pulseBeat(time, 0.18),
      beat: pulseBeat(time, 0.18),
      vocal: 0.3,
    })
    if (step <= 90) continue
    loudMin = Math.min(loudMin, heavy.angleY)
    softMin = Math.min(softMin, light.angleY)
  }
  assert.ok(loudMin < softMin - 0.05)
  assert.ok(loudMin > -0.13)
})

test('a pulsing beat nods down then comes back up', () => {
  const pulseCtl = new SingingGrooveController()
  const flatCtl = new SingingGrooveController()
  let pulseMin = 0
  let pulseMax = -1
  let flatMin = 0
  let flatMax = -1
  for (let step = 0; step <= 60 * 8; step += 1) {
    const time = step / 60
    const beat = pulseBeat(time, 0.82)
    const pulse = pulseCtl.sample(time, true, {
      bass: beat,
      beat,
      vocal: 0.35,
    })
    const flat = flatCtl.sample(time, true, {
      bass: 0.5,
      beat: 0.5,
      vocal: 0.35,
    })
    if (step <= 90) continue
    pulseMin = Math.min(pulseMin, pulse.angleY)
    pulseMax = Math.max(pulseMax, pulse.angleY)
    flatMin = Math.min(flatMin, flat.angleY)
    flatMax = Math.max(flatMax, flat.angleY)
  }
  assert.ok(pulseMin < -0.045)
  assert.ok(pulseMin > -0.1)
  assert.ok(pulseMax > 0.08)
  assert.ok(pulseMax - pulseMin > flatMax - flatMin + 0.055)
})

// The point of the beat clock: with a tempo to lock to, the accent leaves
// before the hit instead of chasing it.
test('a locked tempo pulls the nod earlier than an unlocked one', () => {
  function meanLag(steady: boolean): number {
    const controller = new SingingGrooveController()
    const samples: { t: number; y: number; onset: boolean }[] = []
    let seed = 3
    let nextOnset = 0
    for (let step = 0; step <= 60 * 14; step += 1) {
      const t = step / 60
      let onset = false
      if (t >= nextOnset) {
        onset = true
        seed = (seed * 1103515245 + 12345) % 2147483648
        nextOnset = t + (steady ? 0.5 : 0.22 + (seed / 2147483648) * 0.55)
      }
      const bass = onset ? 0.95 : 0.05
      const pose = controller.sample(t, true, { bass, beat: bass, vocal: 0.35 })
      samples.push({ t, y: pose.angleY, onset })
    }
    const lags: number[] = []
    for (let i = 0; i < samples.length; i += 1) {
      if (!samples[i]!.onset || samples[i]!.t < 6) continue
      let lowest = Infinity
      let at = 0
      for (
        let j = Math.max(0, i - 15);
        j < i + 18 && j < samples.length;
        j += 1
      ) {
        if (samples[j]!.y < lowest) {
          lowest = samples[j]!.y
          at = samples[j]!.t
        }
      }
      lags.push(at - samples[i]!.t)
    }
    return lags.reduce((sum, lag) => sum + lag, 0) / lags.length
  }
  assert.ok(meanLag(true) < meanLag(false))
})
