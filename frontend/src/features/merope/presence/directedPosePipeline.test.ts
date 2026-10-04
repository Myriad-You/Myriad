import type { PerformanceDirective } from '../../../services/agent/types'
import type { RigMotionPort } from '../rig/motionPort'
import assert from 'node:assert/strict'
import test from 'node:test'
import { realizeAnime25DBehaviorPlan } from '../anime25drig/behaviorRealizer'
import { ClosedEyePresentation } from '../anime25drig/closedEyePresentation'
import { IDENTITY_DRIVER } from '../anime25drig/driver'
import { Anime25DIrisRebound } from '../anime25drig/irisRebound'
import { Anime25DMotionComposer } from '../anime25drig/motionComposer'
import { deriveAnime25DMotionEnvelopeProfile } from '../anime25drig/motionEnvelope'
import { bearingDriverPatch } from '../anime25drig/performanceExpression'
import { meropePerformanceEventDetail } from '../events/performanceEvents'
import { applyMotionFrame, createMotionApplyState } from '../motion/applyFrame'
import { RigMotionCoordinator } from '../motion/coordinator'
import { IDLE_MOTION_POLICY } from '../motion/policy'
import { MotionRuntime } from '../motion/runtime'
import { AgentFaceChannel } from './agentFaceChannel'

test('model pose → event → lifecycle → bearing → production writer → composer; cancellation releases it', () => {
  const runtime = new MotionRuntime(new RigMotionCoordinator())
  const release = runtime.retain()
  const current = { ...IDENTITY_DRIVER }
  const target = { ...IDENTITY_DRIVER, talk: false, blink: false, rand: false }
  const composer = new Anime25DMotionComposer(current, new ClosedEyePresentation(), new Anime25DIrisRebound())
  composer.directedPose.setCapabilities(['head-body', 'torso-volume', 'left-arm', 'right-arm'])
  let time = 0
  let policy = IDLE_MOTION_POLICY
  let writes = 0
  const port: RigMotionPort = {
    setMotionPolicy: value => { policy = value },
    setBearing: value => {
      writes++
      Object.assign(target, bearingDriverPatch(value))
      composer.directedPose.set(value?.pose, time)
    },
    setMood: () => {}, setSpeechActive: () => {}, setAutoSpeech: () => {},
    setSpeechEnergy: () => {}, setSpeechArticulation: () => {}, setSpeechProsody: () => {}, enqueueSpeechText: () => {},
    setSinging: value => { target.singing = value }, setSingingTrack: () => {}, setMusicSignal: () => {},
    playBehaviorPlan: (plan) => {
      const nowMs = globalThis.performance.now()
      const realized = realizeAnime25DBehaviorPlan(plan, nowMs)
      composer.behaviorMotion.replace(realized.units, nowMs, time)
      composer.performanceExpression.playBehaviorUnits(realized.units, time, nowMs)
      return realized.reports
    },
    stopBehaviorPlan: () => { composer.behaviorMotion.clear(time); composer.performanceExpression.stopBehaviors(time) },
  }
  const channel = new AgentFaceChannel({
    performance: value => { const event = meropePerformanceEventDetail(value); if (event) runtime.performance.handle(event) },
    speech: event => runtime.performance.handleSpeech(event), utterance: () => {}, state: () => {},
  })
  const performance: PerformanceDirective = {
    phase: 'delivery', moodRevision: 999, motionStyle: 'open',
    plan: { baseline: { expression: 'steady', posture: 'neutral', attention: 1, motionEnergy: 1.3,
      pose: { targets: { headTurn: -0.85, torsoTurn: 0.8, torsoRise: 0.7, leftArmRaise: 0.9, rightArmRaise: 0 }, transitionMs: 250, holdMs: 0 } }, cues: [] },
  }
  const state = createMotionApplyState()
  const envelope = deriveAnime25DMotionEnvelopeProfile({ layers: [{ role: 'handwear' }] })
  const advance = (seconds: number) => {
    for (let i = 0; i < seconds * 120; i++) {
      time += 1 / 120
      applyMotionFrame(port, runtime.frame(), state)
      composer.step(1 / 120, { time, target, policy, mouse: { x: 0, y: 0, inside: false },
        speechActive: false, musicSignal: null, motionEnvelopeProfile: envelope })
    }
  }
  try {
    channel.deliver({ messageId: 'directed-reply', performance })
    assert.deepEqual(runtime.frame().bearing?.pose, performance.plan.baseline!.pose)
    advance(1)
    assert.equal(writes, 1, 'a stable bearing must not restart the pose every render frame')
    assert.ok(current.angleX < -0.84 && current.torsoTurn > 0.79 && current.bodyLift > 0.69)
    assert.ok(current.armRaiseL > 0.89 && Math.abs(current.armRaiseR) < 0.0001)
    channel.deliver({ messageId: 'directed-reply', performance: { ...performance, plan: { baseline: {
      ...performance.plan.baseline!, pose: { targets: { torsoRise: -0.7, headTurn: 0.8 }, transitionMs: 250, holdMs: 0 },
    }, cues: [] } } })
    const before = current.angleX
    advance(1 / 120)
    assert.ok(Math.abs(current.angleX - before) < 0.05, 'revision starts at the actual ongoing pose')
    advance(1)
    assert.ok(current.angleX > 0.79 && current.bodyLift < -0.69)
    runtime.performance.handleSpeech({ phase: 'cancel', messageId: 'directed-reply', source: 'reply' })
    advance(1)
    assert.ok(Math.abs(current.torsoTurn) < 0.001 && Math.abs(current.bodyLift) < 0.001)
  } finally { release() }
})
