import type {
  PerformanceCue,
  PerformanceDirective,
} from '../../../services/agent/types'
import type { Anime25DMotionUnit } from './behaviorMotion'
import assert from 'node:assert/strict'
import test from 'node:test'
import { compilePerformanceBehaviorPlan } from '../motion/performanceBehaviorPlan'
import { realizeAnime25DBehaviorPlan } from './behaviorRealizer'
import {
  applyPerformanceExpressionOffset,
  baselineExpressionOffset,
  bearingDriverPatch,
  intentExpressionOffset,
  mixBoundedExpressionChannel,
  mixEyeOpen,
  PerformanceExpressionController,
} from './performanceExpression'

const steadyBaseline = {
  expression: 'steady' as const,
  posture: 'neutral' as const,
  motionEnergy: 1,
  attention: 1,
}

function cue(
  intent: PerformanceCue['intent'],
  interrupt: PerformanceCue['interrupt'] = 'replace',
  atMs = 0,
): PerformanceCue {
  return {
    intent,
    atMs,
    intensity: 1,
    tempo: 1,
    fadeInMs: 100,
    fadeOutMs: 200,
    interrupt,
  }
}

// "Conservative" is measured against the ambient random-action band the rig
// already plays in (brow 0.12-0.36), not against zero: a semantic cue quieter
// than the character's own idle fidgeting reads as no cue at all.
test('maps semantic baselines and cues to legible expression offsets', () => {
  const warm = baselineExpressionOffset({
    ...steadyBaseline,
    expression: 'warm',
  })
  const withdrawn = baselineExpressionOffset({
    ...steadyBaseline,
    expression: 'withdrawn',
  })
  const delight = intentExpressionOffset('delight', 1.4)
  const dizzy = intentExpressionOffset('dizzy', 1.4)
  const think = intentExpressionOffset('think', 1.4)
  const cry = intentExpressionOffset('cry', 1.2)
  const angry = intentExpressionOffset('angry', 1)
  const speechless = intentExpressionOffset('speechless', 1)
  const maniac = intentExpressionOffset('maniac', 1)
  const silly = intentExpressionOffset('silly', 1)
  const lovestruck = intentExpressionOffset('lovestruck', 1)

  assert.ok(warm.mouthForm > 0 && warm.mouthForm <= 0.16)
  assert.ok(withdrawn.eyeOpen <= -0.16 && withdrawn.eyeOpen >= -0.22)
  assert.ok(Math.abs(delight.mouthForm) <= 0.26)
  assert.ok(Math.abs(delight.angleY) > 0.16)
  assert.ok(Math.abs(delight.angleY) <= 0.18)
  assert.ok(delight.eyeSqueeze > 1.1 && delight.eyeSqueeze < 1.2)
  assert.ok((delight.bust ?? 0) > 0)
  assert.equal(dizzy.eyeDizzy, 1)
  assert.equal(dizzy.eyeOpen, 0)
  assert.equal(dizzy.irisScale, 0)
  assert.equal(dizzy.angleZ, 0)
  assert.equal(dizzy.mouthForm, 0)
  assert.equal(think.eyeOpen, 0)
  assert.ok(think.eyeX > 0.4)
  assert.ok(think.eyeY < -0.3)
  assert.ok(think.angleZ < 0)
  assert.ok(think.brow > 0)
  assert.equal(cry.eyeCry, 1)
  assert.ok(cry.browAngSym < -0.3)
  assert.ok(cry.mouthForm < 0)
  assert.equal(angry.anger, 1)
  assert.equal(speechless.speechless, 1)
  assert.equal(maniac.maniac, 1)
  assert.equal(silly.silly, 1)
  assert.equal(silly.eyeX, 0)
  assert.equal(silly.eyeY, 0)
  assert.equal(lovestruck.lovestruck, 1)
  assert.deepEqual(Object.keys(warm).sort(), [
    'anger',
    'angleY',
    'angleZ',
    'armPos',
    'armY',
    'body',
    'brow',
    'browAngSym',
    'bust',
    'eyeCry',
    'eyeDizzy',
    'eyeOpen',
    'eyeSqueeze',
    'eyeX',
    'eyeY',
    'irisScale',
    'lovestruck',
    'maniac',
    'mouthForm',
    'silly',
    'speechless',
  ])
})

