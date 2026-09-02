import type { StylizedExpressionMotion } from './stylizedExpressionMotion'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  SILLY_IRIS_DRIFT_LIMIT,
  StylizedExpressionMotionController,
} from './stylizedExpressionMotion'

const STEP = 1 / 60
const LOOP_SECONDS = 6

test('keeps extra artwork size intrinsic to the expression envelope', () => {
  const anger = settledExpression(1, 0, 0, 0, 0)
  const speechless = settledExpression(0, 1, 0, 0, 0)
  const silly = settledExpression(0, 0, 0, 1, 0)
  const lovestruck = settledExpression(0, 0, 0, 0, 1)

  assert.ok(anger.angerMarkScale > 0.8, 'the anger mark reaches authored size')
  assert.ok(
    speechless.speechlessSweatScale > 0.9,
    'the sweat drop reaches authored size',
  )
  assert.ok(silly.sillyEyeScale > 0.9, 'the vacant eyes reach authored size')
  assert.ok(
    lovestruck.lovestruckHeartScale > 0.85,
    'heart pupils reach authored size',
  )
  assert.ok(
    lovestruck.lovestruckFaceScale > 0.9,
    'the face accent reaches authored size',
  )
})

test('drifts the two silly irides on independent, unequal paths', () => {
  const frames = runSilly(2 * LOOP_SECONDS)
  const loop = frames.slice(frames.length - Math.round(LOOP_SECONDS / STEP))

  const leftRange = range(loop.map((frame) => frame.sillyIrisOffsetXL))
  const rightRange = range(loop.map((frame) => frame.sillyIrisOffsetXR))
  assert.ok(
    leftRange > SILLY_IRIS_DRIFT_LIMIT.x * 1.5,
    'the near eye wanders far enough to read',
  )
  assert.ok(
    leftRange > rightRange * 1.4,
    'the two eyes must not travel the same distance',
  )

  const horizontalGap = Math.max(
    ...loop.map((frame) =>
      Math.abs(frame.sillyIrisOffsetXL - frame.sillyIrisOffsetXR),
    ),
  )
  assert.ok(
    horizontalGap > SILLY_IRIS_DRIFT_LIMIT.x * 0.35,
    'gaze never converges back onto one point',
  )
  assert.ok(
    loop.some((frame) => frame.sillyIrisOffsetYL * frame.sillyIrisOffsetYR < 0),
    'the two eyes drift to opposite sides of neutral at least once',
  )
})

test('keeps every iris inside the room import reserved for it', () => {
  for (const frame of runSilly(2 * LOOP_SECONDS)) {
    assert.ok(Math.abs(frame.sillyIrisOffsetXL) <= SILLY_IRIS_DRIFT_LIMIT.x)
    assert.ok(Math.abs(frame.sillyIrisOffsetXR) <= SILLY_IRIS_DRIFT_LIMIT.x)
    assert.ok(Math.abs(frame.sillyIrisOffsetYL) <= SILLY_IRIS_DRIFT_LIMIT.y)
    assert.ok(Math.abs(frame.sillyIrisOffsetYR) <= SILLY_IRIS_DRIFT_LIMIT.y)
  }
})

test('starts each vacant look somewhere else in the loop', () => {
  const controller = new StylizedExpressionMotionController()
  const first = burst(controller, 0, 1, 1)
  const quiet = burst(controller, first.time, 0, 2)
  assert.ok(quiet.sample.silly < 0.025, 'the look has to actually end')
  const second = burst(controller, quiet.time, 1, 1)
  assert.ok(
    Math.abs(first.sample.sillyMouthOpen - second.sample.sillyMouthOpen) > 0.05,
    'a replayed look must not land on the same frame of the loop',
  )
})

test('repeats the vacant loop every six seconds without a seam', () => {
  const frames = runSilly(3 * LOOP_SECONDS)
  const early = frames[Math.round(3 / STEP) - 1]
  const late = frames[Math.round(9 / STEP) - 1]
  for (const key of [
    'sillyIrisOffsetXL',
    'sillyIrisOffsetYL',
    'sillyIrisOffsetXR',
    'sillyIrisOffsetYR',
    'sillyMouthOpen',
  ] as const) {
    assert.ok(Math.abs(early[key] - late[key]) < 1e-3, key)
  }
})

test('holds the mouth open once per loop instead of chattering', () => {
  const frames = runSilly(2 * LOOP_SECONDS)
  const loop = frames.slice(frames.length - Math.round(LOOP_SECONDS / STEP))
  const opening = loop.map((frame) => frame.sillyMouthOpen)

  assert.ok(Math.min(...opening) > 0.1, 'the omega mouth never seals shut')
  assert.ok(mean(opening) < 0.35, 'the mouth stays mostly small')
  const holds = runsAbove(opening, 0.6)
  assert.equal(holds.length, 1, 'exactly one long slack-jawed pause per loop')
  assert.ok(holds[0] * STEP > 0.5, 'the pause reads as a delayed reaction')
})

