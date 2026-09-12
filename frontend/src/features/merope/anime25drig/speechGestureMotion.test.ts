import type { SpeechPhrase } from '../../../services/agent/types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { HumanPerformanceRuntime } from '../motion/humanPerformanceRuntime'
import { compileSpeechBehaviorPlan } from '../motion/speechBehaviorPlan'
import { refineSpeechPhrases } from '../speech/phrasePlan'
import { predictTextProsody } from '../speech/textProsody'
import { meropeSpeechEventDetail } from '../speechEvents'
import { Anime25DBehaviorMotionController } from './behaviorMotion'
import { realizeAnime25DBehaviorPlan } from './behaviorRealizer'
import { behaviorMotionScale } from './poseArbitration'
import { CoSpeechExpressionController } from './speechExpression'

function pipeline(text: string, intent?: SpeechPhrase['intent']) {
  let prosody = predictTextProsody({
    text,
    utteranceId: 'gesture',
    startedAtMs: 0,
  })
  if (intent) {
    prosody = refineSpeechPhrases(
      prosody,
      text,
      [{ text, intent }],
      [],
      null,
      [],
      0,
    )
  }
  const detail = meropeSpeechEventDetail({
    source: 'reply',
    messageId: 'message',
    utteranceId: 'gesture',
    phase: 'prosody',
    prosody,
  })
  assert.equal(detail?.phase, 'prosody')
  if (detail?.phase !== 'prosody') throw new Error('missing prosody')
  const plan = compileSpeechBehaviorPlan(detail.prosody)
  const runtime = new HumanPerformanceRuntime()
  const frame = runtime.frame([plan], 0)
  const realized = realizeAnime25DBehaviorPlan(frame.plan!, 0)
  assert.equal(realized.units.length, plan.behaviors.length)
  const motion = new Anime25DBehaviorMotionController()
  motion.replace(realized.units, 0, 0)
  return { motion, units: realized.units, plan }
}

test('question, contrast and laughter reach distinct physical poses through the full behavior path', () => {
  const results = ['你确定吗？', '不过我有个想法。', '哈哈哈！'].map((text) => {
    const { motion, units } = pipeline(text)
    const gesture = units.find((unit) =>
      ['question', 'contrast', 'laugh'].includes(unit.form),
    )!
    assert.ok(gesture)
    const expression = new CoSpeechExpressionController()
    let output = expression.sample(0, true, null, 0, 0, 0)
    for (let at = 0; at <= gesture.timing.strokePeakMs + 180; at += 1000 / 60) {
      const sample = motion.sample(at / 1_000)
      output = expression.sample(
        at / 1_000,
        true,
        null,
        0,
        0,
        0,
        sample.coSpeechQuality,
        sample.coSpeechGesture,
      )
    }
    return { ...output }
  })
  assert.ok(results[0]!.angleZ > 0.08)
  assert.ok(results[0]!.brow > results[1]!.brow)
  assert.ok(results[1]!.angleZ < -0.05)
  assert.ok(results[1]!.body < -0.08)
  assert.ok(results[2]!.eyeOpen < -0.06)
  assert.ok(Math.abs(results[2]!.angleY) > 0.01)
})

test('contextual hesitation, teasing and check-in reach distinct rendered body offsets', () => {
  const results = (['hesitate', 'tease', 'check-in'] as const).map((intent) => {
    const { motion, units } = pipeline('你真的这么想吗？', intent)
    const gesture = units.find((unit) => unit.form === intent)!
    assert.ok(gesture, intent)
    const sample = motion.sample(gesture.timing.strokePeakMs / 1_000)
    assert.ok(sample.coSpeechGesture[intent] > 0)
    const expression = new CoSpeechExpressionController()
    return expression.sample(
      gesture.timing.strokePeakMs / 1_000,
      true,
      null,
      0,
      0,
      0,
      sample.coSpeechQuality,
      sample.coSpeechGesture,
    )
  })
  assert.ok(results[0]!.angleZ < 0)
  assert.ok(results[0]!.body < 0)
  assert.ok(results[1]!.angleZ > results[2]!.angleZ)
  assert.ok(results[1]!.eyeOpen < results[2]!.eyeOpen)
  assert.ok(results[2]!.body > 0)
})

