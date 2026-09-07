import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { realizeAnime25DBehaviorPlan } from '../anime25drig/behaviorRealizer'
import { setLiveFaceVisible } from '../faceVisible'
import { RigMotionCoordinator } from '../motion/coordinator'
import { setLiveMotionGeneration } from '../motion/liveGeneration'
import { MotionRuntime } from '../motion/runtime'
import { Anime25DBodyAdapter } from './anime25dAdapter'

test('production adapter keeps run identity and does not label proactive performance as Chat', () => {
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  const body = new Anime25DBodyAdapter(runtime)
  setLiveFaceVisible(true)
  setLiveMotionGeneration(12)
  try {
    body.intend({
      messageId: 'notice',
      runId: 'notice-run',
      source: 'proactive',
      performance: {
        phase: 'proactive',
        moodRevision: 1,
        motionStyle: 'even',
        plan: {
          cues: [
            {
              intent: 'notify',
              atMs: 0,
              intensity: 1,
              tempo: 1,
              fadeInMs: 80,
              fadeOutMs: 200,
              interrupt: 'replace',
            },
          ],
        },
      },
    })
    const performance = runtime.frame().performance!
    const parameters = performance.behaviorPlan!.behaviors[0]!.form.parameters!
    assert.equal(
      parameters.performanceScope,
      JSON.stringify(['proactive', 0, 'notice-run']),
    )
    assert.equal(parameters.generation, 0)
    assert.equal(performance.generation, undefined)
  } finally {
    release()
    setLiveMotionGeneration(0)
  }
})

test('Anime2.5D adapter exposes semantic capabilities and state, not drivers', () => {
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  runtime.setCapabilities(['blink', 'head-body'])
  const body = new Anime25DBodyAdapter(runtime)
  assert.ok(body.capabilities().semantic.includes('head-body'))
  const state = body.state()
  assert.equal(state.expression, 'steady')
  assert.equal(typeof state.speaking, 'boolean')
  assert.equal('mouthOpen' in state, false)
  release()
})

test('adapter source never mentions Live2D or VRM placeholders', () => {
  const source = readFileSync(
    new URL('./anime25dAdapter.ts', import.meta.url),
    'utf8',
  )
  assert.doesNotMatch(source, /Live2D|VRM/)
  assert.doesNotMatch(source, /mouthOpen|angleX/)
})

test('live faces only show the master portrait when there is no playable rig', () => {
  const widget = readFileSync(
    new URL('../../../components/widgets/MeropeWidget.tsx', import.meta.url),
    'utf8',
  )
  const panel = readFileSync(
    new URL(
      '../../../components/agent-panel/AgentPanelFace.tsx',
      import.meta.url,
    ),
    'utf8',
  )
  for (const source of [widget, panel]) {
    assert.match(source, /fallbackUrl=\{playableRig \? null : portraitUrl\}/)
    assert.match(source, /onPlaybackError=\{handleRigPlaybackError\}/)
    assert.match(
      source,
      /const motionReady = (?:playsLive && )?playableRig && !rigFailed/,
    )
    assert.match(source, /ready: motionReady/)
    assert.match(source, /motionReady \? capabilities : \[\]/)
  }
})

test('static fallback cannot accumulate or report Anime2.5D motion', () => {
  const source = readFileSync(
    new URL('../rig/RigCharacter.tsx', import.meta.url),
    'utf8',
  )
  assert.match(source, /else if \(useAnimeRuntime\)/)
  assert.match(source, /slice\(-MAX_PENDING_SPEECH_CHUNKS\)/)
  assert.match(source, /if \(!useAnimeRuntime\)/)
  assert.match(source, /result: 'rejected'/)
  assert.doesNotMatch(source, /fallbackUrl=\{fallbackUrl\}/)
})

