import assert from 'node:assert/strict'
import test from 'node:test'
import { meropeSpeechEventDetail } from './speechEvents'

test('sanitizes streamed speech lifecycle events', () => {
  assert.deepEqual(
    meropeSpeechEventDetail({
      phase: 'chunk',
      messageId: ' msg-1 ',
      utteranceId: ' stream-1 ',
      source: 'reply',
      text: '  你好  ',
    }),
    {
      phase: 'chunk',
      messageId: 'msg-1',
      utteranceId: 'stream-1',
      source: 'reply',
      text: '  你好  ',
    },
  )
  assert.equal(
    meropeSpeechEventDetail({
      phase: 'chunk',
      messageId: 'msg-1',
      utteranceId: 'stream-1',
      text: '   ',
    }),
    null,
  )
})

test('allows message-wide cancellation and bounds authored samples', () => {
  assert.deepEqual(
    meropeSpeechEventDetail({
      phase: 'cancel',
      messageId: 'msg-1',
      source: 'invalid',
    }),
    { phase: 'cancel', messageId: 'msg-1', source: 'reply' },
  )
  assert.deepEqual(
    meropeSpeechEventDetail({
      phase: 'energy',
      messageId: 'msg-1',
      utteranceId: 'audio-1',
      source: 'reply',
      energy: 3,
    }),
    {
      phase: 'energy',
      messageId: 'msg-1',
      utteranceId: 'audio-1',
      source: 'reply',
      energy: 1,
    },
  )
})

test('rejects malformed articulation before it reaches the rig', () => {
  assert.equal(
    meropeSpeechEventDetail({
      phase: 'articulation',
      messageId: 'msg-1',
      utteranceId: 'audio-1',
      articulation: { viseme: 'unknown', amount: 0.5, energy: 0.5 },
    }),
    null,
  )
  assert.deepEqual(
    meropeSpeechEventDetail({
      phase: 'articulation',
      messageId: 'msg-1',
      utteranceId: 'audio-1',
      source: 'reply',
      articulation: { viseme: 'round', amount: 2, energy: -1 },
    }),
    {
      phase: 'articulation',
      messageId: 'msg-1',
      utteranceId: 'audio-1',
      source: 'reply',
      articulation: { viseme: 'round', amount: 1, energy: 0 },
    },
  )
})
