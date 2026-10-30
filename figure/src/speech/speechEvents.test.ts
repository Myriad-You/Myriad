import assert from 'node:assert/strict'
import test from 'node:test'
import { meropeSpeechEventDetail } from './speechEvents'

test('keeps bounded textual anchor identities across the speech event boundary', () => {
  const parsed = meropeSpeechEventDetail({
    phase: 'prosody',
    source: 'reply',
    messageId: 'message',
    utteranceId: 'utterance',
    prosody: {
      startedAtMs: 1_000,
      durationMs: 5_000,
      accents: [
        { textOffset: 12, offsetMs: 500, intensity: 0.8 },
        { textOffset: -1, offsetMs: 1_000, intensity: 0.8 },
        { textOffset: 2_001, offsetMs: 2_000, intensity: 0.8 },
        { textOffset: 1.5, offsetMs: 3_000, intensity: 0.8 },
      ],
    },
  })
  assert.equal(parsed?.phase, 'prosody')
  if (parsed?.phase !== 'prosody') return
  assert.deepEqual(
    parsed.prosody.accents.map((accent) => accent.textOffset),
    [12, undefined, undefined, undefined],
  )
})

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

test('bounds future prosody anchors before they enter the motion runtime', () => {
  assert.deepEqual(
    meropeSpeechEventDetail({
      phase: 'prosody',
      messageId: 'msg-1',
      utteranceId: 'audio-1',
      source: 'reply',
      prosody: {
        utteranceId: 'forged',
        startedAtMs: 120,
        durationMs: 99_000,
        accents: [{ offsetMs: -20, intensity: 4 }],
      },
    }),
    {
      phase: 'prosody',
      messageId: 'msg-1',
      utteranceId: 'audio-1',
      source: 'reply',
      prosody: {
        utteranceId: 'audio-1',
        startedAtMs: 120,
        durationMs: 30_000,
        accents: [{ offsetMs: 0, intensity: 1 }],
      },
    },
  )
})
