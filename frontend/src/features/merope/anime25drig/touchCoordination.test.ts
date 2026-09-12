import type { Anime25DMotionUnit } from './behaviorMotion'
import assert from 'node:assert/strict'
import test from 'node:test'
import { TouchGestureTracker } from '../interaction/touchGesture'
import { applySingingWrite } from '../motion/applySnapshot'
import { RigMotionCoordinator } from '../motion/coordinator'
import { HumanPerformanceRuntime } from '../motion/humanPerformanceRuntime'
import { resolveSingingApply } from '../motion/singingApply'
import { TouchMotionSource } from '../motion/touchSource'
import { musicSignalAt } from '../singing/musicSignal.test-support'
import { restSingingArticulation } from '../singing/singingClock'
import { SingingGrooveController } from '../singing/singingGroove'
import { completeBehaviorQuality } from './behaviorMotion'
import { realizeAnime25DBehaviorPlan } from './behaviorRealizer'
import { IDENTITY_DRIVER } from './driver'
import {
  deriveAnime25DMotionEnvelopeProfile,
  projectAnime25DMotionEnvelope,
} from './motionEnvelope'
import { PerformanceExpressionController } from './performanceExpression'
import { PoseGateController, resolvePoseGate } from './poseArbitration'
import { composeOccupancyOffsets } from './poseCompositor'
import { occupancyTargets } from './poseOccupancy'
import { PoseResponseController } from './poseResponse'

function unit(
  family: 'performance' | 'touch',
  form: string,
): Anime25DMotionUnit {
  return {
    behaviorId: family,
    family,
    form,
    kind: 'state',
    intensity: 1,
    quality: completeBehaviorQuality({ fluidity: 0.9 }),
    timing: {
      startMs: 0,
      readyMs: 0,
      strokeStartMs: 0,
      strokePeakMs: 120,
      strokeEndMs: 160,
      relaxMs: null,
      endMs: null,
    },
    touch:
      family === 'touch'
        ? { x: 0.8, y: 0, strokeX: 0.2, strokeY: 0, caress: 0.7 }
        : undefined,
  }
}

test('touch preserves authored emotional brows and special eyes while still acknowledging with gaze and body', () => {
  for (const form of ['angry', 'cry', 'speechless', 'maniac']) {
    const alone = new PerformanceExpressionController()
    const together = new PerformanceExpressionController()
    alone.playBehaviorUnits([unit('performance', form)], 0, 0)
    together.playBehaviorUnits(
      [unit('performance', form), unit('touch', 'accept')],
      0,
      0,
    )
    const base = { browAngSym: 0.8, mouthForm: -0.5 }
    for (let i = 0; i <= 120; i++) {
      const a = alone.sample(i / 60, base)
      const b = together.sample(i / 60, base)
      const original = base.browAngSym + a.browAngSym
      if (Math.abs(original) >= 0.6) {
        assert.ok(
          Math.sign(original) * (base.browAngSym + b.browAngSym) >=
            Math.abs(original) * 0.75,
        )
}
      for (const key of [
        'mouthForm',
        'eyeSqueeze',
        'eyeCry',
        'anger',
        'speechless',
        'maniac',
      ] as const)
        assert.equal(b[key], a[key])
    }
    assert.ok(together.sample(2, base).eyeX > alone.sample(2, base).eyeX + 0.1)
    assert.equal(
      together.getTouchShare(),
      0,
      'another directed beat keeps its priority',
    )
    assert.equal(
      together.getSampledTouch(),
      null,
      'an overridden touch must not be reported as the displayed response',
    )
  }
})

test('a touch release retains its family through the tail and does not momentarily become an exclusive director beat', () => {
  const expression = new PerformanceExpressionController()
  expression.playBehaviorUnits([unit('touch', 'accept')], 0, 0)
  expression.sample(1)
  assert.equal(expression.getTouchShare(), 1)
  expression.playBehaviorUnits([], 1, 1000)
  expression.sample(1)
  assert.equal(expression.getTouchShare(), 1)
  expression.sample(1.2)
  assert.ok(expression.getTouchShare() > 0 && expression.getTouchShare() < 1)
  expression.sample(2)
  assert.equal(expression.getTouchShare(), 0)
})

