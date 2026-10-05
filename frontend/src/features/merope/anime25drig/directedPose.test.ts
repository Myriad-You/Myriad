import type { BodyControl, BodyPose } from '../../../services/agent/types'
import type { Anime25DRenderFrame } from './renderer'
import type { Anime25DPlayback, Anime25DPlaybackLayer } from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { BODY_CONTROLS, sanitizeBodyPose } from '../events/performanceContract'
import { TouchGestureTracker } from '../interaction/touchGesture'
import { RigMotionCoordinator } from '../motion/coordinator'
import { compilePerformanceBehaviorPlan } from '../motion/performanceBehaviorPlan'
import { IDLE_MOTION_POLICY, policyFromOwners } from '../motion/policy'
import { TouchMotionSource } from '../motion/touchSource'
import { completeBehaviorQuality } from './behaviorMotion'
import { realizeAnime25DBehaviorPlan } from './behaviorRealizer'
import { Anime25DBodyFrames } from './bodyFrames'
import { applyBodyLift } from './bodyLift'
import { ClosedEyePresentation } from './closedEyePresentation'
import { BODY_CONTROL_DRIVERS, DirectedPoseController, INDEPENDENT_BODY_CONTROLS } from './directedPose'
import { IDENTITY_DRIVER } from './driver'
import { Anime25DIrisRebound } from './irisRebound'
import { deformAnime25DUpstreamFeaturePoint } from './layerDeformation'
import { Anime25DMotionComposer } from './motionComposer'
import { deriveAnime25DMotionEnvelopeProfile } from './motionEnvelope'
import { intentExpressionOffset } from './performanceExpression'
import { ZERO_OFFSET } from './performanceExpressionShared'
import { PoseResponseController } from './poseResponse'
import { deriveAnime25DShellProfile } from './shellProfile'
import { anime25DPlaybackSource } from './types'

const capabilities = [...new Set(Object.values(BODY_CONTROLS).map(c => c.capability))]
const pose = (targets: BodyPose['targets'], holdMs = 0, transitionMs = 300): BodyPose => ({ targets, holdMs, transitionMs })

function controller() {
  const value = new DirectedPoseController()
  value.setCapabilities(capabilities)
  return value
}

function advance(value: DirectedPoseController, seconds: number, start = 0, fps = 120) {
  let target = { ...IDENTITY_DRIVER }
  for (let i = 1; i <= Math.round(seconds * fps); i++) {
    target = { ...IDENTITY_DRIVER }
    value.apply(1 / fps, start + i / fps, target, false)
  }
  return target
}

test('body contract rejects arbitrary drivers/nonfinite targets and keeps explicit release', () => {
  assert.equal(sanitizeBodyPose({ targets: { angleX: 1 } }), null)
  assert.equal(sanitizeBodyPose({ targets: { headTurn: NaN } }), null)
  assert.equal(sanitizeBodyPose({ targets: {}, holdMs: -1 }), null)
  assert.equal(sanitizeBodyPose({ targets: {}, driver: {} }), null)
  assert.deepEqual(sanitizeBodyPose({ targets: {} }), { targets: {}, holdMs: 5000, transitionMs: 300 })
  assert.deepEqual(sanitizeBodyPose({ targets: { headTurn: 99, eyeOpenLeft: -3 }, transitionMs: 0, holdMs: 99000 }),
    { targets: { headTurn: 1, eyeOpenLeft: 0 }, transitionMs: 80, holdMs: 15000 })
})

test('every declared control has a real numeric runtime mapping and reaches its target', () => {
  assert.deepEqual(Object.keys(BODY_CONTROL_DRIVERS).sort(), Object.keys(BODY_CONTROLS).sort())
  for (const key of Object.keys(BODY_CONTROLS) as BodyControl[]) {
    const value = controller()
    const goal = BODY_CONTROLS[key].min < 0 ? -0.7 : 0.7
    value.set(pose({ [key]: goal }), 0)
    const target = advance(value, 1)
    const expected = key === 'gazeVertical' ? -goal : key === 'hairSway' ? goal * 1.5 : key === 'chestSway' ? goal * 4 : goal
    assert.ok(Math.abs(target[BODY_CONTROL_DRIVERS[key]] - expected) < 0.0001, key)
  }
})

