import type { ScheduledBehavior } from './behavior'
import type { MotionFrame } from './intents'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { arbitrateFaceSpeech } from '../faceSpeechArbitration'
import { PERFORMANCE_CUE_INTENTS } from '../performanceContract'
import { musicSignalAt } from '../singing/musicSignal.test-support'
import { applyMotionFrame, createMotionApplyState } from './applyFrame'
import { applySingingWrite } from './applySnapshot'
import { RigMotionCoordinator } from './coordinator'
import { compilePerformanceBehaviorPlan } from './performanceBehaviorPlan'
import { BEHAVIOR_FUNCTIONS, BEHAVIOR_SOURCES } from './rigStateSummary'
import { resolveSingingApply } from './singingApply'
import { compileSpeechBehaviorPlan } from './speechBehaviorPlan'

function source(relative: string): string {
  return readFileSync(new URL(relative, import.meta.url), 'utf8')
}

function recordingRig() {
  const calls: string[] = []
  return {
    calls,
    rig: {
      setMotionPolicy: (policy: { mouth: string; expression: string }) =>
        calls.push(`policy:${policy.mouth}:${policy.expression}`),
      setMood: () => undefined,
      setBearing: () => undefined,
      setSpeechActive: (value: boolean) => calls.push(`speechActive:${value}`),
      setAutoSpeech: (value: boolean) => calls.push(`auto:${value}`),
      setSpeechEnergy: () => undefined,
      setSpeechArticulation: () => undefined,
      setSpeechProsody: () => undefined,
      enqueueSpeechText: (text: string) => calls.push(`text:${text}`),
      playBehaviorPlan: (plan: { behaviors: readonly { id: string }[] }) => {
        calls.push('play')
        return plan.behaviors.map((behavior) => ({
          behaviorId: behavior.id,
          result: 'accepted' as const,
          atMs: 0,
        }))
      },
      stopBehaviorPlan: () => calls.push('stop'),
      setSinging: (value: boolean) => calls.push(`singing:${value}`),
      setSingingTrack: () => undefined,
      setMusicSignal: () => undefined,
    },
  }
}

function frame(
  coordinator: RigMotionCoordinator,
  nowMs: number,
  extra: Partial<MotionFrame> = {},
): MotionFrame {
  return {
    snapshot: coordinator.snapshot(nowMs),
    bearing: null,
    speech: null,
    performance: null,
    music: null,
    mood: null,
    behaviorPlan: null,
    behaviorRevision: 0,
    behaviors: [],
    ...extra,
  }
}

test('Chat speech occupies only the mouth; music keeps head and body', () => {
  const coordinator = new RigMotionCoordinator()
  coordinator.claim('music', ['mouth', 'headBody'], { nowMs: 0 })
  coordinator.claim('speech', ['mouth'], { nowMs: 1 })
  const snapshot = coordinator.snapshot(1)
  assert.equal(snapshot.owners.mouth, 'speech')
  assert.equal(snapshot.owners.headBody, 'music')

  const apply = resolveSingingApply({
    gap: 'active',
    holdExpired: false,
    audioPaused: false,
    mouthOwner: snapshot.owners.mouth,
    headBodyOwner: snapshot.owners.headBody,
  })
  assert.equal(apply.writeMouth, false)
  assert.equal(apply.writeGroove, true)
  assert.equal(apply.release, false)

  const writes: string[] = []
  applySingingWrite(
    {
      setSingingTrack: (value) => writes.push(`track:${value}`),
      setSinging: (value) => writes.push(`singing:${value}`),
      setMusicSignal: (value) => writes.push(`signal:${value !== null}`),
      setSpeechArticulation: () => writes.push('articulation'),
      setSpeechActive: () => writes.push('speechActive'),
    },
    apply,
    {
      trackId: 'song-a',
      signal: musicSignalAt(1),
      articulation: { energy: 0.6, viseme: 'open', amount: 0.8 },
    },
  )
  assert.deepEqual(writes, ['track:song-a', 'singing:true', 'signal:true'])
})

test('background Work cannot take the visible Chat face', () => {
  assert.equal(
    arbitrateFaceSpeech({
      visibleMode: 'chat',
      incomingMode: 'work',
      chatUtteranceActive: false,
    }),
    'record-without-speech',
  )
  assert.equal(
    arbitrateFaceSpeech({
      visibleMode: 'work',
      incomingMode: 'work',
      chatUtteranceActive: true,
    }),
    'record-without-speech',
  )
  assert.equal(
    arbitrateFaceSpeech({
      visibleMode: 'chat',
      incomingMode: 'chat',
      chatUtteranceActive: false,
    }),
    'speak',
  )
})

test('applyFrame writes speech text only while speech owns the mouth', () => {
  const coordinator = new RigMotionCoordinator()
  coordinator.claim('music', ['mouth', 'headBody'], { nowMs: 0 })
  coordinator.claim('speech', ['mouth'], { nowMs: 1 })
  const host = recordingRig()
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
    createMotionApplyState(),
  )
  assert.ok(host.calls.includes('policy:speech:idle'))
  assert.ok(host.calls.includes('text:你好'))
})