for (const strongExpression of [false, true]) {
  test(`speech and music survive ${strongExpression ? 'a strong expression with touch' : 'repeated hair contact'} at 30/60/120 fps`, () => {
    for (const fps of [30, 60, 120]) {
      const coordinator = new RigMotionCoordinator()
      coordinator.claim('speech', ['mouth'])
      coordinator.claim('coSpeech', ['expression'])
      coordinator.claim('music', ['headBody'])
      const source = new TouchMotionSource(coordinator, () => {})
      const tracker = new TouchGestureTracker()
      const scheduler = new HumanPerformanceRuntime()
      const expression = new PerformanceExpressionController()
      const mixer = new PoseGateController()
      const groove = new SingingGrooveController()
      groove.setArmMotion(true)
      const response = new PoseResponseController()
      const displayed = { ...IDENTITY_DRIVER }
      const profile = deriveAnime25DMotionEnvelopeProfile({
        layers: [{ role: 'handwear' }],
      })
      let previousBody = 0
      let retainedMotion = 0
      const occupancy = occupancyTargets({
        speaking: true,
        singing: true,
        thinking: false,
        pointerDriven: false,
        automation: true,
        sticker: 0,
      })
      let previousGroove = 1
      for (let i = 0; i <= fps * 5; i++) {
        const now = (i * 1000) / fps
        const point = {
          pointerId: 1,
          atMs: now,
          x: 0.12 * Math.sin(now / 500),
          y: 0,
          region: 'hair' as const,
        }
        if (i === fps) source.update('panel', tracker.begin(point)!, now)
        else if (i > fps && i < fps * 3)
          source.update('panel', tracker.update(point)!, now)
        if (i === fps * 3) source.update('panel', tracker.cancel(now)!, now)
        const frame = scheduler.frame([source.current()], now)
        const units = frame.plan
          ? realizeAnime25DBehaviorPlan(frame.plan, now).units
          : []
        if (strongExpression && i >= fps && i < fps * 3) {
          const directed = unit('performance', 'maniac')
          directed.timing = {
            ...directed.timing,
            startMs: 1000,
            readyMs: 1100,
            strokeStartMs: 1200,
            strokePeakMs: 1400,
            strokeEndMs: 1480,
          }
          units.push(directed)
        }
        expression.playBehaviorUnits(units, now / 1000, now)
        expression.sample(now / 1000)
        const policy = coordinator.snapshot(now).owners
        let singing = false
        let musicSignal: ReturnType<typeof musicSignalAt> | null = null
        applySingingWrite(
          {
            setSinging: (value) => {
              singing = value
            },
            setSingingTrack: (id) => groove.setTrack(id),
            setMusicSignal: (value) => {
              musicSignal = value
            },
            setSpeechArticulation: () =>
              assert.fail('music must not overwrite speech articulation'),
            setSpeechActive: () =>
              assert.fail('music must not release active speech'),
          },
          resolveSingingApply({
            gap: 'active',
            holdExpired: false,
            audioPaused: false,
            mouthOwner: policy.mouth,
            headBodyOwner: policy.headBody,
          }),
          {
            trackId: 'touch-coordination',
            signal: musicSignalAt(now / 1000),
            articulation: restSingingArticulation(),
          },
        )
        assert.equal(
          singing,
          true,
          'a temporary body owner must not stop musical evidence',
        )
        const musicPose = groove.sample(now / 1000, singing, musicSignal)
        const gate = mixer.sample(
          1 / fps,
          resolvePoseGate(policy, occupancy, {
            performance: 1,
            stylized: 1,
            randomAmbient: 1,
            touch: expression.getTouchShare(),
          }),
        )
        assert.equal(policy.mouth, 'speech')
        assert.equal(gate.speechMouth, 1)
        assert.equal(gate.grooveMouth, 0)
        // An exclusive expression changes the target share by 0.75, versus
        // 0.25 for touch; displayed-pose continuity is checked below as well.
        assert.ok(
          Math.abs(gate.groove.headBody - previousGroove) <
            (strongExpression ? 0.4 : 0.18),
          `${fps}: music must not switch abruptly`,
        )
        if (i >= fps * 2 && i < fps * 3) {
          assert.ok(gate.groove.headBody >= (strongExpression ? 0.24 : 0.7))
          if (!strongExpression) {
            assert.ok(gate.coSpeech.expression >= 0.55)
          } else {
            assert.ok(
              expression.sample(now / 1000).maniac > 0.5,
              'strong face remains visible',
            )
          }
        }
        if (i > fps * 4) assert.ok(gate.groove.headBody > 0.98)
        const offset = composeOccupancyOffsets([
          { offset: musicPose, weights: gate.groove },
          { offset: expression.sample(now / 1000), weights: gate.performance },
        ])
        const target = { ...IDENTITY_DRIVER, ...offset }
        projectAnime25DMotionEnvelope(target, profile, {
          clippedEnergy: 0,
          transferredEnergy: 0,
        })
        response.step(displayed, target, 1 / fps)
        assert.ok(
          Math.abs(displayed.body - previousBody) < 0.16,
          `${fps}: displayed body discontinuity`,
        )
        assert.ok(
          Math.abs(displayed.angleY) <= 1 && Math.abs(displayed.angleZ) <= 1,
        )
        if (i >= fps * 2 && i < fps * 3) {
          retainedMotion +=
            Math.abs(musicPose.body * gate.groove.headBody) / fps
        }
        previousBody = displayed.body
        previousGroove = gate.groove.headBody
      }
      source.release()
      assert.ok(
        retainedMotion > (strongExpression ? 0.025 : 0.08),
        `${fps}: music was weighted but had no actual movement`,
      )
    }
  })
}

test('touch sharing never overrides isolated preview channels', () => {
  const gate = resolvePoseGate(
    {
      mouth: 'preview',
      gaze: 'preview',
      expression: 'preview',
      headBody: 'preview',
    },
    occupancyTargets({
      speaking: false,
      singing: true,
      thinking: false,
      pointerDriven: false,
      automation: false,
      sticker: 0,
    }),
    { performance: 1, stylized: 1, randomAmbient: 1, touch: 1 },
  )
  assert.equal(gate.groove.headBody, 0)
  assert.equal(gate.coSpeech.expression, 0)
})
