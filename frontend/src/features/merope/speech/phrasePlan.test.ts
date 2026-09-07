import assert from 'node:assert/strict'
import test from 'node:test'
import { HumanPerformanceRuntime } from '../motion/humanPerformanceRuntime'
import { compileSpeechBehaviorPlan } from '../motion/speechBehaviorPlan'
import {
  directorPhraseCoverage,
  mergeSpeechPhrases,
  refineSpeechPhrases,
  sanitizeSpeechPhrases,
  upcomingSpeechText,
} from './phrasePlan'
import { predictTextProsody } from './textProsody'

const text = '也许我们能试一下。其实可以先解释清楚。你觉得呢？'
const base = predictTextProsody({
  text,
  utteranceId: 'plan',
  startedAtMs: 1_000,
})
const phrases = sanitizeSpeechPhrases([
  { text: '也许我们能试一下。', intent: 'hesitate' },
  { text: '其实可以先解释清楚。', intent: 'explain' },
  { text: '你觉得呢？', intent: 'check-in' },
])

test('upcoming evidence follows scheduler commitment and preserves normalized offsets', () => {
  const plan = {
    utteranceId: 'window',
    startedAtMs: 0,
    durationMs: 3000,
    accents: [
      { textOffset: 3, offsetMs: 1000, intensity: 1 },
      { textOffset: 6, offsetMs: 2000, intensity: 1 },
    ],
  }
  assert.equal(upcomingSpeechText(plan, 'ＡＢ。ＣＤ。', [], 0), 'AB。CD。')
  assert.equal(upcomingSpeechText(plan, 'ＡＢ。ＣＤ。', [], 1000), 'CD。')
  const runtime = new HumanPerformanceRuntime()
  const active = runtime
    .frame([compileSpeechBehaviorPlan(plan)], 0)
    .behaviors.map((item) => ({ ...item, strokeStartAtMs: 4000 }))
  assert.equal(
    upcomingSpeechText(plan, 'ＡＢ。ＣＤ。', active, 1000),
    'AB。CD。',
  )
  assert.equal(upcomingSpeechText(plan, 'ＡＢ。ＣＤ。', [], 3000), '')
  assert.doesNotMatch(
    upcomingSpeechText(plan, '“哈哈！”回答。', [], 1000),
    /哈/,
  )
})

test('fragment revisions retain queued phrases, replace exact matches and stay bounded', () => {
  const first = mergeSpeechPhrases(
    [],
    [{ text: '你真的这么想吗？', intent: 'tease' }],
  )
  const next = mergeSpeechPhrases(first, [
    { text: '你觉得呢？', intent: 'check-in' },
  ])
  assert.equal(next.length, 2)
  assert.equal(next[0]!.intent, 'tease')
  const corrected = mergeSpeechPhrases(next, [
    { text: '你真的这么想吗？', intent: 'none' },
  ])
  assert.equal(corrected.length, 2)
  assert.equal(corrected.at(-1)!.intent, 'none')
  assert.deepEqual(mergeSpeechPhrases(corrected, []), corrected)
  assert.deepEqual(
    mergeSpeechPhrases(corrected, [{ text: 'x', intent: 'laugh' }]),
    corrected,
  )
  let bounded = first
  for (let i = 0; i < 40; i++) {
    bounded = mergeSpeechPhrases(bounded, [
      { text: `句段${i}`, intent: 'explain' },
    ])
  }
  assert.equal(bounded.length, 24)
  assert.equal(bounded.at(-1)!.text, '句段39')
})

test('a discourse sequence selects one delivery per fragment, without new timestamps', () => {
  const result = refineSpeechPhrases(base, text, phrases, [], null, [], 1_000)
  assert.deepEqual(
    result.accents
      .filter((accent) => accent.gesture !== 'none')
      .map((accent) => accent.gesture),
    ['hesitate', 'contrast', 'check-in'],
  )
  assert.deepEqual(
    result.accents.map((accent) => [accent.textOffset, accent.offsetMs]),
    base.accents.map((accent) => [accent.textOffset, accent.offsetMs]),
  )
})

test('ambiguous, missing and malformed evidence cannot assign a gesture', () => {
  assert.deepEqual(
    sanitizeSpeechPhrases([
      { text: '不存在', intent: 'driver' },
      { text: 'x', intent: 'laugh' },
    ]),
    [],
  )
  const repeated = predictTextProsody({
    text: '你好？你好？',
    utteranceId: 'repeat',
    startedAtMs: 0,
  })
  assert.deepEqual(
    refineSpeechPhrases(
      repeated,
      '你好？你好？',
      [{ text: '你好？', intent: 'tease' }],
      [],
      null,
      [],
      0,
    ),
    repeated,
  )
  assert.deepEqual(
    refineSpeechPhrases(
      base,
      text,
      [{ text: '不存在。', intent: 'laugh' }],
      [],
      null,
      [],
      0,
    ),
    base,
  )
})

