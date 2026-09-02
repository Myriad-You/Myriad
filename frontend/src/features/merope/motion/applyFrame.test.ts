import type { PerformanceDirective } from '../../../services/agent/types'
import type { RigBearing } from './bearing'
import type { MotionFrame } from './intents'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { bearingDriverPatch } from '../anime25drig/performanceExpression'
import { applyMotionFrame, createMotionApplyState } from './applyFrame'
import { RigMotionCoordinator } from './coordinator'
import { compilePerformanceBehaviorPlan } from './performanceBehaviorPlan'
import { MotionRuntime } from './runtime'

function recordingRig() {
  const calls: string[] = []
  return {
    calls,
    rig: {
      setMotionPolicy: (policy: { mouth: string }) =>
        calls.push(`policy:${policy.mouth}`),
      setMood: (mood: number, activity: string) =>
        calls.push(`mood:${mood}:${activity}`),
      setBearing: (bearing: unknown) =>
        calls.push(`bearing:${bearing !== null}`),
      setSpeechActive: (value: boolean) => calls.push(`speechActive:${value}`),
      setAutoSpeech: (value: boolean) => calls.push(`auto:${value}`),
      setSpeechEnergy: (value: number | null) => calls.push(`energy:${value}`),
      setSpeechArticulation: (value: { viseme: string }) =>
        calls.push(`articulation:${value.viseme}`),
      setSpeechProsody: (value: { utteranceId: string } | null) =>
        calls.push(`prosody:${value?.utteranceId ?? 'none'}`),
      enqueueSpeechText: (text: string) => calls.push(`text:${text}`),
      playBehaviorPlan: (plan: { behaviors: readonly { id: string }[] }) => {
        calls.push('play')
        return plan.behaviors.map((behavior) => ({
          behaviorId: behavior.id,
          result: 'accepted' as const,
          atMs: 10,
        }))
      },
      stopBehaviorPlan: () => calls.push('stop'),
      setSinging: (value: boolean) => calls.push(`singing:${value}`),
      setSingingTrack: (value: string | null) => calls.push(`track:${value}`),
      setSingingSpectrum: (value: unknown) =>
        calls.push(`spectrum:${value !== null}`),
    },
  }
}

function frame(
  coordinator: RigMotionCoordinator,
  nowMs: number,
  extra: Partial<MotionFrame> = {},
): MotionFrame {
  const behaviorPlan =
    extra.behaviorPlan ??
    extra.performance?.behaviorPlan ??
    extra.speech?.behaviorPlan ??
    extra.music?.behaviorPlan ??
    null
  return {
    snapshot: coordinator.snapshot(nowMs),
    bearing: null,
    speech: null,
    performance: null,
    music: null,
    mood: null,
    behaviorPlan,
    behaviorRevision: behaviorPlan ? 1 : 0,
    behaviors: [],
    ...extra,
  }
}

const directive: PerformanceDirective = {
  phase: 'delivery',
  moodRevision: 1,
  motionStyle: 'even',
  plan: {
    baseline: {
      expression: 'warm',
      posture: 'neutral',
      motionEnergy: 1,
      attention: 1,
    },
    cues: [
      {
        intent: 'respond',
        atMs: 0,
        intensity: 1,
        tempo: 1,
        fadeInMs: 80,
        fadeOutMs: 120,
        interrupt: 'replace',
      },
    ],
  },
}

function performanceIntent(planId: string, startedAtMs: number) {
  return {
    directive,
    startedAtMs,
    motionIntentId: planId,
    behaviorPlan: compilePerformanceBehaviorPlan(
      directive,
      startedAtMs,
      planId,
    ),
  }
}

test('speech intent writes the mouth only while speech owns it', () => {
  const coordinator = new RigMotionCoordinator()
  coordinator.claim('speech', ['mouth'], { nowMs: 1 })
  const host = recordingRig()
  const state = createMotionApplyState()
  applyMotionFrame(
    host.rig,
    frame(coordinator, 1, {
      speech: {
        active: true,
        autoSpeech: true,
        energy: null,
        articulation: null,
        prosody: null,
        behaviorPlan: null,
        behaviors: [],
        queuedText: [{ seq: 1, text: '你好' }],
      },
    }),
    state,
  )
  assert.ok(host.calls.includes('speechActive:true'))
  assert.ok(host.calls.includes('auto:true'))
  assert.ok(host.calls.includes('text:你好'))
})

