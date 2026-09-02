import assert from 'node:assert/strict'
import test from 'node:test'
import { compileMusicPhrases, sampleMusicPhrase } from './musicPhrases'

test('preserves phrase boundaries, including anticipation and release', () => {
  const phrases = compileMusicPhrases({
    lines: [
      { time: 2, text: 'hello' },
      { time: 10, text: 'world' },
    ],
    songDuration: 12,
  })
  assert.deepEqual(
    phrases.map(({ start, end }) => [start, end]),
    [
      [2, 8],
      [10, 12],
    ],
  )
  assert.equal(sampleMusicPhrase(phrases, 1.5), null)
  assert.equal(sampleMusicPhrase(phrases, 1.75), phrases[0])
  assert.equal(sampleMusicPhrase(phrases, 8.5), phrases[0])
  assert.equal(sampleMusicPhrase(phrases, 9), null)
  assert.equal(sampleMusicPhrase(phrases, 10.5), phrases[1])
  assert.equal(sampleMusicPhrase(phrases, 3), phrases[0])
})

test('word timing gives a real phrase ending, not the next line start', () => {
  const phrases = compileMusicPhrases({
    verbatim: [
      { time: 2, text: 'hi', words: [{ time: 2, duration: 0.4, text: 'hi' }] },
    ],
  })
  assert.deepEqual(phrases, [{ start: 2, end: 2.4, confidence: 0.95 }])
})
