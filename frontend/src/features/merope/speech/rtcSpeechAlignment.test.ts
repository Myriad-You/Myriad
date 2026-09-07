import type { TextVisemeCue } from '../anime25drig/textVisemes'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  compileRtcVisemeTimeline,
  RtcSpeechAlignment,
} from './rtcSpeechAlignment'

async function compiler(text: string): Promise<TextVisemeCue[]> {
  return text === '你'
    ? [
        { viseme: 'narrow', duration: 1, emphasis: false },
        { viseme: 'wide', duration: 1, emphasis: true },
      ]
    : [{ viseme: 'round', duration: 1, emphasis: false }]
}

test('maps Myriad visemes inside provider word timestamps', async () => {
  const spans = await compileRtcVisemeTimeline(
    [
      { word: '你', start_ms: 1_000, duration_ms: 400 },
      { word: '好', start_ms: 1_500, duration_ms: 200 },
    ],
    'zh-CN',
    compiler,
  )
  assert.deepEqual(spans, [
    {
      startsAtMs: 1_000,
      endsAtMs: 1_200,
      viseme: 'narrow',
      emphasis: false,
    },
    {
      startsAtMs: 1_200,
      endsAtMs: 1_400,
      viseme: 'wide',
      emphasis: true,
    },
    {
      startsAtMs: 1_500,
      endsAtMs: 1_700,
      viseme: 'round',
      emphasis: false,
    },
  ])
})

test('only the selected AgentRun and agent publisher can drive the mouth', async () => {
  const alignment = new RtcSpeechAlignment('8888', 'room', compiler)
  alignment.select(7, 'zh-CN')
  assert.equal(
    await alignment.update(
      transcript('1', 7, [{ word: '你', start_ms: 1_000, duration_ms: 400 }]),
    ),
    false,
  )
  assert.equal(
    await alignment.update(
      transcript('8888', 6, [
        { word: '你', start_ms: 1_000, duration_ms: 400 },
      ]),
    ),
    false,
  )
  alignment.noteAudioPts(1_000, 100)
  assert.equal(alignment.sample(0.5, 100), null)

  assert.equal(
    await alignment.update(
      transcript('8888', 7, [
        { word: '你', start_ms: 1_000, duration_ms: 400 },
      ]),
    ),
    true,
  )
  assert.deepEqual(alignment.sample(0.5, 100), {
    energy: 0.5,
    viseme: 'narrow',
    amount: 0.5,
  })
  assert.deepEqual(alignment.sample(0.01, 100), {
    energy: 0.01,
    viseme: 'rest',
    amount: 0,
  })
})

test('a late compilation cannot overwrite the next turn', async () => {
  let finish: ((value: TextVisemeCue[]) => void) | undefined
  const slowCompiler = () =>
    new Promise<TextVisemeCue[]>((resolve) => {
      finish = resolve
    })
  const alignment = new RtcSpeechAlignment('8888', 'room', slowCompiler)
  alignment.select(7)
  const update = alignment.update(
    transcript('8888', 7, [{ word: '你', start_ms: 1_000, duration_ms: 400 }]),
  )
  alignment.select(8)
  finish?.([{ viseme: 'open', duration: 1, emphasis: false }])
  assert.equal(await update, false)
  alignment.noteAudioPts(1_000, 100)
  assert.equal(alignment.sample(0.5, 100), null)
})

function transcript(
  uid: string,
  turnId: number,
  words: Array<{ word: string; start_ms: number; duration_ms: number }>,
  extra: Record<string, unknown> = {},
) {
  return {
    publisher: uid,
    channelName: 'room',
    message: JSON.stringify({
      object: 'assistant.transcription',
      turn_id: turnId,
      words,
      language: 'zh-CN',
      ...extra,
    }),
  }
}