test('irritation is the one baseline that opens the eye while the brow drops', () => {
  const ladder = (['withdrawn', 'subdued', 'steady', 'warm'] as const).map(
    (expression) => baselineExpressionOffset({ ...steadyBaseline, expression }),
  )
  const tense = baselineExpressionOffset({ ...steadyBaseline, expression: 'tense' })

  // The four-rung ladder is one valence axis: nothing on it can lower the brow
  // without also closing the eye, so low mood with high arousal had nowhere to
  // land and wore the flat face instead.
  for (const rung of ladder) {
    assert.ok(rung.eyeOpen <= 0, 'a ladder rung opened the eye')
  }
  assert.ok(tense.eyeOpen > 0)
  assert.ok(tense.brow < Math.min(...ladder.map((rung) => rung.brow)))
  assert.ok(tense.brow <= -0.28)
  assert.ok(tense.browAngSym >= 0.4)
  assert.ok(tense.mouthForm < 0)
  assert.ok(tense.irisScale < Math.min(...ladder.map((rung) => rung.irisScale)))
  // A bearing, not the angry sticker: knitted brow without the vein mark.
  assert.equal(tense.anger ?? 0, 0)
})

test('the two low-valence standing faces use distinct sad brow geometry', () => {
  const withdrawn = baselineExpressionOffset({
    ...steadyBaseline,
    expression: 'withdrawn',
  })
  const subdued = baselineExpressionOffset({
    ...steadyBaseline,
    expression: 'subdued',
  })

  // Negative symmetric rotation lifts the inner ends of the two brows — the
  // same readable sad direction as `cry`, without taking over eyes or mouth.
  assert.ok(withdrawn.browAngSym <= -0.55)
  assert.ok(subdued.browAngSym <= -0.28)
  assert.ok(withdrawn.browAngSym < subdued.browAngSym)
  assert.ok(withdrawn.eyeOpen < subdued.eyeOpen)
  assert.ok(withdrawn.mouthForm < subdued.mouthForm)
  assert.ok(withdrawn.irisScale < subdued.irisScale)
  assert.equal(withdrawn.eyeCry, 0)
  assert.equal(subdued.eyeCry, 0)
  assert.equal(withdrawn.anger, 0)
  assert.equal(subdued.anger, 0)
})

test('gives every directed pose a legible low-intensity movement floor', () => {
  for (const intent of [
    'greet',
    'respond',
    'question',
    'delight',
    'emphasize',
    'listen',
    'notify',
    'think',
    'angry',
    'speechless',
    'maniac',
    'silly',
    'lovestruck',
  ] as const) {
    const offset = intentExpressionOffset(intent, 0.55)
    const displacement = Math.max(
      Math.abs(offset.angleY),
      Math.abs(offset.angleZ),
      Math.abs(offset.body),
      Math.abs(offset.armY),
      Math.abs(offset.armPos),
      Math.abs(offset.bust ?? 0),
    )
    assert.ok(displacement >= 0.08, intent)
    assert.ok(displacement <= 0.5, intent)
  }

  for (const intent of ['dizzy', 'cry'] as const) {
    const offset = intentExpressionOffset(intent, 1)
    assert.equal(offset.angleY, 0, intent)
    assert.equal(offset.angleZ, 0, intent)
    assert.equal(offset.body, 0, intent)
    assert.equal(offset.armY, 0, intent)
    assert.equal(offset.armPos, 0, intent)
  }
})

