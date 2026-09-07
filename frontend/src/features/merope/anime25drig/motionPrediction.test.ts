import assert from 'node:assert/strict'
import test from 'node:test'
import {
  monotonicControlTime,
  predictedControlTime,
  SCHEDULED_CONTROL_PREDICTION_SECONDS,
} from './motionPrediction'

test('scheduled control leads presentation by a small bounded horizon', () => {
  assert.equal(predictedControlTime(2), 2.04)
  assert.ok(SCHEDULED_CONTROL_PREDICTION_SECONDS >= 0.03)
  assert.ok(SCHEDULED_CONTROL_PREDICTION_SECONDS <= 0.05)
})

test('invalid clocks cannot create an unbounded prediction', () => {
  assert.equal(
    predictedControlTime(Number.NaN),
    SCHEDULED_CONTROL_PREDICTION_SECONDS,
  )
  assert.equal(predictedControlTime(-1), SCHEDULED_CONTROL_PREDICTION_SECONDS)
})

test('a delivery at the rig own rate keeps the documented lead', () => {
  assert.equal(predictedControlTime(2, 1), predictedControlTime(2))
})

test('the lead follows the response it compensates', () => {
  // `poseResponseScale` moves the pose filter bandwidth with the delivery's
  // manner. A single constant under-compensated a quick, direct beat and
  // over-compensated a fluid one.
  const quick = predictedControlTime(2, 1.35) - 2
  const even = predictedControlTime(2, 1) - 2
  const fluid = predictedControlTime(2, 0.75) - 2
  assert.ok(quick < even)
  assert.ok(even < fluid)
  // Only the response half moves; the display frame is not negotiable.
  assert.ok(quick > 0.0167)
})

test('a nonsensical scale falls back to the rig own rate', () => {
  for (const scale of [0, -1, Number.NaN, Number.POSITIVE_INFINITY]) {
    const lead = predictedControlTime(2, scale) - 2
    assert.ok(Number.isFinite(lead))
    assert.ok(
      Math.abs(lead - SCHEDULED_CONTROL_PREDICTION_SECONDS) < 1e-9,
      `${scale} produced a lead of ${lead}`,
    )
  }
  assert.equal(predictedControlTime(Number.NaN, 1), predictedControlTime(0, 1))
})

test('a shrinking lead cannot rewind the clock every controller reads on', () => {
  // A fluid delivery leads by ~48ms and a quick one by ~34ms. At 120Hz the
  // frame is 8.3ms, so switching between them mid-utterance would otherwise
  // hand every scheduled controller a time earlier than the last one and step
  // every envelope backwards at once.
  const fluid = monotonicControlTime(0, 2, 0.75)
  const quick = monotonicControlTime(fluid, 2 + 1 / 120, 1.35)
  assert.ok(predictedControlTime(2 + 1 / 120, 1.35) < fluid)
  assert.equal(quick, fluid)

  // It holds, it does not freeze: real elapsed time still moves it on.
  assert.ok(monotonicControlTime(quick, 2 + 0.05, 1.35) > quick)
  assert.equal(
    monotonicControlTime(Number.NaN, 2, 1),
    predictedControlTime(2, 1),
  )
})