test('production chat and perception go through the body adapters', () => {
  const engine = readFileSync(
    new URL('../../../components/agent-panel/AgentEngine.tsx', import.meta.url),
    'utf8',
  )
  const panel = readFileSync(
    new URL('../../../components/GlobalControlPanel.tsx', import.meta.url),
    'utf8',
  )
  const arbitration = readFileSync(
    new URL('../faceSpeechArbitration.ts', import.meta.url),
    'utf8',
  )
  const captureCallers = [
    engine,
    panel,
    arbitration,
    readFileSync(new URL('./host.ts', import.meta.url), 'utf8'),
  ]
  for (const source of captureCallers) {
    assert.doesNotMatch(source, /capturePerceptionSnapshots/)
    assert.doesNotMatch(source, /runtime\.performance\.apply/)
  }
  assert.doesNotMatch(engine, /speakLine/)
  assert.doesNotMatch(panel, /speakLine/)
  assert.doesNotMatch(
    readFileSync(new URL('./host.ts', import.meta.url), 'utf8'),
    /speakLine/,
  )
  assert.match(arbitration, /speakUnmountedLine/)
  const adapter = readFileSync(
    new URL('./anime25dAdapter.ts', import.meta.url),
    'utf8',
  )
  assert.doesNotMatch(adapter, /capturePerceptionSnapshots/)
  const perception = readFileSync(
    new URL('./perceptionAdapter.ts', import.meta.url),
    'utf8',
  )
  assert.doesNotMatch(perception, / as PageContent/)
  assert.doesNotMatch(perception, /speakLine/)
})

test('hidden face does not pretend a body intent was played', () => {
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  const body = new Anime25DBodyAdapter(runtime)
  setLiveFaceVisible(false)
  body.intend({
    messageId: 'proactive-1',
    speechText: '想跟你说一声',
    performance: {
      phase: 'delivery',
      moodRevision: 1,
      motionStyle: 'even',
      plan: { cues: [] },
    },
  })
  assert.equal(runtime.frame().speech, null)
  assert.equal(runtime.frame().performance, null)
  setLiveFaceVisible(true)
  body.intend({
    performance: {
      phase: 'delivery',
      moodRevision: 1,
      motionStyle: 'even',
      plan: {
        cues: [
          {
            intent: 'listen',
            atMs: 0,
            intensity: 1,
            tempo: 1,
            fadeInMs: 80,
            fadeOutMs: 120,
            interrupt: 'if-lower',
          },
        ],
      },
    },
  })
  assert.equal(
    runtime.frame().performance?.directive?.plan.cues[0]?.intent,
    'listen',
  )
  release()
})

test('a production body intent reaches the Anime2.5D realizer and reports acceptance', () => {
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  const body = new Anime25DBodyAdapter(runtime)
  setLiveFaceVisible(true)

  body.intend({
    performance: {
      phase: 'reaction',
      moodRevision: 4,
      motionStyle: 'open',
      plan: {
        cues: [
          {
            intent: 'respond',
            atMs: 0,
            intensity: 1.15,
            tempo: 1,
            fadeInMs: 105,
            fadeOutMs: 420,
            interrupt: 'if-lower',
          },
        ],
      },
    },
  })

  const frame = runtime.frame()
  assert.equal(frame.performance?.directive?.plan.cues[0]?.intent, 'respond')
  assert.ok(frame.behaviorPlan)
  const realized = realizeAnime25DBehaviorPlan(
    frame.behaviorPlan,
    performance.now(),
  )
  assert.equal(realized.units[0]?.family, 'performance')
  assert.equal(realized.units[0]?.form, 'respond')
  assert.ok(realized.reports.length > 0)
  assert.ok(realized.reports.every((report) => report.result === 'accepted'))
  for (const report of realized.reports) {
    runtime.reportBehaviorRealizer(
      frame.behaviorPlan.id,
      report.behaviorId,
      report.result,
      report.atMs,
      report.reason,
    )
  }
  assert.ok(
    runtime
      .frame()
      .performance?.behaviors?.every(
        (behavior) => behavior.phase !== 'rejected',
      ),
  )

  release()
})

test('a cancelled reply clears its transient performance and bearing', () => {
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  const body = new Anime25DBodyAdapter(runtime)
  setLiveFaceVisible(true)

  body.intend({
    messageId: 'message-cancel',
    performance: {
      phase: 'delivery',
      moodRevision: 4,
      motionStyle: 'open',
      plan: {
        baseline: {
          expression: 'warm',
          posture: 'open',
          motionEnergy: 1,
          attention: 0.9,
        },
        cues: [
          {
            intent: 'respond',
            atMs: 0,
            intensity: 1.1,
            tempo: 1,
            fadeInMs: 100,
            fadeOutMs: 360,
            interrupt: 'if-lower',
          },
        ],
      },
    },
  })
  assert.ok(runtime.frame().performance)
  assert.equal(runtime.frame().bearing?.expression, 'warm')

  runtime.performance.handleSpeech({
    phase: 'cancel',
    source: 'reply',
    messageId: 'message-cancel',
  })

  assert.equal(runtime.frame().performance, null)
  assert.equal(runtime.frame().bearing?.expression, 'steady')
  release()
})
