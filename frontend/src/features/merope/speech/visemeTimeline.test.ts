import assert from 'node:assert/strict'
import test from 'node:test'
import { compileTextVisemes } from '../anime25drig/textVisemes'
import { alignVisemeTimeline, visemeAmount, visemeAt } from './visemeTimeline'

test('stretches the compiled shapes onto the audio that says them', async () => {
  const cues = await compileTextVisemes('你好世界', 'zh-CN')
  assert.ok(cues.length > 0)
  const spans = alignVisemeTimeline(cues, 2)
  assert.equal(spans.length, cues.filter((cue) => cue.duration > 0).length)
  assert.ok(Math.abs(spans[spans.length - 1]!.endsAt - 2) < 1e-9)
  for (let i = 1; i < spans.length; i++) {
    assert.ok(spans[i]!.endsAt > spans[i - 1]!.endsAt)
  }
})

test('the same text keeps its proportions at any audio length', async () => {
  const cues = await compileTextVisemes('hello there', 'en-US')
  const short = alignVisemeTimeline(cues, 1)
  const long = alignVisemeTimeline(cues, 4)
  assert.equal(short.length, long.length)
  for (let i = 0; i < short.length; i++) {
    assert.ok(Math.abs(short[i]!.endsAt * 4 - long[i]!.endsAt) < 1e-6)
    assert.equal(short[i]!.viseme, long[i]!.viseme)
  }
})

test('an unusable timeline degrades to empty rather than to nonsense', () => {
  assert.deepEqual(alignVisemeTimeline([], 2), [])
  assert.deepEqual(
    alignVisemeTimeline([{ viseme: 'open', duration: 0, emphasis: false }], 2),
    [],
  )
  assert.deepEqual(
    alignVisemeTimeline(
      [{ viseme: 'open', duration: 1, emphasis: false }],
      Number.NaN,
    ),
    [],
  )
  assert.equal(visemeAt([], 0.5), null)
})

test('reads the shape playing at a moment and holds the last one at the end', () => {
  const spans = [
    { viseme: 'closed' as const, endsAt: 0.2, emphasis: false },
    { viseme: 'wide' as const, endsAt: 0.5, emphasis: true },
  ]
  assert.equal(visemeAt(spans, 0)?.viseme, 'closed')
  assert.equal(visemeAt(spans, 0.19)?.viseme, 'closed')
  assert.equal(visemeAt(spans, 0.2)?.viseme, 'wide')
  assert.equal(visemeAt(spans, 9)?.viseme, 'wide')
  assert.equal(visemeAt(spans, -1), null)
})

test('emphasis opens the mouth further and never past the bound', () => {
  assert.ok(visemeAmount(0.5, true) > visemeAmount(0.5, false))
  assert.equal(visemeAmount(1, true), 1)
  assert.equal(visemeAmount(-1, false), 0)
})

// Han characters alone cannot say whether a line is Chinese or Japanese, and
// reading Japanese kanji as pinyin gives the wrong mouth for the sentence.
test('the locale decides how han characters are read', async () => {
  const japanese = await compileTextVisemes('日本語', 'ja-JP')
  const chinese = await compileTextVisemes('日本語', 'zh-CN')
  assert.ok(japanese.length > 0)
  assert.ok(chinese.length > 0)
  assert.notDeepEqual(
    japanese.map((cue) => cue.viseme),
    chinese.map((cue) => cue.viseme),
  )
})

test('keeps conversational pacing and real sentence pauses without audio', async () => {
  const chinese = await compileTextVisemes('你好，世界。再见', 'zh-CN')
  const rests = chinese.filter((cue) => cue.viseme === 'rest')
  assert.ok(rests.some((cue) => cue.duration >= 0.18))
  assert.ok(rests.some((cue) => cue.duration >= 0.32))
  assert.ok(
    chinese.reduce((duration, cue) => duration + cue.duration, 0) >= 1.65,
  )

  const english = await compileTextVisemes('Hello. There', 'en-US')
  assert.ok(
    english.some((cue) => cue.viseme === 'rest' && cue.duration >= 0.32),
    'ASCII full stops are sentence boundaries too',
  )
})