test('maps additive bearing offsets onto absolute driver neutrals', () => {
  const steady = bearingDriverPatch(steadyBaseline)
  const withdrawn = bearingDriverPatch({
    ...steadyBaseline,
    expression: 'withdrawn',
  })
  const cleared = bearingDriverPatch(null)

  assert.equal(steady.eyeOpenL, 1)
  assert.equal(steady.eyeOpenR, 1)
  assert.equal(steady.irisScale, 1)
  assert.equal(withdrawn.eyeOpenL, 0.8)
  assert.equal(withdrawn.eyeOpenR, 0.8)
  assert.equal(withdrawn.irisScale, 0.945)
  const tense = bearingDriverPatch({
    ...steadyBaseline,
    expression: 'tense',
  })
  assert.ok((tense.brow ?? 0) <= -0.28)
  assert.ok((tense.browAngSym ?? 0) >= 0.4)
  assert.ok((tense.eyeOpenL ?? 0) > 1)
  assert.equal(cleared.eyeOpenL, 1)
  assert.equal(cleared.irisScale, 1)
  assert.equal('bust' in steady, false)
  assert.equal('eyeDizzy' in steady, false)
})

test('adds to manual channels without flattening left-right eye differences', () => {
  const target = {
    brow: 0.3,
    browAngSym: 0.1,
    eyeOpenL: 0.72,
    eyeOpenR: 0.91,
    eyeDizzy: 0,
    eyeSqueeze: 0,
    eyeCry: 0,
    mouthForm: -0.2,
    irisScale: 1.1,
    angleX: 0.6,
    angleY: 0.25,
    angleZ: -0.3,
    body: -0.4,
    eyeX: 0.35,
    eyeY: -0.25,
  }
  applyPerformanceExpressionOffset(target, {
    brow: 0.05,
    browAngSym: -0.04,
    eyeOpen: -0.04,
    eyeDizzy: 0,
    eyeSqueeze: 0,
    eyeCry: 0,
    eyeX: 0,
    eyeY: 0,
    mouthForm: 0.12,
    irisScale: 0.01,
    angleY: -0.03,
    angleZ: 0.02,
    body: 0,
    armY: 0,
    armPos: 0,
  })

  assert.ok(Math.abs(target.eyeOpenL - 0.68) < 1e-12)
  assert.ok(Math.abs(target.eyeOpenR - 0.87) < 1e-12)
  assert.ok(Math.abs(target.eyeOpenR - target.eyeOpenL - 0.19) < 1e-12)
  assert.ok(Math.abs(target.mouthForm - -0.08) < 1e-12)
  assert.ok(Math.abs(target.angleY - 0.22) < 1e-12)
  assert.equal(target.angleX, 0.6)
  assert.equal(target.body, -0.4)
  assert.equal(target.eyeX, 0.35)
  assert.equal(target.eyeY, -0.25)
  assert.ok(Math.abs(target.browAngSym - 0.06) < 1e-12)
})

test('never reopens an authored closed eye or closed-eye smile', () => {
  const wink = {
    brow: 0.2,
    browAngSym: 0,
    eyeOpenL: 0,
    eyeOpenR: 1,
    eyeDizzy: 0,
    eyeSqueeze: 0,
    eyeCry: 0,
    eyeX: 0,
    eyeY: 0,
    mouthForm: 0.7,
    irisScale: 1,
    angleY: 0,
    angleZ: 0,
  }
  applyPerformanceExpressionOffset(wink, {
    brow: 0.1,
    browAngSym: 0,
    eyeOpen: 0.04,
    eyeDizzy: 0,
    eyeSqueeze: 0,
    eyeCry: 0,
    eyeX: 0,
    eyeY: 0,
    mouthForm: 0.1,
    irisScale: 0,
    angleY: 0,
    angleZ: 0,
    body: 0,
    armY: 0,
    armPos: 0,
  })

  assert.equal(wink.eyeOpenL, 0)
  assert.equal(wink.eyeOpenR, 1)
  assert.equal(mixEyeOpen(0, 0.05), 0)
  assert.equal(mixEyeOpen(0, -0.05), 0)
  assert.ok(Math.abs(mixEyeOpen(0.04, -0.03) - 0.01) < 1e-12)
})