test('breathes lovestruck accents without pumping the whole face', () => {
  const controller = new StylizedExpressionMotionController()
  const frames: StylizedExpressionMotion[] = []
  for (let frame = 1; frame <= 360; frame += 1) {
    frames.push({ ...controller.sample(frame * STEP, 0, 0, 0, 0, 1) })
  }
  const settled = frames.slice(120)
  const heartRange = range(settled.map((frame) => frame.lovestruckHeartScale))
  const faceRange = range(settled.map((frame) => frame.lovestruckFaceScale))
  const droolRange = range(settled.map((frame) => frame.lovestruckDroolOffsetY))
  assert.ok(heartRange > 0.1, 'heart pupils visibly breathe')
  assert.ok(faceRange < heartRange * 0.2, 'blush remains visually anchored')
  assert.ok(droolRange > 0.4, 'the drool accent remains alive')
})

test('lets the wilder face win and keeps quieter ones out of the way', () => {
  const owned = new StylizedExpressionMotionController()
  let sample: Readonly<StylizedExpressionMotion> = owned.sample(0, 1, 1, 0, 1)
  for (let frame = 1; frame <= 120; frame += 1) {
    sample = owned.sample(frame * STEP, 1, 1, 0, 1)
  }
  assert.ok(sample.silly > 0.9)
  assert.ok(sample.anger < 0.02)
  assert.ok(sample.speechless < 0.02)
  assert.ok(sample.ambientScale <= 0.35, 'idle motion nearly stops')

  const maniac = new StylizedExpressionMotionController()
  let laughing: Readonly<StylizedExpressionMotion> = maniac.sample(
    0,
    0,
    0,
    1,
    1,
  )
  for (let frame = 1; frame <= 120; frame += 1) {
    laughing = maniac.sample(frame * STEP, 0, 0, 1, 1)
  }
  assert.ok(laughing.maniac > 0.9)
  assert.ok(laughing.silly < 0.02)
})

test('pairs every settled maniac laugh burst with a visible delayed head nod', () => {
  const controller = new StylizedExpressionMotionController()
  const frames: StylizedExpressionMotion[] = []
  for (let frame = 0; frame <= 60 * 6; frame += 1) {
    frames.push({ ...controller.sample(frame * STEP, 0, 0, 1) })
  }
  const settled = frames.slice(90)
  const headPulse = settled.map((frame) => frame.maniacHeadPulse)
  const headPitch = settled.map((frame) => frame.angleY)
  const mouthPeaks = localPeaks(
    settled.map((frame) => frame.maniacUpperMouthPulse),
    0.02,
  )
  const headPeaks = localPeaks(headPulse, 0.045)

  assert.ok(range(headPulse) > 0.065, 'the laugh moves the head vertically')
  assert.ok(range(headPitch) > 0.13, 'the laugh reads primarily as a head nod')
  assert.ok(
    headPitch.slice(1).every((value, index) => {
      return Math.abs(value - headPitch[index]!) < 0.03
    }),
    'the stronger nod remains continuous frame to frame',
  )
  assert.ok(mouthPeaks.length >= 4)
  for (const mouthPeak of mouthPeaks.slice(0, -1)) {
    assert.ok(
      headPeaks.some(
        (headPeak) => headPeak > mouthPeak && headPeak - mouthPeak <= 6,
      ),
      'the head follows each mouth burst within 100ms',
    )
  }
})

function runSilly(seconds: number): StylizedExpressionMotion[] {
  const controller = new StylizedExpressionMotionController()
  const frames: StylizedExpressionMotion[] = []
  for (let frame = 1; frame <= Math.round(seconds / STEP); frame += 1) {
    frames.push({ ...controller.sample(frame * STEP, 0, 0, 0, 1) })
  }
  return frames
}

function settledExpression(
  anger: number,
  speechless: number,
  maniac: number,
  silly: number,
  lovestruck: number,
): StylizedExpressionMotion {
  const controller = new StylizedExpressionMotionController()
  let sample = {
    ...controller.sample(0, anger, speechless, maniac, silly, lovestruck),
  }
  for (let frame = 1; frame <= 120; frame += 1) {
    sample = {
      ...controller.sample(
        frame * STEP,
        anger,
        speechless,
        maniac,
        silly,
        lovestruck,
      ),
    }
  }
  return sample
}

function burst(
  controller: StylizedExpressionMotionController,
  startTime: number,
  sillyTarget: number,
  seconds: number,
): { sample: StylizedExpressionMotion; time: number } {
  const frames = Math.round(seconds / STEP)
  let sample = { ...controller.sample(startTime, 0, 0, 0, sillyTarget) }
  for (let frame = 1; frame <= frames; frame += 1) {
    sample = {
      ...controller.sample(startTime + frame * STEP, 0, 0, 0, sillyTarget),
    }
  }
  return { sample, time: startTime + frames * STEP }
}

function range(values: number[]): number {
  return Math.max(...values) - Math.min(...values)
}

function localPeaks(values: number[], threshold: number): number[] {
  const peaks: number[] = []
  for (let index = 1; index < values.length - 1; index += 1) {
    if (
      values[index]! > threshold &&
      values[index]! >= values[index - 1]! &&
      values[index]! > values[index + 1]!
    ) {
      peaks.push(index)
    }
  }
  return peaks
}

function mean(values: number[]): number {
  return values.reduce((total, value) => total + value, 0) / values.length
}

function runsAbove(values: number[], threshold: number): number[] {
  const runs: number[] = []
  let current = 0
  for (const value of values) {
    if (value > threshold) {
      current += 1
      continue
    }
    if (current > 0) runs.push(current)
    current = 0
  }
  if (current > 0) runs.push(current)
  return runs
}