test('accepts Unix-millisecond PTS and words arriving before the run notice', async () => {
  const pts = 1_746_969_805_235
  const alignment = new RtcSpeechAlignment('8888', 'room', compiler)
  assert.equal(
    await alignment.update(
      transcript('8888', 0, [{ word: '你', start_ms: pts, duration_ms: 400 }]),
    ),
    false,
  )
  alignment.noteAudioPts(pts, 100)
  assert.equal(alignment.sample(0.5, 100), null)
  assert.equal(await alignment.select(0), true)
  assert.equal(alignment.sample(0.5, 100)?.viseme, 'narrow')
  alignment.noteAudioPts(pts + 180, 280)
  assert.equal(alignment.sample(0.5, 280)?.viseme, 'wide')
  assert.equal(
    alignment.sample(0.5, 900),
    null,
    'stale audio must not freeze a viseme',
  )
})

test('merges incremental words without recompiling unchanged text and rejects older revisions', async () => {
  const calls: string[] = []
  const alignment = new RtcSpeechAlignment('8888', 'room', async (text) => {
    calls.push(text)
    return compiler(text)
  })
  await alignment.select(7)
  const first = transcript(
    '8888',
    7,
    [{ word: '你', start_ms: 1_000, duration_ms: 400 }],
    { turn_seq_id: 1 },
  )
  await alignment.update(first)
  assert.equal(await alignment.update(first), false)
  await alignment.update(
    transcript('8888', 7, [{ word: '好', start_ms: 1_500, duration_ms: 400 }], {
      turn_seq_id: 2,
    }),
  )
  assert.deepEqual(calls, ['你', '好'])
  alignment.noteAudioPts(1_500, 100)
  assert.equal(alignment.sample(0.5, 100)?.viseme, 'round')
  assert.equal(await alignment.update(first), false)
})

test('preserves explicit pauses and infers only omitted word durations', async () => {
  const spans = await compileRtcVisemeTimeline(
    [
      { word: '你', start_ms: 1_000, duration_ms: 0 },
      { word: '好', start_ms: 1_600, duration_ms: 0 },
    ],
    'zh-CN',
    compiler,
  )
  assert.equal(spans[1]?.endsAtMs, 1_600)
  assert.equal(spans[2]?.endsAtMs, 1_780)
  const alignment = new RtcSpeechAlignment('8888', 'room', compiler)
  await alignment.select(7)
  await alignment.update(
    transcript('8888', 7, [
      { word: '你', start_ms: 1_000, duration_ms: 200 },
      { word: '好', start_ms: 1_600, duration_ms: 400 },
    ]),
  )
  alignment.noteAudioPts(1_300, 100)
  assert.equal(alignment.sample(0.5, 100)?.viseme, 'rest')
})

test('rejects foreign channels, user ASR, malformed data, and late cancelled words', async () => {
  const alignment = new RtcSpeechAlignment('8888', 'room', compiler)
  await alignment.select(7)
  const valid = transcript('8888', 7, [
    { word: '你', start_ms: 1_000, duration_ms: 400 },
  ])
  for (const event of [
    { ...valid, channelName: 'other' },
    transcript('8888', 7, [], { object: 'user.transcription' }),
    { ...valid, message: '{' },
    { ...valid, message: ' '.repeat(64_001) },
  ])
    assert.equal(await alignment.update(event), false)
  const binary = { ...valid, message: new TextEncoder().encode(valid.message) }
  assert.equal(await alignment.update(binary), true)
  alignment.cancel()
  assert.equal(await alignment.update(valid), false)
  alignment.noteAudioPts(1_000, 100)
  assert.equal(alignment.sample(0.5, 100), null)
})

test('a provider interruption arriving before the run notice cannot be resurrected', async () => {
  const alignment = new RtcSpeechAlignment('8888', 'room', compiler)
  await alignment.update(
    transcript('8888', 7, [], { object: 'message.interrupt' }),
  )
  assert.equal(await alignment.select(7), false)
  assert.equal(
    await alignment.update(
      transcript('8888', 7, [
        { word: '你', start_ms: 1_000, duration_ms: 400 },
      ]),
    ),
    false,
  )
  alignment.noteAudioPts(1_000, 100)
  assert.equal(alignment.sample(0.5, 100), null)
})