test('semantic look-up actually moves the iris up in the renderer image coordinates', () => {
  const value = controller()
  value.set(pose({ gazeVertical: 0.8, gazeHorizontal: -0.5 }), 0)
  const target = advance(value, 1)
  const eye = { x0: 50, x1: 100, y0: 70, y1: 100, icx: 75, icy: 85, closeY: 95 }
  const point = { x: eye.icx, y: eye.icy }
  deformAnime25DUpstreamFeaturePoint(point, { kind: 'eye-open-iris', side: 'L', eye,
    centerX: 75, centerY: 85, faceScale: 1, expression: target })
  assert.ok(point.y < eye.icy && point.x < eye.icx)
})

test('opposed head/torso, fixed gaze and one-sided arms remain independent', () => {
  const value = controller()
  value.set(pose({ headTurn: -0.9, torsoTurn: 0.8, gazeHorizontal: 0, torsoPitch: 0.65,
    torsoRise: -0.5, leftArmRaise: 0.85, rightArmRaise: 0, rightArmSwing: -0.4 }), 0)
  const target = advance(value, 1)
  assert.ok(target.angleX < -0.89 && target.torsoTurn > 0.79)
  assert.ok(target.bodyPitch > 0.64 && target.bodyLift < -0.49)
  assert.equal(target.eyeX, 0)
  assert.ok(target.armRaiseL > 0.84)
  assert.equal(target.armRaiseR, 0)
  assert.ok(target.armSwingR < -0.39)
})

test('retargeting carries motion instead of jumping to rest or discarding velocity', () => {
  const value = controller()
  value.set(pose({ headTurn: 1 }), 0)
  const before = advance(value, 0.1)
  value.set(pose({ headTurn: -1 }), 0.1)
  const after = { ...IDENTITY_DRIVER }
  value.apply(1e-6, 0.100001, after, false)
  assert.ok(after.angleX > before.angleX, 'the old velocity still carries the turn on its first instant')
  assert.ok(Math.abs(after.angleX - before.angleX) < 0.0001)
  assert.ok(advance(value, 1, 0.100001).angleX < -0.99)
})

test('identical restatement does not restart expiry; explicit empty pose releases continuously', () => {
  const value = controller()
  const direction = pose({ headTilt: 0.8 }, 500)
  value.set(direction, 0)
  assert.ok(advance(value, 0.4).angleZ > 0.79)
  value.set(direction, 0.4)
  assert.ok(Math.abs(advance(value, 1, 0.4).angleZ) < 0.001)
  value.set(pose({ headTilt: -0.7 }), 1.4)
  assert.ok(advance(value, 0.5, 1.4).angleZ < -0.69)
  value.set(pose({}), 1.9)
  const initial = { ...IDENTITY_DRIVER }
  value.apply(1e-6, 1.900001, initial, false)
  assert.ok(initial.angleZ < -0.69)
  assert.ok(Math.abs(advance(value, 1, 1.900001).angleZ) < 0.001)
})

test('missing asset capability and preview ownership cannot be bypassed', () => {
  const value = new DirectedPoseController()
  value.setCapabilities(['head-body'])
  value.set(pose({ headTurn: 0.8, torsoTurn: 1, leftArmRaise: 1, cry: 1 }), 0)
  const target = advance(value, 1)
  assert.ok(target.angleX > 0.79)
  assert.equal(target.torsoTurn, 0)
  assert.equal(target.armRaiseL, 0)
  assert.equal(target.eyeCry, 0)
  value.setPolicy({ ...IDLE_MOTION_POLICY, headBody: 'preview' })
  const preview = advance(value, 0.1, 1)
  assert.equal(preview.angleX, 0)
  assert.equal(value.weight('headTurn'), 0)
})

test('speech retains articulation while pose can colour eyes, brows and smile', () => {
  const value = controller()
  value.set(pose({ mouthOpen: 1, mouthRound: 1, mouthSeal: 1, maniac: 1, mouthSmile: 0.7, eyeSmile: 0.8 }), 0)
  advance(value, 1)
  const target = { ...IDENTITY_DRIVER, mouthOpen: 0.25, mouthRound: 0.3, mouthSeal: 0.1 }
  value.apply(1 / 120, 1.01, target, true)
  assert.equal(target.mouthOpen, 0.25)
  assert.equal(target.mouthRound, 0.3)
  assert.equal(target.mouthSeal, 0.1)
  assert.equal(target.maniac, 0)
  assert.ok(target.mouthForm > 0.69 && target.eyeSmile > 0.79)
})

