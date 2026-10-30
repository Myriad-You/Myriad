import assert from 'node:assert/strict'
import test from 'node:test'
import { compileTextVisemes } from './textVisemes'
import { alignVisemeTimeline, alignVisemeTimelineToAudio, audioVoicing, textTimeOnAudio, visemeAmount, visemeAt } from './visemeTimeline'

test('stretches the compiled shapes onto the audio that says them', async () => {
  const cues = await compileTextVisemes('你好世界', 'zh-CN')
  assert.ok(cues.length > 0)
  const spans = alignVisemeTimeline(cues, 2)
  assert.equal(spans.length, cues.filter((cue) => cue.duration > 0).length)
  assert.ok(Math.abs(spans.at(-1)!.endsAt - 2) < 1e-9)
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

function utterance(parts: readonly [boolean, number][], rate = 16_000): Float32Array {
  const total = parts.reduce((sum, [, seconds]) => sum + seconds, 0)
  const samples = new Float32Array(Math.round(total * rate))
  let at = 0
  for (const [voiced, seconds] of parts) {
    const end = at + Math.round(seconds * rate)
    if (voiced) {
      for (let i = at; i < end; i += 1) samples[i] = 0.3 * Math.sin((i / rate) * 2 * Math.PI * 180)
    }
    at = end
  }
  return samples
}

test('the mouth keeps to the voice: it starts with it, pauses in its pauses and stops with it', () => {
  const samples = utterance([[false, 0.3], [true, 0.8], [false, 0.4], [true, 0.6], [false, 0.2]])
  const voicing = audioVoicing(samples, 16_000)!
  assert.ok(Math.abs(voicing.voiceStart - 0.3) < 0.02 && Math.abs(voicing.voiceEnd - 2.1) < 0.02)
  assert.equal(voicing.pauses.length, 1)
  const cues = [
    { viseme: 'open' as const, duration: 0.5, emphasis: false },
    { viseme: 'wide' as const, duration: 0.5, emphasis: false },
    { viseme: 'rest' as const, duration: 0.2, emphasis: false },
    { viseme: 'round' as const, duration: 0.8, emphasis: false },
  ]
  const spans = alignVisemeTimelineToAudio(cues, 2.3, voicing)
  assert.equal(spans[0]!.viseme, 'rest', 'still before the voice')
  assert.ok(Math.abs(spans[0]!.endsAt - 0.3) < 0.02)
  const pause = spans.find((span, i) => span.viseme === 'rest' && i > 0)!
  const before = spans[spans.indexOf(pause) - 1]!
  assert.ok(Math.abs(before.endsAt - 1.1) < 0.03, `pause starts with the audio's: ${before.endsAt}`)
  assert.ok(Math.abs(pause.endsAt - 1.5) < 0.03, `and ends with it: ${pause.endsAt}`)
  assert.ok(Math.abs(spans.at(-1)!.endsAt - 2.1) < 0.02, 'stops with the voice')
  // Spread evenly, the mouth was already open through the silence before the voice.
  const even = alignVisemeTimeline(cues, 2.3)
  assert.equal(visemeAt(even, 0.1)?.viseme, 'open')
  assert.equal(visemeAt(spans, 0.1)?.viseme, 'rest')
})

test('without voicing the shapes are spread over the whole audio as before', () => {
  const cues = [{ viseme: 'open' as const, duration: 1, emphasis: false }]
  assert.deepEqual(alignVisemeTimelineToAudio(cues, 2, null), alignVisemeTimeline(cues, 2))
})

test('a place in the text is said where the voice says it: commas in the audio pauses', () => {
  const voicing = { voiceStart: 0.3, voiceEnd: 2.1, pauses: [[1.1, 1.5]] as const }
  const place = textTimeOnAudio('你好呀，今天天气很好', voicing)!
  assert.ok(Math.abs(place(0) - 0.3) < 1e-9, 'the first word with the voice')
  assert.ok(Math.abs(place(3) - 1.1) < 1e-9, 'the comma where the audio pauses')
  assert.ok(Math.abs(place(4) - 1.5) < 1e-9, 'and the next word as it resumes')
  assert.ok(Math.abs(place(7) - 1.8) < 1e-9, 'between, by share of the characters')
  assert.ok(Math.abs(place(10) - 2.1) < 1e-9, 'the last with the voice')
  assert.equal(textTimeOnAudio('你好', null), null)
})