test('forwards one future prosody plan and clears it when speech yields', () => {
  const coordinator = new RigMotionCoordinator()
  const speech = coordinator.claim('speech', ['mouth'], { nowMs: 1 })
  const host = recordingRig()
  const state = createMotionApplyState()
  const prosody = {
    utteranceId: 'utt-1',
    startedAtMs: 10,
    durationMs: 500,
    accents: [{ offsetMs: 200, intensity: 0.8 }],
  }
  const intent = {
    active: true,
    autoSpeech: false,
    energy: null,
    articulation: null,
    prosody,
    behaviorPlan: null,
    behaviors: [],
    queuedText: [],
  }
  applyMotionFrame(host.rig, frame(coordinator, 1, { speech: intent }), state)
  applyMotionFrame(host.rig, frame(coordinator, 2, { speech: intent }), state)
  coordinator.release(speech)
  applyMotionFrame(host.rig, frame(coordinator, 3, { speech: intent }), state)
  assert.deepEqual(
    host.calls.filter((call) => call.startsWith('prosody:')),
    ['prosody:utt-1', 'prosody:none'],
  )
})

test('forwards an incremental prosody revision with the same utterance id', () => {
  const coordinator = new RigMotionCoordinator()
  coordinator.claim('speech', ['mouth'], { nowMs: 1 })
  const host = recordingRig()
  const state = createMotionApplyState()
  const base = {
    active: true,
    autoSpeech: false,
    energy: null,
    articulation: null,
    behaviorPlan: null,
    behaviors: [],
    queuedText: [],
  }
  const first = {
    utteranceId: 'utt-live',
    startedAtMs: 10,
    durationMs: 900,
    accents: [{ offsetMs: 200, intensity: 0.7 }],
  }
  applyMotionFrame(
    host.rig,
    frame(coordinator, 1, { speech: { ...base, prosody: first } }),
    state,
  )
  applyMotionFrame(
    host.rig,
    frame(coordinator, 2, {
      speech: {
        ...base,
        prosody: {
          ...first,
          accents: [...first.accents, { offsetMs: 620, intensity: 0.82 }],
        },
      },
    }),
    state,
  )
  assert.equal(
    host.calls.filter((call) => call === 'prosody:utt-live').length,
    2,
  )
})

test('mood and activity enter the rig through the shared frame', () => {
  const coordinator = new RigMotionCoordinator()
  const host = recordingRig()
  applyMotionFrame(
    host.rig,
    frame(coordinator, 1, {
      mood: { mood: 82, arousal: 48, activity: 'thinking' },
    }),
    createMotionApplyState(),
  )
  assert.ok(host.calls.includes('mood:82:thinking'))
})

test('a standing bearing is written even without a performance round', () => {
  const coordinator = new RigMotionCoordinator()
  const host = recordingRig()
  applyMotionFrame(
    host.rig,
    frame(coordinator, 1, {
      bearing: {
        expression: 'tense',
        posture: 'neutral',
        motionEnergy: 0.945,
        attention: 0.4,
        revision: 0,
      },
    }),
    createMotionApplyState(),
  )
  assert.ok(host.calls.includes('bearing:true'))
})

test('a low mood frame reaches the Anime2.5D driver as the sad standing face', () => {
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  runtime.mood.set(30, 'idle', 40)
  const host = recordingRig()
  let applied: ReturnType<typeof bearingDriverPatch> | null = null
  const rig = {
    ...host.rig,
    setBearing: (bearing: RigBearing | null) => {
      applied = bearingDriverPatch(bearing)
    },
  }

  applyMotionFrame(rig, runtime.frame(), createMotionApplyState())

  assert.ok(applied)
  assert.ok((applied.browAngSym ?? 0) <= -0.28)
  assert.ok((applied.eyeOpenL ?? 1) < 1)
  assert.ok((applied.mouthForm ?? 0) < -0.1)
  release()
})