test('a held pose yields to immediate touch and resumes without replaying its goal', () => {
  const value = controller()
  value.set(pose({ headTurn: -0.8, gazeHorizontal: -0.7, eyeSmile: 0.8 }), 0)
  advance(value, 1)
  value.setPolicy({ ...IDLE_MOTION_POLICY, headBody: 'performance', gaze: 'performance', expression: 'performance' })
  value.setTouchShare(1)
  const touched = { ...IDENTITY_DRIVER, angleX: 0.4, eyeX: 0.3, eyeSmile: 0.2 }
  value.apply(1 / 120, 1.01, touched, false)
  assert.equal(touched.angleX, 0.4)
  assert.equal(touched.eyeX, 0.3)
  assert.equal(touched.eyeSmile, 0.2)
  assert.equal(value.weight('headTurn'), 0)
  value.setTouchShare(0)
  const resumed = advance(value, 0.1, 1.01)
  assert.ok(resumed.angleX < -0.79 && resumed.eyeX < -0.69 && resumed.eyeSmile > 0.79)
})

test('the production composer distinguishes listening from actual singing, humming and speech articulation', () => {
  for (const mode of ['listen', 'sing', 'hum'] as const) {
    for (const speechActive of [false, true]) {
      const current = { ...IDENTITY_DRIVER }
      const composer = new Anime25DMotionComposer(current, new ClosedEyePresentation(), new Anime25DIrisRebound())
      composer.directedPose.setCapabilities(capabilities)
      composer.directedPose.set(pose({ mouthOpen: 0.85, mouthRound: 0.9 }), 0)
      composer.behaviorMotion.replace([{
        behaviorId: `music:${mode}`, family: 'music', kind: 'rhythmic', form: mode,
        timing: { startMs: 0, readyMs: 0, strokeStartMs: 0, strokePeakMs: 0,
          strokeEndMs: 0, relaxMs: null, endMs: null },
        intensity: 1, quality: completeBehaviorQuality({}),
      }], 0, 0)
      const target = { ...IDENTITY_DRIVER, talk: false, blink: false, rand: false, idle: false,
        singing: true, mouthOpen: 0.25, mouthRound: 0.2 }
      const envelope = deriveAnime25DMotionEnvelopeProfile({ layers: [] })
      for (let i = 1; i <= 120; i++) { composer.step(1 / 120, {
        time: i / 120, target, policy: IDLE_MOTION_POLICY, mouse: { x: 0, y: 0, inside: false },
        speechActive, musicSignal: null, motionEnvelopeProfile: envelope,
      })
}
      const articulated = speechActive || mode !== 'listen'
      assert.ok(Math.abs(current.mouthOpen - (articulated ? 0.25 : 0.85)) < 0.001, `${mode}, speech=${speechActive}`)
      assert.ok(Math.abs(current.mouthRound - (articulated ? 0.2 : 0.9)) < 0.001, `${mode}, speech=${speechActive}`)
    }
  }
})

test('a brief cue borrows only its moving axes, then returns to the held pose without restarting its deadline', () => {
  const value = controller()
  value.set(pose({ headTurn: -0.8, headNod: 0.5, torsoTurn: 0.7, leftArmRaise: 0.8, eyeSmile: 0.6 }, 1800), 0)
  advance(value, 0.8)
  value.setPolicy({ ...IDLE_MOTION_POLICY, expression: 'performance', headBody: 'performance' })
  value.setTransientMotion({ ...ZERO_OFFSET, angleY: -0.2, eyeSmile: 0.8, armY: 0.4 }, 1)
  assert.equal(value.weight('headNod'), 0)
  assert.equal(value.weight('eyeSmile'), 0)
  assert.equal(value.weight('leftArmRaise'), 0)
  assert.ok(value.weight('headTurn') > 0.99 && value.weight('torsoTurn') > 0.99)
  value.setTransientMotion(ZERO_OFFSET, 0)
  const returned = advance(value, 0.2, 0.8)
  assert.ok(returned.angleY > 0.49 && returned.eyeSmile > 0.59 && returned.armRaiseL > 0.79)
  assert.ok(Math.abs(advance(value, 2, 1).angleY) < 0.001, 'the brief cue must not extend a timed body pose')
})

