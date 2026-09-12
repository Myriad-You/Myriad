import assert from 'node:assert/strict'
import test from 'node:test'
import { compileTextVisemes } from '../anime25drig/textVisemes'
import {
  estimateVisualSpeechDurationMs,
  estimateVisualSpeechTailMs,
  visualSpeechPrefixMs,
} from './textTiming'

const MAX_REALIZED_PACE = 1.32

const SAMPLES: Array<[string, string | undefined]> = [
  ['你好，这是一段回答。', 'zh-CN'],
  ['我在想这件事，可能需要一点时间。', 'zh-CN'],
  ['こんにちは、少し考えさせてください。', 'ja-JP'],
  ['That is a reasonably long English sentence, I think.', 'en-US'],
  ['好的 OK，我看看 2026 年的数据。', 'zh-CN'],
  ['这一句说完了。'.repeat(30), 'zh-CN'],
]

test('the linear clock preserves every multilingual prefix and UTF-16 anchor', () => {
  for (const text of [
    '你好，不过呢？',
    'a butterfly 2026 OK!',
    'こんにちは、𠮷野さん。',
    'ＡＢＣ １２3x',
    '😀 Hello 👋，世界！',
  ]) {
    const normalized = text.normalize('NFKC')
    const clock = visualSpeechPrefixMs(text)
    assert.equal(clock.length, normalized.length + 1)
    for (let offset = 0; offset <= normalized.length; offset++) {
      assert.equal(
        clock[offset],
        estimateVisualSpeechDurationMs(normalized.slice(0, offset)),
        `${text}:${offset}`,
      )
    }
  }
})

test('the lifecycle tail outlasts the slowest compiled mouth timeline', async () => {
  for (const [text, locale] of SAMPLES) {
    const cues = await compileTextVisemes(text, locale)
    const slowestMs =
      cues.reduce((total, cue) => total + cue.duration, 0) *
      MAX_REALIZED_PACE *
      1_000
    const tailMs = estimateVisualSpeechTailMs(text, locale)
    assert.ok(
      tailMs >= slowestMs,
      `${text}: tail ${Math.round(tailMs)}ms cuts a ${Math.round(slowestMs)}ms mouth`,
    )
  }
})

test('long-phrase beat prediction uses conversational pace, not the worst-case lifecycle tail', async () => {
  const text = '这一句说完了。'.repeat(30)
  const cues = await compileTextVisemes(text, 'zh-CN')
  const nominalMs = cues.reduce((total, cue) => total + cue.duration, 0) * 1_000
  const predicted = estimateVisualSpeechDurationMs(text, 'zh-CN')
  assert.ok(Math.abs(predicted - nominalMs) / nominalMs < 0.06)
  assert.ok(estimateVisualSpeechTailMs(text, 'zh-CN') > predicted * 1.3)
})

test('long text compiles its final phrase instead of truncating at an unrelated cue count', async () => {
  const prefix = '这一句说完了。'.repeat(30)
  const before = await compileTextVisemes(prefix, 'zh-CN')
  const after = await compileTextVisemes(`${prefix}你好。`, 'zh-CN')
  assert.ok(before.length > 256)
  assert.ok(after.length > before.length)
  assert.deepEqual(after.slice(0, before.length), before)
})

test('visual articulation stays slower than a real speaking rate', async () => {
  const text = '今天天气不错我们出去走走吧'
  const cues = await compileTextVisemes(text, 'zh-CN')
  const seconds = cues.reduce((total, cue) => total + cue.duration, 0)
  const perSyllable = seconds / text.length
  assert.ok(
    perSyllable > 0.22,
    `${perSyllable.toFixed(3)}s per syllable is too fast`,
  )
  assert.ok(
    perSyllable < 0.4,
    `${perSyllable.toFixed(3)}s per syllable is sluggish`,
  )
})