test('soft-limits additive expression near manual channel extremes', () => {
  const positive = mixBoundedExpressionChannel(0.95, 0.1, -1, 1, 0)
  const negative = mixBoundedExpressionChannel(-0.95, -0.1, -1, 1, 0)
  const combinedWarmDelight = mixBoundedExpressionChannel(
    0.75,
    0.12 + 0.18 * 1.4,
    -1,
    1,
    0,
  )
  assert.ok(positive > 0.95 && positive < 1)
  assert.ok(negative < -0.95 && negative > -1)
  assert.ok(combinedWarmDelight > 0.8 && combinedWarmDelight < 0.98)
  assert.equal(mixBoundedExpressionChannel(0.5, 0.1, -1, 1, 0), 0.6)
  assert.equal(mixBoundedExpressionChannel(1, -0.1, -1, 1, 0), 0.9)
  assert.equal(mixBoundedExpressionChannel(1, 0.1, -1, 1, 0), 1)
  assert.equal(mixBoundedExpressionChannel(-1, -0.1, -1, 1, 0), -1)
  for (let step = -99; step <= 99; step += 1) {
    const base = step / 100
    assert.ok(mixBoundedExpressionChannel(base, 0.4, -1, 1, 0) < 1)
    assert.ok(mixBoundedExpressionChannel(base, -0.4, -1, 1, 0) > -1)
  }
})

/** Schedules cues the way the player does: compile, realize, restate. */
function playUnits(
  controller: PerformanceExpressionController,
  cues: PerformanceCue[],
  atSeconds = 0,
): readonly Anime25DMotionUnit[] {
  const originMs = atSeconds * 1_000
  const plan = compilePerformanceBehaviorPlan(
    { phase: 'delivery', moodRevision: 1, motionStyle: 'even', plan: { cues } },
    originMs,
    'performance',
  )
  const { units } = realizeAnime25DBehaviorPlan(plan, originMs)
  controller.playBehaviorUnits(units, atSeconds, originMs)
  return units
}

const ZERO_SAMPLE = {
  brow: 0,
  browAngSym: 0,
  eyeOpen: 0,
  eyeDizzy: 0,
  eyeSqueeze: 0,
  eyeCry: 0,
  eyeX: 0,
  eyeY: 0,
  mouthForm: 0,
  irisScale: 0,
  angleY: 0,
  angleZ: 0,
  body: 0,
  armY: 0,
  armPos: 0,
  bust: 0,
  anger: 0,
  speechless: 0,
  maniac: 0,
  silly: 0,
  lovestruck: 0,
} as const

test('sample reuses one offset object and starts every frame at rest', () => {
  const expression = new PerformanceExpressionController()
  const first = expression.sample(0)
  assert.deepEqual({ ...first }, { ...ZERO_SAMPLE })
  const next = expression.sample(1 / 60)
  // The bearing is a base pose the player installs; this controller holds no
  // second copy of it, so with nothing scheduled it stays at zero — and it
  // must not allocate a new offset per frame.
  assert.equal(first, next)
  assert.deepEqual({ ...next }, { ...ZERO_SAMPLE })
})

test('attention easing is equivalent at 30 and 60 FPS', () => {
  const thirty = new PerformanceExpressionController()
  const sixty = new PerformanceExpressionController()
  thirty.setBearingAttention(1)
  sixty.setBearingAttention(1)
  // Prime both clocks: the first sample only establishes `lastTime`.
  thirty.sample(0)
  sixty.sample(0)
  for (let frame = 1; frame <= 30; frame += 1) thirty.sample(frame / 30)
  for (let frame = 1; frame <= 60; frame += 1) sixty.sample(frame / 60)
  assert.ok(
    Math.abs(thirty.getAmbientMotionScale() - sixty.getAmbientMotionScale()) <
      1e-9,
  )
})