test('the production composer performs a real short cue over held axes and recovers to its ongoing goal', () => {
  const current = { ...IDENTITY_DRIVER }
  const composer = new Anime25DMotionComposer(current, new ClosedEyePresentation(), new Anime25DIrisRebound())
  composer.directedPose.setCapabilities(capabilities)
  composer.directedPose.set(pose({ headTurn: -0.65, headNod: 0, torsoTurn: 0.7, eyeSmile: 0, leftArmRaise: 0.7 }), 0)
  const target = { ...IDENTITY_DRIVER, talk: false, blink: false, rand: false, idle: false }
  const envelope = deriveAnime25DMotionEnvelopeProfile({ layers: [{ role: 'handwear' }] })
  const policy = { ...IDLE_MOTION_POLICY, expression: 'performance' as const, headBody: 'performance' as const }
  let time = 0
  const step = () => {
    time += 1 / 120
    composer.step(1 / 120, { time, target, policy, mouse: { x: 0, y: 0, inside: false },
      speechActive: false, musicSignal: null, motionEnvelopeProfile: envelope })
  }
  for (let i = 0; i < 120; i++) step()
  const plan = compilePerformanceBehaviorPlan({ phase: 'delivery', moodRevision: 1, motionStyle: 'open',
    plan: { cues: [{ intent: 'delight', atMs: 0, intensity: 1.3, tempo: 1, fadeInMs: 120,
      fadeOutMs: 300, interrupt: 'replace' }] } }, 1000, 'brief-over-held')
  const realized = realizeAnime25DBehaviorPlan(plan, 1000)
  composer.performanceExpression.playBehaviorUnits(realized.units, time, 1000)
  let nod = 0
  let smile = 0
  for (let i = 0; i < 120; i++) {
    step()
    nod = Math.max(nod, Math.abs(current.angleY))
    smile = Math.max(smile, current.eyeSmile)
    assert.ok(current.angleX < -0.64 && current.torsoTurn > 0.69, 'the cue never borrowed head yaw or torso yaw')
  }
  const authoredNod = Math.abs(intentExpressionOffset('delight', 1.3).angleY)
  assert.ok(nod > authoredNod * 0.9 && smile > 0.5, `${nod}, ${smile}: the held zero values must not erase the authored short reaction`)
  for (let i = 0; i < 240; i++) step()
  assert.ok(Math.abs(current.angleY) < 0.001 && current.eyeSmile < 0.001)
  assert.ok(current.armRaiseL > 0.69, 'the side arm goal remains alive after the cue')
})

test('constant directed goals integrate consistently across 30/60/120 fps', () => {
  const results = [30, 60, 120].map(fps => {
    const value = controller(); value.set(pose({ headTurn: 0.8, eyeOpenLeft: 0.1, torsoRise: 0.6 }), 0)
    return advance(value, 0.3, 0, fps)
  })
  for (const result of results) {
    for (const key of ['angleX', 'eyeOpenL', 'bodyLift'] as const)
      assert.ok(Math.abs(result[key] - results[0]![key]) < 1e-12, key)
  }
})

test('the production composer mixes each independent body goal once and filters geometry authority with the same response', () => {
  for (const fps of [30, 60, 120]) {
    const current = { ...IDENTITY_DRIVER }
    const composer = new Anime25DMotionComposer(current, new ClosedEyePresentation(), new Anime25DIrisRebound())
    composer.directedPose.setCapabilities(capabilities)
    const directed = controller()
    const reference = { ...IDENTITY_DRIVER }
    const authority = { ...IDENTITY_DRIVER }
    const authorityTarget = { ...IDENTITY_DRIVER }
    const response = new PoseResponseController()
    const authorityResponse = new PoseResponseController()
    const target = { ...IDENTITY_DRIVER, talk: false, blink: false, rand: false, idle: false }
    const envelope = deriveAnime25DMotionEnvelopeProfile({ layers: [{ role: 'handwear' }] })
    let time = 0
    for (const value of [0.4, -0.3, null]) {
      const next = value === null ? null : pose(Object.fromEntries(INDEPENDENT_BODY_CONTROLS.map(control => [control, value])))
      directed.set(next, time)
      composer.directedPose.set(next, time)
      for (let i = 0; i < fps / 5; i++) {
        time += 1 / fps
        const expected = { ...target }
        directed.apply(1 / fps, time, expected, false)
        response.step(reference, expected, 1 / fps)
        composer.step(1 / fps, { time, target, policy: IDLE_MOTION_POLICY,
          mouse: { x: 0, y: 0, inside: false }, speechActive: false,
          musicSignal: null, motionEnvelopeProfile: envelope })
        for (const control of INDEPENDENT_BODY_CONTROLS)
          authorityTarget[BODY_CONTROL_DRIVERS[control]] = directed.weight(control)
        authorityResponse.step(authority, authorityTarget, 1 / fps)
        for (const control of INDEPENDENT_BODY_CONTROLS) {
          const key = BODY_CONTROL_DRIVERS[control]
          assert.ok(Math.abs(current[key] - reference[key]) < 1e-12, `${control} at ${fps}fps: the goal must not be mixed twice`)
          assert.ok(Math.abs(composer.directedBodyWeight(control) - authority[key]) < 1e-12, `${control}: geometry must not read a raw ownership switch`)
        }
      }
    }
  }
})

