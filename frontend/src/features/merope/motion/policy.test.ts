import assert from 'node:assert/strict'
import test from 'node:test'
import {
  allowsAmbientMotion,
  allowsCoSpeechExpression,
  allowsCoSpeechHead,
  allowsPointerGaze,
} from './policy'

test('ambient and thinking only run on idle or ambient-owned channels', () => {
  assert.equal(allowsAmbientMotion('idle'), true)
  assert.equal(allowsAmbientMotion('ambient'), true)
  assert.equal(allowsAmbientMotion('music'), false)
  assert.equal(allowsAmbientMotion('performance'), false)
})

test('co-speech brows yield to performance; head nods yield to music', () => {
  assert.equal(allowsCoSpeechExpression('coSpeech'), true)
  assert.equal(allowsCoSpeechExpression('performance'), false)
  assert.equal(allowsCoSpeechHead('coSpeech'), true)
  assert.equal(allowsCoSpeechHead('music'), false)
})

test('local pointer wins gaze except in preview scope', () => {
  assert.equal(allowsPointerGaze('idle'), true)
  assert.equal(allowsPointerGaze('performance'), true)
  assert.equal(allowsPointerGaze('preview'), false)
})