test('uses attention only to symmetrically restrain ambient wandering', () => {
  const focused = new PerformanceExpressionController()
  const unfocused = new PerformanceExpressionController()
  focused.setBearingAttention(1)
  unfocused.setBearingAttention(0)
  for (let frame = 1; frame <= 60; frame += 1) {
    focused.sample(frame / 60)
    unfocused.sample(frame / 60)
  }
  assert.ok(focused.getAmbientMotionScale() < 0.71)
  assert.equal(unfocused.getAmbientMotionScale(), 1)
})

test('fades a semantic behavior in and out back to rest', () => {
  const expression = new PerformanceExpressionController()
  const [unit] = playUnits(expression, [cue('question')])
  assert.ok(unit)
  assert.equal(expression.sample(0).brow, 0)
  const rising = expression.sample(unit.timing.strokePeakMs / 2_000).brow
  assert.ok(rising > 0)
  const peak = expression.sample(unit.timing.strokePeakMs / 1_000).brow
  assert.ok(peak > rising)
  // The amplitude that matters is legibility against the rig's own idle band
  // (brow 0.12-0.36), not a decimal in the driver.
  assert.ok(peak >= 0.12, `${peak} is quieter than an idle fidget`)
  assert.ok(Math.abs(expression.sample(unit.timing.endMs! / 1_000).brow) < 1e-9)
})

test('does not replay a behavior that already ended before this frame', () => {
  const expression = new PerformanceExpressionController()
  const plan = compilePerformanceBehaviorPlan(
    {
      phase: 'delivery',
      moodRevision: 1,
      motionStyle: 'even',
      plan: { cues: [cue('question')] },
    },
    10_000,
    'performance',
  )
  const { units } = realizeAnime25DBehaviorPlan(plan, 10_000)
  // A remount hands the body a plan whose behaviors are already finished. The
  // rig state summary has told the director they are over; replaying them
  // would contradict what the backend was told.
  expression.playBehaviorUnits(units, 18, 18_000)
  assert.equal(expression.sample(18).brow, 0)
  assert.equal(expression.getScheduledCueCount(), 0)
})

test('a remount resumes a behavior mid-flight instead of replaying it', () => {
  const plan = compilePerformanceBehaviorPlan(
    {
      phase: 'delivery',
      moodRevision: 1,
      motionStyle: 'even',
      plan: { cues: [cue('question')] },
    },
    0,
    'performance',
  )
  const { units } = realizeAnime25DBehaviorPlan(plan, 0)
  const peakSeconds = units[0]!.timing.strokePeakMs / 1_000

  const fresh = new PerformanceExpressionController()
  fresh.playBehaviorUnits(units, 0, 0)
  const atPeak = fresh.sample(peakSeconds).brow

  // The same plan reaching a player whose clock starts at zero 300ms later.
  const remounted = new PerformanceExpressionController()
  remounted.playBehaviorUnits(units, 0, 300)
  assert.ok(
    Math.abs(remounted.sample(peakSeconds - 0.3).brow - atPeak) < 1e-9,
    'the plan restarted from the top instead of resuming',
  )
})

test('prunes expired cues during long-lived playback', () => {
  const controller = new PerformanceExpressionController()
  for (let index = 0; index < 80; index += 1) {
    playUnits(controller, [{ ...cue('notify'), intensity: 0.8 }], index * 2)
    controller.sample(index * 2 + 1.9)
  }
  assert.ok(controller.getScheduledCueCount() <= 1)
})

test('replacing a behavior crossfades instead of cutting the active face', () => {
  const expression = new PerformanceExpressionController()
  playUnits(expression, [cue('maniac')])
  const held = poseMagnitude(expression.sample(0.3))
  assert.ok(held > 0)
  playUnits(expression, [cue('greet')], 0.3)
  // The outgoing sticker must release through its own fade, not vanish on the
  // first sample after the replacement lands.
  assert.ok(poseMagnitude(expression.sample(0.31)) > 0)
})

