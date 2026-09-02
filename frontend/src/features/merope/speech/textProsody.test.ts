import assert from 'node:assert/strict'
import test from 'node:test'
import { predictTextProsody } from './textProsody'

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
