import assert from 'node:assert/strict'
import test from 'node:test'
import { meropeSpeechEventDetail } from '../speechEvents'
import {
  alignTextProsody,
  continueTextProsody,
  predictTextProsody,
} from './textProsody'

test('text prosody is stable for one utterance but varies between utterances', () => {
  const input = {
    utteranceId: 'utterance-a',
    text: '其实这个部分很重要，但是我们还可以再自然一点。',
    locale: 'zh-CN',
    startedAtMs: 1_000,
  }
  const first = predictTextProsody(input)
  assert.deepEqual(predictTextProsody(input), first)
  const other = predictTextProsody({ ...input, utteranceId: 'utterance-b' })
  assert.notDeepEqual(other.accents, first.accents)
})

test('semantic focus and questions produce bounded, spaced accents', () => {
  const plan = predictTextProsody({
    utteranceId: 'question',
    text: '不过关键是什么？所以我们必须现在决定。',
    locale: 'zh-CN',
    startedAtMs: 50,
  })
  assert.ok(plan.accents.length >= 2)
  assert.ok(plan.accents.some((accent) => accent.intensity >= 0.84))
  for (let index = 1; index < plan.accents.length; index += 1) {
    assert.ok(
      plan.accents[index]!.offsetMs - plan.accents[index - 1]!.offsetMs >= 380,
    )
  }
  assert.ok(
    plan.accents.every(
      (accent) => accent.offsetMs >= 0 && accent.offsetMs <= plan.durationMs,
    ),
  )
})

test('plain text still receives one non-periodic phrase accent', () => {
  const plan = predictTextProsody({
    utteranceId: 'plain',
    text: '这是一段没有显式标点的自然回复',
    startedAtMs: 0,
  })
  assert.equal(plan.accents.length, 1)
  assert.ok(plan.accents[0]!.offsetMs > plan.durationMs * 0.4)
})

test('long replies retain late semantic beats on an advancing, prefix-stable clock', () => {
  const prefix = '这一句说完了。'.repeat(16)
  const input = {
    utteranceId: 'long',
    text: prefix,
    startedAtMs: 0,
    streaming: true,
  }
  const first = predictTextProsody(input)
  const next = predictTextProsody({
    ...input,
    text: `${prefix}不过还可以换个办法。你觉得呢？`,
  })
  assert.ok(first.accents.length > 12)
  assert.ok(next.accents.at(-1)!.offsetMs > 12_000)
  assert.equal(next.accents.at(-1)!.gesture, 'question')
  assert.deepEqual(next.accents.slice(0, first.accents.length), first.accents)
  for (let i = 1; i < next.accents.length; i++) {
    assert.ok(next.accents[i]!.offsetMs - next.accents[i - 1]!.offsetMs >= 380)
  }
})

test('later streamed sentences preserve every earlier text boundary and time', () => {
  const input = {
    utteranceId: 'stream',
    text: '其实可以先试一下。',
    startedAtMs: 0,
    streaming: true,
  }
  const first = predictTextProsody(input)
  const later = predictTextProsody({
    ...input,
    text: `${input.text}不过还需要仔细检查，最后结果怎么样？`,
  })
  assert.ok(later.accents.length > first.accents.length)
  assert.deepEqual(later.accents.slice(0, first.accents.length), first.accents)
  assert.equal(
    new Set(later.accents.map((accent) => accent.textOffset)).size,
    later.accents.length,
  )
})

test('a partial word and an unfinished midpoint are not committed as phrase beats', () => {
  const input = {
    utteranceId: 'partial',
    text: 'but',
    startedAtMs: 0,
    streaming: true,
  }
  assert.deepEqual(predictTextProsody(input).accents, [])
  assert.deepEqual(
    predictTextProsody({ ...input, text: 'butterfly' }).accents,
    [],
  )
  assert.deepEqual(
    predictTextProsody({ ...input, text: 'the value is 3.14' }).accents,
    [],
  )
  assert.equal(
    predictTextProsody({ ...input, text: 'but we can try' }).accents.length,
    1,
  )
})

test('nearby later punctuation cannot strengthen or move an earlier published beat', () => {
  const input = {
    utteranceId: 'nearby',
    text: '其实',
    startedAtMs: 0,
    streaming: true,
  }
  const first = predictTextProsody(input)
  assert.deepEqual(
    predictTextProsody({ ...input, text: '其实！' }).accents,
    first.accents,
  )
})

test('late text prepares in the future without replaying completed earlier accents', () => {
  const input = {
    utteranceId: 'late',
    text: '先来试一下。',
    startedAtMs: 1_000,
    streaming: true,
  }
  const first = continueTextProsody(predictTextProsody(input), null, 1_000)
  const predicted = predictTextProsody({
    ...input,
    text: `${input.text}不过你真的确定吗？`,
  })
  const later = continueTextProsody(predicted, first, 15_000)
  assert.deepEqual(later.accents.slice(0, first.accents.length), first.accents)
  assert.ok(later.accents[first.accents.length]!.offsetMs >= 14_140)
  assert.ok(later.durationMs >= later.accents.at(-1)!.offsetMs + 270)
  assert.deepEqual(continueTextProsody(predicted, later, 18_000), later)
  const another = continueTextProsody(
    { ...predicted, utteranceId: 'new' },
    later,
    1_000,
  )
  assert.deepEqual(another.accents, predicted.accents)
})

test('audio phrasing keeps text identities and uses the decoded duration, without duplicate accents', () => {
  const input = {
    utteranceId: 'audio',
    text: '不过我们可以试试。你觉得呢？',
    startedAtMs: 1_000,
  }
  const first = alignTextProsody(input, { durationMs: 6_000, accents: [] })
  const refined = alignTextProsody(input, {
    durationMs: 6_000,
    accents: [
      { offsetMs: first.accents[0]!.offsetMs + 50, intensity: 1 },
      { offsetMs: 0, intensity: 0.8 },
    ],
  })
  assert.ok(first.accents.length >= 2)
  assert.equal(refined.durationMs, 6_000)
  for (const accent of first.accents) {
    assert.deepEqual(
      refined.accents.find((item) => item.textOffset === accent.textOffset),
      accent,
    )
  }
  for (let index = 1; index < refined.accents.length; index++) {
    assert.ok(
      refined.accents[index]!.offsetMs - refined.accents[index - 1]!.offsetMs >=
        380,
    )
  }
  assert.deepEqual(alignTextProsody(input, { durationMs: 0, accents: [] }), {
    durationMs: 0,
    accents: [],
  })
})

test('later audio phrase anchors survive alignment and the production event boundary', () => {
  const text = `${'这一句说完了。'.repeat(16)}你觉得呢？`
  const aligned = alignTextProsody(
    { text, utteranceId: 'audio-tail', startedAtMs: 0 },
    { durationMs: 28_000, accents: [] },
  )
  const event = meropeSpeechEventDetail({
    phase: 'prosody',
    messageId: 'message',
    utteranceId: 'audio-tail',
    source: 'reply',
    text,
    prosody: { ...aligned, startedAtMs: 0 },
  })
  assert.equal(event?.phase, 'prosody')
  if (event?.phase !== 'prosody') return
  assert.ok(event.prosody.accents.length > 12)
  assert.equal(event.prosody.accents.at(-1)!.gesture, 'question')
  assert.deepEqual(event.prosody.accents, aligned.accents)
})