test('a music frame forwards track identity before the groove sample', () => {
  const coordinator = new RigMotionCoordinator()
  coordinator.claim('music', ['mouth', 'headBody'], { nowMs: 1 })
  const host = recordingRig()
  applyMotionFrame(
    host.rig,
    frame(coordinator, 1, {
      music: {
        trackId: 'netease:123',
        apply: {
          release: false,
          writeGroove: true,
          writeMouth: true,
          restMouth: false,
        },
        spectrum: { bass: 0.4, beat: 0.5, vocal: 0.6 },
        articulation: { energy: 0.6, viseme: 'open', amount: 0.8 },
        behaviorPlan: null,
        behaviors: [],
      },
    }),
    createMotionApplyState(),
  )
  assert.ok(
    host.calls.indexOf('track:netease:123') <
      host.calls.indexOf('singing:true'),
  )
})

test('the same performance plan is not replayed on later frames', () => {
  const coordinator = new RigMotionCoordinator()
  coordinator.claim('performance', ['expression'], { nowMs: 1 })
  const host = recordingRig()
  const state = createMotionApplyState()
  const performance = performanceIntent('plan-1', 10)
  applyMotionFrame(host.rig, frame(coordinator, 1, { performance }), state)
  applyMotionFrame(host.rig, frame(coordinator, 2, { performance }), state)
  assert.equal(host.calls.filter((call) => call === 'play').length, 1)
})

test('clearing the performance intent stops the plan once', () => {
  const coordinator = new RigMotionCoordinator()
  const host = recordingRig()
  const state = createMotionApplyState()
  applyMotionFrame(
    host.rig,
    frame(coordinator, 1, { performance: performanceIntent('plan-1', 10) }),
    state,
  )
  applyMotionFrame(
    host.rig,
    frame(coordinator, 2, { performance: { directive: null, startedAtMs: 0 } }),
    state,
  )
  applyMotionFrame(
    host.rig,
    frame(coordinator, 3, { performance: { directive: null, startedAtMs: 0 } }),
    state,
  )
  assert.equal(host.calls.filter((call) => call === 'stop').length, 1)
})

test('a rejected plan reports once instead of retrying every frame', () => {
  const coordinator = new RigMotionCoordinator()
  coordinator.claim('performance', ['expression'], { nowMs: 1 })
  const host = recordingRig()
  const calls = host.calls
  let accept = false
  const rig = {
    ...host.rig,
    playBehaviorPlan: (plan: { behaviors: readonly { id: string }[] }) => {
      calls.push('play')
      return plan.behaviors.map((behavior) => ({
        behaviorId: behavior.id,
        result: accept ? ('accepted' as const) : ('rejected' as const),
        atMs: 30,
      }))
    },
  }
  const state = createMotionApplyState()
  const performance = performanceIntent('plan-1', 30)
  applyMotionFrame(rig, frame(coordinator, 1, { performance }), state)
  applyMotionFrame(rig, frame(coordinator, 2, { performance }), state)
  assert.equal(calls.filter((call) => call === 'play').length, 1)

  // A new semantic plan may be offered; changing renderer state alone may not
  // replay the rejected command without planner feedback.
  accept = true
  const nextPerformance = performanceIntent('next-plan', 31)
  applyMotionFrame(
    rig,
    frame(coordinator, 3, {
      performance: nextPerformance,
    }),
    state,
  )
  applyMotionFrame(
    rig,
    frame(coordinator, 4, { performance: nextPerformance }),
    state,
  )
  assert.equal(calls.filter((call) => call === 'play').length, 2)
})

test('the frame writer keeps exactly one behavior path', () => {
  const source = readFileSync(new URL('./applyFrame.ts', import.meta.url), 'utf8')
  // Motion reaches the body through the behavior protocol or not at all. A
  // second call site here is a second scheduler, which is what the protocol
  // exists to prevent; signals (speech text, audio spectrum) are inputs to a
  // generator and are deliberately not counted.
  const behaviorWrites = [...source.matchAll(/rig\.playBehaviorPlan\(/g)]
  assert.equal(
    behaviorWrites.length,
    1,
    `applyFrame plays ${behaviorWrites.length} behavior paths, expected 1`,
  )
  assert.match(source, /applyStanding\(rig, frame, state\)/)
  assert.match(source, /applySignals\(rig, frame, state\)/)
})