test('production composer preserves broad independent targets while bounding high-collar pitch', () => {
  const current = { ...IDENTITY_DRIVER }
  const composer = new Anime25DMotionComposer(current, new ClosedEyePresentation(), new Anime25DIrisRebound())
  composer.directedPose.setCapabilities(capabilities)
  composer.directedPose.set(pose({ headTurn: -0.9, headNod: -1, torsoTurn: 0.8, torsoRise: 0.7,
    torsoPitch: 0.65, gazeHorizontal: 0, leftArmRaise: 0.9, rightArmRaise: 0, eyeOpenLeft: 0.1, eyeOpenRight: 1 }), 0)
  const envelope = deriveAnime25DMotionEnvelopeProfile({ layers: [{ role: 'collar-front' }, { role: 'handwear' }] })
  for (let i = 1; i <= 120; i++) { composer.step(1 / 120, {
    time: i / 120, target: { ...IDENTITY_DRIVER, talk: false, blink: false, mouse: true },
    policy: IDLE_MOTION_POLICY, mouse: { x: 1, y: 1, inside: true }, speechActive: false,
    musicSignal: null, motionEnvelopeProfile: envelope,
  })
}
  assert.ok(current.angleX < -0.75, 'director head target wins over opposite pointer gaze')
  assert.ok(Math.abs(current.angleY) <= 0.8 + 1e-6, 'high collar retains its safe pitch cap')
  assert.ok(current.torsoTurn > 0.7 && current.bodyLift > 0.69 && current.bodyPitch > 0.64)
  assert.ok(Math.abs(current.eyeX) < 0.1 && current.eyeOpenL < 0.15 && current.eyeOpenR > 0.9)
  assert.ok(current.armRaiseL > 0.85 && Math.abs(current.armRaiseR) < 0.001)
  composer.directedPose.set(null, 1)
  for (let i = 121; i <= 360; i++) { composer.step(1 / 120, {
    time: i / 120, target: { ...IDENTITY_DRIVER, talk: false, blink: false, rand: false },
    policy: IDLE_MOTION_POLICY, mouse: { x: 0, y: 0, inside: false }, speechActive: false,
    musicSignal: null, motionEnvelopeProfile: envelope,
  })
}
  assert.ok(Math.abs(current.torsoTurn) < 0.001 && Math.abs(current.bodyLift) < 0.001)
})