test('stopping an active sticker releases it into rest', () => {
  const expression = new PerformanceExpressionController()
  playUnits(expression, [cue('maniac')])
  assert.ok(poseMagnitude(expression.sample(0.3)) > 0)
  expression.stopBehaviors(0.3)
  assert.equal(poseMagnitude(expression.sample(2)), 0)
})

test('stop releases every scheduled behavior and restores ambient motion', () => {
  const expression = new PerformanceExpressionController()
  expression.setBearingAttention(1)
  playUnits(expression, [cue('greet')])
  assert.ok(poseMagnitude(expression.sample(0.3)) > 0)
  expression.stop(0.3)
  for (let frame = 1; frame <= 120; frame += 1) expression.sample(0.3 + frame / 60)
  assert.equal(poseMagnitude(expression.sample(3)), 0)
  assert.ok(expression.getAmbientMotionScale() > 0.99)
})

test('face and body of one behavior peak in the same envelope window', () => {
  const expression = new PerformanceExpressionController()
  const [unit] = playUnits(expression, [cue('greet')])
  assert.ok(unit)
  const peak = expression.sample(unit.timing.strokePeakMs / 1_000)
  assert.ok(Math.abs(peak.brow) > 0)
  assert.ok(Math.abs(peak.angleY) > 0 || Math.abs(peak.body) > 0)
})

test('replacing a behavior releases face and body together', () => {
  const expression = new PerformanceExpressionController()
  playUnits(expression, [cue('greet')])
  expression.sample(0.2)
  playUnits(expression, [], 0.2)
  const settled = expression.sample(3)
  assert.equal(Math.abs(settled.brow) + Math.abs(settled.angleY) + Math.abs(settled.body), 0)
})

function poseMagnitude(
  offset: Readonly<Record<string, number | undefined>>,
): number {
  return Object.values(offset).reduce<number>(
    (total, value) => total + Math.abs(value ?? 0),
    0,
  )
}

const heldDirective: PerformanceDirective = {
  phase: 'delivery',
  moodRevision: 1,
  motionStyle: 'even',
  plan: {
    cues: [
      {
        intent: 'emphasize',
        atMs: 0,
        intensity: 1.1,
        tempo: 1,
        fadeInMs: 120,
        fadeOutMs: 200,
        interrupt: 'replace',
      },
    ],
  },
}

test('a performance unit holds through its stroke plateau, not up to it', () => {
  const plan = compilePerformanceBehaviorPlan(heldDirective, 0, 'held')
  const realized = realizeAnime25DBehaviorPlan(plan, 0)
  const unit = realized.units[0]!
  // The planner puts up to 80ms between the peak and the end of the stroke.
  // Packing the lifecycle into a cue measured the hold from strokeEnd, so that
  // plateau was silently dropped from every performance behavior.
  const plateau = (unit.timing.strokeEndMs - unit.timing.strokePeakMs) / 1_000
  assert.ok(plateau > 0, 'this plan has no plateau to protect')

  const controller = new PerformanceExpressionController()
  controller.playBehaviorUnits(realized.units, 0, 0)
  const peak = poseMagnitude(controller.sample(unit.timing.strokePeakMs / 1_000))
  assert.ok(peak > 0)
  const atRelax = poseMagnitude(controller.sample(unit.timing.relaxMs! / 1_000))
  assert.ok(
    Math.abs(atRelax - peak) < 1e-6,
    `full amplitude ended early: ${atRelax} at relax vs ${peak} at peak`,
  )
  assert.ok(poseMagnitude(controller.sample(unit.timing.endMs! / 1_000 - 0.01)) > 0)
  assert.equal(poseMagnitude(controller.sample(unit.timing.endMs! / 1_000)), 0)
})

