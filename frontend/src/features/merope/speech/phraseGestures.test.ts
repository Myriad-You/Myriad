import assert from 'node:assert/strict'
import test from 'node:test'
import { speechPhraseGestures } from './phraseGestures'
import {
  alignTextProsody,
  continueTextProsody,
  predictTextProsody,
} from './textProsody'

test('recognizes delivery cues without interpreting descriptions as laughter', () => {
  assert.deepEqual(
    speechPhraseGestures('不过你确定吗？哈哈哈！').map((cue) => cue.gesture),
    ['contrast', 'question', 'laugh'],
  )
  assert.deepEqual(speechPhraseGestures('他说哈哈。不要哈哈笑。'), [])
  assert.deepEqual(speechPhraseGestures('“不过哈哈？” `but?` 「しかし？」'), [])
  assert.deepEqual(speechPhraseGestures('```typescript\nbut?'), [])
  assert.deepEqual(speechPhraseGestures('她说：“不过你确定吗？'), [])
  assert.deepEqual(speechPhraseGestures('butterfly and yesterday'), [])
  assert.deepEqual(
    speechPhraseGestures('‘哈哈？’ https://example.test/?q=but'),
    [],
  )
  assert.deepEqual(
    speechPhraseGestures('However, why? Haha!').map((cue) => cue.gesture),
    ['contrast', 'question', 'laugh'],
  )
  assert.deepEqual(
    speechPhraseGestures('しかし、どうして？ふふ。').map((cue) => cue.gesture),
    ['contrast', 'question', 'laugh'],
  )
})

test('unfinished words and laughter do not briefly misclassify while streaming', () => {
  assert.deepEqual(speechPhraseGestures('but', true), [])
  assert.deepEqual(speechPhraseGestures('哈哈', true), [])
  assert.deepEqual(speechPhraseGestures('哈哈不是我的口头禅。', true), [])
  assert.equal(speechPhraseGestures('哈哈！', true)[0]?.textOffset, 2)
  assert.equal(speechPhraseGestures('哈哈哈哈！', true)[0]?.textOffset, 2)
  const repeated = predictTextProsody({
    text: '你真的确定吗？？？',
    utteranceId: 'question',
    startedAtMs: 0,
  })
  assert.equal(repeated.accents.length, 1)
  assert.equal(repeated.accents[0]?.gesture, 'question')
})

test('gesture identities survive streaming continuation and audio alignment', () => {
  const input = {
    utteranceId: 'reply',
    startedAtMs: 0,
    text: '不过这次确实有区别。',
    streaming: true,
  }
  const first = continueTextProsody(predictTextProsody(input), null, 0)
  const text = `${input.text}你看出来了吗？哈哈哈！`
  const next = continueTextProsody(
    predictTextProsody({ ...input, text }),
    first,
    0,
  )
  assert.deepEqual(next.accents.slice(0, first.accents.length), first.accents)
  assert.ok(next.accents.some((cue) => cue.gesture === 'question'))
  assert.ok(next.accents.some((cue) => cue.gesture === 'laugh'))
  const audio = alignTextProsody(
    { ...input, text },
    { durationMs: 10_000, accents: [] },
  )
  assert.deepEqual(
    audio.accents
      .filter((cue) => cue.gesture)
      .map((cue) => [cue.textOffset, cue.gesture]),
    next.accents
      .filter((cue) => cue.gesture)
      .map((cue) => [cue.textOffset, cue.gesture]),
  )
})