test('production faces consume the snapshot; sources do not take a rig', () => {
  assert.doesNotMatch(source('./speechSource.ts'), /rigRef/)
  assert.doesNotMatch(source('./performanceSource.ts'), /rigRef/)
  assert.doesNotMatch(
    source('../useRigSingingLifecycle.ts'),
    /applySingingWrite/,
  )
  assert.doesNotMatch(source('../useRigSingingLifecycle.ts'), /rigRef/)
})

test('the production rig port stays renderer-neutral', () => {
  const port = source('../rig/motionPort.ts')
  assert.doesNotMatch(port, /Anime25D|setDriver|debugSnapshot/)
  assert.doesNotMatch(port, /PerformanceDirective|playMotionPlan/)
  assert.doesNotMatch(source('./applyFrame.ts'), /RigCharacter/)
  assert.doesNotMatch(source('./useRigMotionLifecycle.ts'), /RigCharacter/)
})

test('cue facts have one registry and dead motion channels stay removed', () => {
  const channels = source('./performanceChannels.ts')
  assert.doesNotMatch(channels, /speechless|maniac|lovestruck/)
  assert.match(channels, /performanceCueDefinition/)
  assert.doesNotMatch(source('./channels.ts'), /['"]physics['"]/)
})

test('workbench preview stays off the production coordinator', () => {
  const workbench = source('../anime25drig/Anime25DWorkbench.tsx')
  const studio = source('../SiteMotionWorkbench.tsx')
  assert.doesNotMatch(workbench, /useRigMotionLifecycle/)
  assert.doesNotMatch(workbench, /getRigMotionCoordinator/)
  assert.doesNotMatch(workbench, /getProductionMotionRuntime/)
  assert.doesNotMatch(studio, /useRigSingingLifecycle/)
  assert.doesNotMatch(studio, /getProductionMotionRuntime/)
})

test('agent turns send a semantic rig summary instead of per-frame drivers', () => {
  assert.doesNotMatch(source('./rigStateSummary.ts'), /mouthOpen/)
  assert.doesNotMatch(source('./rigStateSummary.ts'), /angleX/)
  assert.doesNotMatch(
    source('../anime25drig/player.ts'),
    /captureRigStateSummary/,
  )
})

test('idle self-motion does not masquerade as a semantic directed plan', () => {
  assert.doesNotMatch(source('./runtime.ts'), /AutonomyMotionSource/)
  assert.doesNotMatch(source('./applyFrame.ts'), /autonomy/)
  assert.doesNotMatch(source('./runtimeHost.ts'), /new MotionRuntime/)
})

test('the behavior vocabulary is exactly what a producer can emit', () => {
  const functions = new Set<string>()
  const sources = new Set<string>()
  const record = (plan: { behaviors: readonly ScheduledBehavior[] }): void => {
    for (const behavior of plan.behaviors) {
      functions.add(behavior.function)
      sources.add(behavior.source)
    }
  }
  for (const intent of PERFORMANCE_CUE_INTENTS) {
    record(
      compilePerformanceBehaviorPlan(
        {
          phase: 'delivery',
          moodRevision: 1,
          motionStyle: 'even',
          plan: {
            cues: [
              {
                intent,
                atMs: 0,
                intensity: 1,
                tempo: 1,
                fadeInMs: 120,
                fadeOutMs: 200,
                interrupt: 'replace',
              },
            ],
          },
        },
        0,
        'performance',
      ),
    )
  }
  record(
    compileSpeechBehaviorPlan({
      utteranceId: 'utt-1',
      startedAtMs: 0,
      durationMs: 800,
      accents: [{ offsetMs: 300, intensity: 0.8 }],
    }),
  )
  for (const [, name] of source('./musicSource.ts').matchAll(
    /\bfunction: '([A-Za-z]+)'/g,
  )) {
    functions.add(name!)
  }
  for (const [, name] of source('./musicSource.ts').matchAll(
    /\bsource: '([A-Za-z]+)'/g,
  )) {
    sources.add(name!)
  }

  assert.deepEqual(
    Iterator.from(functions).toArray().toSorted(),
    BEHAVIOR_FUNCTIONS.toSorted(),
  )
  assert.deepEqual(
    Iterator.from(sources).toArray().toSorted(),
    BEHAVIOR_SOURCES.toSorted(),
  )

  const contract = source(
    '../../../../../crates/myriad-merope/src/rig_state.rs',
  )
  assert.deepEqual(
    rustList(contract, 'RIG_STATE_BEHAVIOR_FUNCTIONS').toSorted(),
    Iterator.from(functions).toArray().toSorted(),
  )
  assert.deepEqual(
    rustList(contract, 'RIG_STATE_BEHAVIOR_SOURCES').toSorted(),
    Iterator.from(sources).toArray().toSorted(),
  )
})

function rustList(contract: string, name: string): string[] {
  const block = new RegExp(
    `${RegExp.escape(name)}: &\\[&str\\] = &\\[([^\\]]*)\\]`,
  ).exec(
    contract,
  )?.[1]
  assert.ok(block, `${name} missing from the director contract`)
  return Iterator.from(block.matchAll(/"([a-z.]+)"/gi))
    .map((match) => match[1]!)
    .toArray()
}