test('directed torso targets reach the production body shell and final-space volume field', () => {
  const layer: Anime25DPlaybackLayer = { name: 'topwear', role: 'topwear', z: 0, depth: 0.9,
    group: 'body', phys: null, fade: null, side: null, x: 80, y: 440, w: 600, h: 580,
    atlas: { x: 0, y: 0, w: 1, h: 1 }, strands: [] }
  const anchors = { face: { x0: 230, y0: 90, x1: 538, y1: 405, cx: 384, cy: 248 },
    mouth: { x0: 347, y0: 310, x1: 421, y1: 344, cx: 384, cy: 327 },
    neckPivot: { x: 384, y: 426 }, neckTop: 395, neckBottom: 482,
    bodyPivot: { x: 384, y: 1024 }, faceScale: 0.925 }
  const playback: Anime25DPlayback = { ...anime25DPlaybackSource(), pixelCanvas: { width: 768, height: 1024 },
    layers: [layer], anchors, shellProfile: deriveAnime25DShellProfile({ anchors, layers: [layer] }),
    mouthProfile: { version: 1, source: 'bounds-fallback', silhouettes: [], bridges: [] },
    chestProfile: { version: 2, source: 'gender-policy', enabled: false, centerX: 384, centerY: 650,
      radiusX: 140, radiusY: 90, visibleScale: 0, motionScale: 0, frequencyScale: 1,
      supportScale: 1, garmentMotionScale: 0, confidence: 1 } }
  const frame: Anime25DRenderFrame = { viewWidth: 768, viewHeight: 1024, bodyPivotX: 384,
    bodyPivotY: 1024, bodyRotationCosine: 1, bodyRotationSine: 0, bodyBendHeight: 542, time: 0, eyeCry: 0 }
  const current = { ...IDENTITY_DRIVER }
  const composer = new Anime25DMotionComposer(current, new ClosedEyePresentation(), new Anime25DIrisRebound())
  composer.directedPose.setCapabilities(capabilities)
  composer.directedPose.set(pose({ headTurn: -0.8, headNod: -0.8, torsoTurn: 0.8, torsoRise: 0.7, torsoPitch: 0.65 }), 0)
  const body = new Anime25DBodyFrames(current, frame, composer)
  const envelope = deriveAnime25DMotionEnvelopeProfile(playback)
  body.bind(playback, envelope)
  for (let i = 1; i <= 240; i++) {
    const time = i / 120
    composer.step(1 / 120, { time, target: { ...IDENTITY_DRIVER, talk: false, blink: false, rand: false },
      policy: IDLE_MOTION_POLICY, mouse: { x: 0, y: 0, inside: false }, speechActive: false,
      musicSignal: null, motionEnvelopeProfile: envelope })
    body.stepPosture(1 / 120, time)
    body.prepareHeadFrame(time)
  }
  assert.ok(current.angleX < 0 && current.angleY < 0)
  assert.ok(body.secondaryDeformationFrame.torsoShellRotation!.yawSine > 0.3, 'the actual torso shell opposes the head')
  assert.ok(Math.abs(frame.bodyLift!.amount - 0.7 * 15.75) < 0.01, 'automatic head-driven settling must not override directed rise')
  assert.ok(Math.abs(frame.bodyLift!.pitch! - 0.65 * 0.18) < 0.001, 'pitch reaches the final deformation field')
  const upper = { x: 500, y: 600 }
  applyBodyLift(upper, frame.bodyLift)
  assert.ok(upper.y < 600 && upper.x !== 500, 'the authored torso changes actual projected volume, not just a reported driver')
  const crop = { x: 500, y: anchors.bodyPivot.y }
  applyBodyLift(crop, frame.bodyLift)
  assert.deepEqual(crop, { x: 500, y: anchors.bodyPivot.y })
  // Real touch contact immediately takes the lease, but geometry must not add
  // all of its natural recruitment back before the held contribution retreats.
  const coordinator = new RigMotionCoordinator()
  const touch = new TouchMotionSource(coordinator, () => {})
  const tracker = new TouchGestureTracker()
  touch.update('panel', tracker.begin({ pointerId: 1, x: 0.2, y: -0.5, atMs: 2000, region: 'hair' })!, 2000)
  composer.performanceExpression.playBehaviorUnits(realizeAnime25DBehaviorPlan(touch.current()!, 2000).units, 2, 2000)
  let time = 2
  const drawn = () => [frame.bodyLift!.amount, frame.bodyLift!.pitch!, body.secondaryDeformationFrame.torsoShellRotation!.yawSine]
  const step = (dt: number) => {
    time += dt
    composer.step(dt, { time, target: { ...IDENTITY_DRIVER, talk: false, blink: false, rand: false },
      policy: policyFromOwners(coordinator.snapshot(time * 1000).owners), mouse: { x: 0, y: 0, inside: false },
      speechActive: false, musicSignal: null, motionEnvelopeProfile: envelope })
    body.stepPosture(dt, time)
    body.prepareHeadFrame(time)
  }
  const beforeTouch = drawn()
  step(1e-6)
  assert.equal(composer.directedPose.weight('torsoRise'), 0, 'contact really took the source authority')
  assert.ok(composer.directedBodyWeight('torsoRise') > 0.99, 'rendered authority follows the actual motion, not the new source switch')
  drawn().forEach((value, index) => assert.ok(Math.abs(value - beforeTouch[index]) < 1e-5, 'contact cannot pop the body projection'))
  for (let i = 0; i < 60; i++) step(1 / 120)
  assert.ok(composer.directedBodyWeight('torsoRise') < 0.001)
  const beforeReturn = drawn()
  touch.release()
  composer.performanceExpression.stopBehaviors(time)
  step(1e-6)
  drawn().forEach((value, index) => assert.ok(Math.abs(value - beforeReturn[index]) < 0.001, 'returning to the held pose cannot pop either'))
  for (let i = 0; i < 120; i++) step(1 / 120)
  assert.ok(Math.abs(frame.bodyLift!.amount - 0.7 * 15.75) < 0.01)
  assert.ok(body.secondaryDeformationFrame.torsoShellRotation!.yawSine > 0.3)

  composer.directedPose.set(null, time)
  const amount = frame.bodyLift!.amount
  step(1e-6)
  assert.ok(Math.abs(frame.bodyLift!.amount - amount) < 0.001, 'release does not snap the body field')
})

