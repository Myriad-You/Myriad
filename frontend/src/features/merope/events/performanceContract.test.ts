import assert from 'node:assert/strict'
import test from 'node:test'
import {
  PERFORMANCE_CUE_INTENTS,
  performanceContractIsComplete,
  performanceCuePriority,
} from './performanceContract'

test('defines one priority for every semantic performance cue', () => {
  assert.equal(performanceContractIsComplete(), true)
  assert.equal(
    new Set(PERFORMANCE_CUE_INTENTS).size,
    PERFORMANCE_CUE_INTENTS.length,
  )
  assert.equal(performanceCuePriority('respond'), 1)
  assert.equal(performanceCuePriority('speechless'), 2)
  assert.equal(performanceCuePriority('lovestruck'), 3)
})