test('the same question can be an inquiry, a tease or a restrained line', () => {
  const line = '你真的这么想吗？'
  const prosody = predictTextProsody({
    text: line,
    utteranceId: 'question',
    startedAtMs: 0,
  })
  for (const [intent, form] of [
    ['ask', 'question'],
    ['tease', 'tease'],
    ['none', 'none'],
  ] as const) {
    const revised = refineSpeechPhrases(
      prosody,
      line,
      [{ text: line, intent }],
      [],
      null,
      [],
      0,
    )
    assert.equal(revised.accents[0]?.gesture, form)
    const plan = compileSpeechBehaviorPlan(revised)
    if (form === 'none') assert.equal(plan.behaviors.length, 1)
    else assert.equal(plan.behaviors[1]?.form.id, form)
  }
})

test('late intent respects scheduler commitment but can revise later clauses', () => {
  const runtime = new HumanPerformanceRuntime()
  const original = compileSpeechBehaviorPlan(base)
  runtime.frame([original], 1_000)
  const peak = base.startedAtMs + base.accents[0]!.offsetMs
  const active = runtime.frame([original], peak).behaviors
  const result = refineSpeechPhrases(
    base,
    text,
    phrases,
    [],
    base,
    active,
    peak,
  )
  assert.deepEqual(result.accents[0], base.accents[0])
  assert.ok(result.accents.some((accent) => accent.gesture === 'check-in'))
  const revised = runtime.frame([compileSpeechBehaviorPlan(result)], peak)
  assert.ok(revised.behaviors.some((item) => item.form.id === 'check-in'))
  assert.equal(
    revised.behaviors.find((item) => item.id === original.behaviors[1]!.id)
      ?.form.id,
    original.behaviors[1]!.form.id,
  )
  assert.equal(
    revised.behaviors.find((item) => item.id === original.behaviors[1]!.id)
      ?.strokePeakAtMs,
    peak,
  )
})

test('a director beat suppresses only same-meaning, temporally overlapping speech', () => {
  const line = '你确定吗？'
  const prosody = predictTextProsody({
    text: line,
    utteranceId: 'overlap',
    startedAtMs: 1_000,
  })
  const peak = 1_000 + prosody.accents[0]!.offsetMs
  const coverage = [
    { gesture: 'question' as const, startMs: 1_000, endMs: peak },
  ]
  assert.equal(
    refineSpeechPhrases(prosody, line, [], coverage, null, [], 1_000).accents[0]
      ?.gesture,
    'none',
  )
  const later = { ...prosody, startedAtMs: 10_000 }
  assert.equal(
    refineSpeechPhrases(later, line, [], coverage, null, [], 10_000).accents[0]
      ?.gesture,
    'question',
  )
  const already = refineSpeechPhrases(
    prosody,
    line,
    [],
    coverage,
    prosody,
    [],
    peak,
  )
  assert.equal(already.accents[0]?.gesture, 'question')
})

test('a phrase already expressed is not added again on a subsequent accent', () => {
  const line = '不过，我还要再解释这一点。'
  const prosody = predictTextProsody({
    text: line,
    utteranceId: 'single',
    startedAtMs: 0,
  })
  const hint = [{ text: line, intent: 'explain' as const }]
  const first = refineSpeechPhrases(prosody, line, hint, [], null, [], 0)
  const later = refineSpeechPhrases(
    prosody,
    line,
    hint,
    [],
    first,
    [],
    first.accents[0]!.offsetMs + 100,
  )
  assert.equal(
    later.accents.filter((accent) => accent.gesture === 'contrast').length,
    1,
  )
})

test('quoted text, code and links cannot become contextual delivery cues', () => {
  for (const line of [
    '她说：“你确定吗？”',
    '示例：`你确定吗？`',
    '链接 https://example.test/你确定吗？',
  ]) {
    const prosody = predictTextProsody({
      text: line,
      utteranceId: 'quoted',
      startedAtMs: 0,
    })
    assert.deepEqual(
      refineSpeechPhrases(
        prosody,
        line,
        [{ text: '你确定吗？', intent: 'tease' }],
        [],
        null,
        [],
        0,
      ),
      prosody,
    )
  }
})

test('director deduplication uses the committed scheduler timing', () => {
  const plan = compileSpeechBehaviorPlan(base)
  const behavior = plan.behaviors[1]!
  behavior.form = { ...behavior.form, id: 'question' }
  const runtime = new HumanPerformanceRuntime()
  const active = runtime.frame([plan], 1_000).behaviors
  const resolved = active.find((item) => item.id === behavior.id)!
  const shifted = {
    ...plan,
    pegs: plan.pegs.map((peg) => ({ ...peg, atMs: peg.atMs + 10_000 })),
  }
  assert.deepEqual(directorPhraseCoverage(shifted, active)[0], {
    gesture: 'question',
    startMs: resolved.startedAtMs,
    endMs: resolved.endsAtMs,
  })
})