test('a restated laugh retains its phase and cancellation releases the whole gesture', () => {
  const { motion, units } = pipeline('哈哈哈！')
  const peak = units.find((unit) => unit.form === 'laugh')!.timing.strokePeakMs
  const now = peak + 130
  const before = { ...motion.sample(now / 1_000).coSpeechGesture }
  motion.replace(units, now, now / 1_000)
  const restated = motion.sample(now / 1_000).coSpeechGesture
  for (const key of Object.keys(before) as Array<keyof typeof before>) {
    assert.ok(Math.abs(restated[key] - before[key]) < 1e-12, key)
  }
  motion.clear(now / 1_000)
  const later = motion.sample(now / 1_000 + 1)
  assert.equal(later.coSpeech, 0)
  assert.deepEqual(later.coSpeechGesture, {
    question: 0,
    contrast: 0,
    laugh: 0,
    laughPulse: 0,
    hesitate: 0,
    tease: 0,
    'check-in': 0,
  })
})

test('semantic recovery leaves no residual pose when the empty-plan gate returns to fallback', () => {
  const { motion, units } = pipeline('哈哈哈！')
  const peak = units.find((unit) => unit.form === 'laugh')!.timing.strokePeakMs
  const expression = new CoSpeechExpressionController()
  let last = 0
  let largestStep = 0
  let final = 0
  for (let frame = 0; frame <= 120; frame++) {
    const now = peak + (frame * 1000) / 60
    if (frame === 12) motion.clear(now / 1000)
    const sample = motion.sample(now / 1000)
    const pose = expression.sample(
      now / 1000,
      true,
      null,
      0,
      0,
      0,
      sample.coSpeechQuality,
      sample.coSpeechGesture,
    )
    final =
      pose.body * behaviorMotionScale(sample.coSpeech, sample.coSpeechPower)
    if (frame) largestStep = Math.max(largestStep, Math.abs(final - last))
    last = final
  }
  assert.ok(largestStep < 0.075, `largest body step: ${largestStep}`)
  assert.equal(final, 0)
})

test('overlapping phrase gestures blend within one budget rather than add full-strength poses', () => {
  const { motion, units } = pipeline('你确定吗？')
  const question = units.find((unit) => unit.form === 'question')!
  motion.replace(
    [
      ...units,
      { ...question, behaviorId: 'contrast-overlap', form: 'contrast' },
    ],
    0,
    0,
  )
  const mix = motion.sample(question.timing.strokePeakMs / 1000).coSpeechGesture
  assert.ok(mix.question > 0 && mix.contrast > 0)
  assert.ok(mix.question + mix.contrast + mix.laugh <= 1)
})

test('a late director correction preserves the drawn body pose and original stroke peak', () => {
  const text = '你真的这么想吗？'
  const prosody = predictTextProsody({
    text,
    utteranceId: 'revised',
    startedAtMs: 0,
  })
  const plan = compileSpeechBehaviorPlan(prosody)
  const runtime = new HumanPerformanceRuntime()
  const motion = new Anime25DBehaviorMotionController()
  const expression = new CoSpeechExpressionController()
  const first = runtime.frame([plan], 0)
  motion.replace(realizeAnime25DBehaviorPlan(first.plan!, 0).units, 0, 0)
  const peak = prosody.accents[0]!.offsetMs
  const arrival = peak - 90
  const sample = (atMs: number) => {
    const frame = motion.sample(atMs / 1_000)
    const pose = expression.sample(
      atMs / 1_000,
      true,
      null,
      0,
      0,
      0,
      frame.coSpeechQuality,
      frame.coSpeechGesture,
    )
    const scale = behaviorMotionScale(frame.coSpeech, frame.coSpeechPower)
    return { roll: pose.angleZ * scale, body: pose.body * scale }
  }
  const before = sample(arrival)
  const refined = refineSpeechPhrases(
    prosody,
    text,
    [{ text, intent: 'hesitate' }],
    [],
    prosody,
    runtime.snapshots(arrival),
    arrival,
  )
  const revised = runtime.frame([compileSpeechBehaviorPlan(refined)], arrival)
  const units = realizeAnime25DBehaviorPlan(revised.plan!, arrival).units
  const gesture = units.find((unit) => unit.form === 'hesitate')!
  assert.equal(gesture.timing.strokePeakMs, peak)
  motion.replace(units, arrival, arrival / 1_000)
  assert.deepEqual(sample(arrival), before)
  let previous = before
  let largestStep = 0
  for (let at = arrival + 1000 / 120; at < peak + 100; at += 1000 / 120) {
    const current = sample(at)
    largestStep = Math.max(
      largestStep,
      Math.abs(current.roll - previous.roll),
      Math.abs(current.body - previous.body),
    )
    previous = current
  }
  assert.ok(largestStep < 0.07, `largest step: ${largestStep}`)
  assert.ok(previous.roll < 0 && previous.body < 0)
})