test('restating the same units keeps the pose instead of replaying it', () => {
  const plan = compilePerformanceBehaviorPlan(heldDirective, 0, 'restated')
  const realized = realizeAnime25DBehaviorPlan(plan, 0)
  const unit = realized.units[0]!
  const peakSeconds = unit.timing.strokePeakMs / 1_000

  const once = new PerformanceExpressionController()
  once.playBehaviorUnits(realized.units, 0, 0)
  const expected = poseMagnitude(once.sample(peakSeconds))

  const restated = new PerformanceExpressionController()
  restated.playBehaviorUnits(realized.units, 0, 0)
  restated.sample(peakSeconds / 2)
  // A plan revision restates every live behavior. Scheduling it a second time
  // would stack the same pose on itself; dropping it would freeze the face.
  restated.playBehaviorUnits(realized.units, peakSeconds / 2, peakSeconds * 500)
  assert.equal(poseMagnitude(restated.sample(peakSeconds)), expected)
})

test('a behavior dropped from the plan releases instead of playing on', () => {
  const plan = compilePerformanceBehaviorPlan(heldDirective, 0, 'dropped')
  const realized = realizeAnime25DBehaviorPlan(plan, 0)
  const unit = realized.units[0]!
  const controller = new PerformanceExpressionController()
  controller.playBehaviorUnits(realized.units, 0, 0)
  const peakSeconds = unit.timing.strokePeakMs / 1_000
  assert.ok(poseMagnitude(controller.sample(peakSeconds)) > 0)
  controller.playBehaviorUnits([], peakSeconds, peakSeconds * 1_000)
  const released = unit.timing.endMs! / 1_000
  assert.equal(poseMagnitude(controller.sample(released)), 0)
})

test('a beat restated with more force reaches the face, not only the body', () => {
  const stronger: PerformanceDirective = {
    ...heldDirective,
    plan: {
      cues: [{ ...heldDirective.plan.cues[0]!, intensity: 1.4 }],
    },
  }
  const floor = realizeAnime25DBehaviorPlan(
    compilePerformanceBehaviorPlan(heldDirective, 0, 'performance'),
    0,
  )
  const refined = realizeAnime25DBehaviorPlan(
    compilePerformanceBehaviorPlan(stronger, 0, 'performance'),
    0,
  )
  // Stable ids are what let the scheduler carry a beat across a refinement.
  // The expression controller keys on the same id, so it has to notice that
  // the beat behind that id changed — otherwise the body follows Lite while
  // the face keeps playing the deterministic floor.
  assert.equal(refined.units[0]?.behaviorId, floor.units[0]?.behaviorId)
  assert.ok(refined.units[0]!.intensity > floor.units[0]!.intensity)

  const peakSeconds = floor.units[0]!.timing.strokePeakMs / 1_000
  const controller = new PerformanceExpressionController()
  controller.playBehaviorUnits(floor.units, 0, 0)
  controller.sample(peakSeconds / 2)
  const weak = poseMagnitude(controller.sample(peakSeconds))

  const refinedController = new PerformanceExpressionController()
  refinedController.playBehaviorUnits(floor.units, 0, 0)
  refinedController.sample(peakSeconds / 2)
  // The wall clock and the player clock must agree, the way the player passes
  // them: `nowMs` is the same instant as `peakSeconds / 2`.
  refinedController.playBehaviorUnits(
    refined.units,
    peakSeconds / 2,
    peakSeconds * 500,
  )
  assert.ok(
    poseMagnitude(refinedController.sample(peakSeconds)) > weak,
    'the refinement never reached the pose',
  )
})

test('a beat that leaves the plan and returns is scheduled again', () => {
  const realized = realizeAnime25DBehaviorPlan(
    compilePerformanceBehaviorPlan(heldDirective, 0, 'performance'),
    0,
  )
  const unit = realized.units[0]!
  const controller = new PerformanceExpressionController()
  controller.playBehaviorUnits(realized.units, 0, 0)
  // Release clears the signature, so an identical restatement is not mistaken
  // for the cue that is already fading out.
  controller.playBehaviorUnits([], 0.05, 50)
  controller.playBehaviorUnits(realized.units, 0.05, 50)
  assert.ok(poseMagnitude(controller.sample(unit.timing.strokePeakMs / 1_000)) > 0)
})
