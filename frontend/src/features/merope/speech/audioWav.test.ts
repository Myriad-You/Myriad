import assert from 'node:assert/strict'
import test from 'node:test'
import { frameRms, isSubmittableTranscript } from './audioWav'

test('low-confidence noise is not a Chat submit', () => {
  assert.equal(isSubmittableTranscript(''), false)
  assert.equal(isSubmittableTranscript('…'), false)
  assert.equal(isSubmittableTranscript('嗯'), false)
  assert.equal(isSubmittableTranscript('你好'), true)
  assert.equal(isSubmittableTranscript('ok'), true)
})

test('silence has near-zero rms', () => {
  assert.ok(frameRms(new Float32Array(32)) < 0.001)
})
