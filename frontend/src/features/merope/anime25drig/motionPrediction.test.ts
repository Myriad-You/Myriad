import assert from 'node:assert/strict'
import test from 'node:test'
import {
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