function sampleAt(value: DirectedPoseController, from: number, to: number, fps = 120) {
  const samples: Array<{ time: number; target: typeof IDENTITY_DRIVER }> = []
  for (let i = 1; i <= Math.round((to - from) * fps); i++) {
    const target = { ...IDENTITY_DRIVER }
    value.apply(1 / fps, from + i / fps, target, false)
    samples.push({ time: from + i / fps, target })
  }
  return samples
}

test('score beats take over held controls on their moment, hold, and give back to the standing pose', () => {
  const value = controller()
  value.set(pose({ headTilt: 0.2 }), 0)
  // Placed on the clock at 1000 ms; the player clock is at 0 s then.
  value.setScore({ id: 1, beats: [
    { id: '1:0', atMs: 1500, pose: pose({ headTilt: -0.6 }, 1000, 200) },
    { id: '1:1', atMs: 2000, pose: pose({ gazeHorizontal: 0.8 }, 0, 200) },
  ] }, 0, 1000)
  const samples = sampleAt(value, 0, 3)
  const at = (seconds: number) => samples.find((sample) => sample.time >= seconds)!.target
  assert.ok(Math.abs(at(0.45).angleZ - 0.2) < 0.03, 'the standing pose before the beat')
  assert.ok(at(0.95).angleZ < -0.5, 'the beat takes the tilt over')
  assert.ok(at(1.4).eyeX > 0.7, 'a later beat on another control joins it')
  assert.ok(Math.abs(at(2.9).angleZ - 0.2) < 0.05, 'after its hold the standing pose comes back')
  assert.ok(at(2.9).eyeX > 0.7, 'a beat with no hold keeps going')
})

test('a move goes and comes back over what is held; a new score drops what the old had not begun', () => {
  const value = controller()
  value.setScore({ id: 1, beats: [
    { id: '1:0', atMs: 0, move: { kind: 'nod', amount: 1, count: 1, tempo: 1 } },
    { id: '1:1', atMs: 2000, move: { kind: 'shake', amount: 1, count: 1, tempo: 1 } },
  ] }, 0, 0)
  const nod = sampleAt(value, 0, 0.6)
  assert.ok(Math.min(...nod.map((sample) => sample.target.angleY)) < -0.4, 'the head dips')
  assert.ok(Math.abs(nod.at(-1)!.target.angleY) < 0.02, 'and comes back')
  assert.ok(value.weight('headNod') === 0, 'a finished move no longer drives anything')
  // A new score before 2 s replaces the shake that had not started.
  value.setScore({ id: 2, beats: [{ id: '2:0', atMs: 2500, move: { kind: 'nod', amount: 1, count: 1, tempo: 1 } }] }, 1, 1000)
  const later = sampleAt(value, 1, 3.2)
  assert.ok(later.every((sample) => Math.abs(sample.target.angleX) < 1e-6), 'the old shake never plays')
  assert.ok(Math.min(...later.map((sample) => sample.target.angleY)) < -0.4, 'the new nod does')
})

test('a hand beat drives its arm through the directed arm weight, then lets go', () => {
  const value = controller()
  value.setScore({ id: 1, beats: [{ id: '1:0', atMs: 0, move: { kind: 'beat', side: 'left', amount: 1, count: 1, tempo: 1 } }] }, 0, 0)
  const samples = sampleAt(value, 0, 0.5)
  const peak = samples.reduce((best, sample) => (sample.target.armRaiseL > best.target.armRaiseL ? sample : best))
  assert.ok(peak.target.armRaiseL > 0.3 && peak.target.armRaiseR === 0)
  assert.equal(value.weight('leftArmRaise'), 0)
})
