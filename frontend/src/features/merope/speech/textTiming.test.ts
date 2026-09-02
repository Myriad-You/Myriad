import assert from 'node:assert/strict'
import test from 'node:test'
import { compileTextVisemes } from '../anime25drig/textVisemes'
import { estimateVisualSpeechDurationMs } from './textTiming'

/** Matches the pace ceiling `AutoSpeechController.prepareTextCue` clamps to. */
const MAX_REALIZED_PACE = 1.32

const SAMPLES: Array<[string, string | undefined]> = [
  ['你好，这是一段回答。', 'zh-CN'],
  ['我在想这件事，可能需要一点时间。', 'zh-CN'],
  ['こんにちは、少し考えさせてください。', 'ja-JP'],
  ['That is a reasonably long English sentence, I think.', 'en-US'],
  ['好的 OK，我看看 2026 年的数据。', 'zh-CN'],
]

test('the lifecycle tail outlasts the slowest compiled mouth timeline', async () => {
  for (const [text, locale] of SAMPLES) {
    const cues = await compileTextVisemes(text, locale)
    const slowestMs =
      cues.reduce((total, cue) => total + cue.duration, 0) *
      MAX_REALIZED_PACE *
      1_000
    const tailMs = estimateVisualSpeechDurationMs(text, locale)
    assert.ok(
      tailMs >= slowestMs,
      `${text}: tail ${Math.round(tailMs)}ms cuts a ${Math.round(slowestMs)}ms mouth`,
    )
  }
})

test('visual articulation stays slower than a real speaking rate', async () => {
  const text = '今天天气不错我们出去走走吧'
  const cues = await compileTextVisemes(text, 'zh-CN')
  const seconds = cues.reduce((total, cue) => total + cue.duration, 0)
  const perSyllable = seconds / text.length
  // Read-aloud Mandarin sits near 0.2s per syllable. Without audio the mouth is
  // the only cue, so it must stay clearly below that rate to read as speech.
  assert.ok(perSyllable > 0.22, `${perSyllable.toFixed(3)}s per syllable is too fast`)
  assert.ok(perSyllable < 0.4, `${perSyllable.toFixed(3)}s per syllable is sluggish`)
})
