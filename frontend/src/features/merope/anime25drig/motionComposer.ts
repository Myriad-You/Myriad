import type { MotionChannelPolicy } from '../motion/policy'
import type { MusicMotionSignal } from '../singing/musicSignal'
import type { ClosedEyePresentation } from './closedEyePresentation'
import type { IndependentBodyControl } from './directedPose'
import type { Anime25DDriver } from './driver'
import type {
  Anime25DBlinkState,
  Anime25DStylizedTargets,
} from './driverComposition'
import type { Anime25DIrisRebound } from './irisRebound'
import type { Anime25DMotionEnvelopeProfile } from './motionEnvelope'
import type { StylizedExpressionMotion } from './stylizedExpressionMotion'
import { allowsPointerGaze } from '../motion/policy'
import { SingingGrooveController } from '../singing/singingGroove'
import { AmbientMotionController } from './ambientMotion'
import { Anime25DBehaviorMotionController } from './behaviorMotion'
import { BODY_CONTROL_DRIVERS, DirectedPoseController, INDEPENDENT_BODY_CONTROLS } from './directedPose'
import { IDENTITY_DRIVER } from './driver'
import {
  applyAnime25DComposedPose,
  applyAnime25DCryMouth,
  applyAnime25DSillyMouthOwnership,
  applyAnime25DSpeechExtras,
  applyAnime25DStylizedExpression,
  prepareAnime25DWorkingTarget,
  resolveAnime25DStylizedTargets,
  smoothAnime25DUnit,
  stepAnime25DBlink,
  stepAnime25DDriverResponse,
} from './driverComposition'
import { expressiveEyeOpenOffset } from './expressiveMotionEnvelope'
import { GazeShiftBlink } from './gazeBlink'
import { idleBreathOffset } from './idleBreath'
import { projectAnime25DMotionEnvelope } from './motionEnvelope'
import { monotonicControlTime } from './motionPrediction'
import { PerformanceExpressionController } from './performanceExpression'
import {
  applyBehaviorMotionGate,
  PoseGateController,
  resolvePoseGate,
} from './poseArbitration'
import { zeroOccupancyOffset } from './poseCompositor'
import { PoseOccupancyController } from './poseOccupancy'
import {
  PoseResponseController,
  resolvePoseResponseScale,
} from './poseResponse'
import {
  DIRECTED_BODY_BLOCK_LEVEL,
  RandomActionController,
} from './randomAction'
import { CoSpeechExpressionController } from './speechExpression'
import { AutoSpeechController } from './speechMotion'
import { standingWeightShift } from './standing'
import { StylizedExpressionMotionController } from './stylizedExpressionMotion'
import { ThinkingMotionController } from './thinkingMotion'

const POINTER_ATTACK_RATE = 16
const POINTER_RELEASE_RATE = 5.5

/** What the composer reads each step; the player refills one instance. */
export interface MotionComposerInput {
  time: number
  /** The authored target: workbench drivers, bearing and directed state. */
  target: Anime25DDriver
  policy: MotionChannelPolicy
  mouse: { x: number; y: number; inside: boolean }
  speechActive: boolean
  musicSignal: MusicMotionSignal | null
  motionEnvelopeProfile: Anime25DMotionEnvelopeProfile
}

/**
 * Everything she does, composed into one driver. Speech, song, directed
 * performances, stylized faces, idle and random motion each propose an
 * offset; occupancy and the pose gate decide who owns which channel; the
 * envelope keeps the result drawable; and the response smoothing writes it
 * into `current`. Rendering reads `current` and the few outputs below.
 */
export class Anime25DMotionComposer {
  readonly directedPose = new DirectedPoseController()
  readonly performanceExpression = new PerformanceExpressionController()
  readonly behaviorMotion = new Anime25DBehaviorMotionController()
  readonly speechMotion = new AutoSpeechController()
  readonly speechExpression = new CoSpeechExpressionController()
  readonly singingGroove = new SingingGrooveController()
  readonly motionEnvelopeResult = {
    clippedEnergy: 0,
    transferredEnergy: 0,
  }

  /** The stylized face's own motion, which the head and mouth deformers read. */
  stylizedMotion: Readonly<StylizedExpressionMotion> | null = null
  /** How much of the stylized head pulse the gate lets through. */
  stylizedHeadShare = 1
  /** How far a sticker face owns the mouth once she stops talking. */
  sillyMouthShare = 1
  /** Speech's brow accent, which the jaw follows. */
  jawEmphasis = 0

  private readonly workingTarget: Anime25DDriver = { ...IDENTITY_DRIVER }
  private readonly expressionTarget: Anime25DDriver = { ...IDENTITY_DRIVER }
  private readonly poseResponse = new PoseResponseController()
  // Geometry mixes its automatic posture with already-weighted drivers. Its
  // remaining automatic share must follow the same response, not raw ownership.
  private readonly directedAuthority = { ...IDENTITY_DRIVER }
  private readonly directedAuthorityTarget = { ...IDENTITY_DRIVER }
  private readonly directedAuthorityResponse = new PoseResponseController()
  private unblinkedEyeOpenL = 1
  private unblinkedEyeOpenR = 1
  private readonly blinkState: Anime25DBlinkState = {
    activeSeconds: -1,
    nextAtSeconds: 1.8,
  }

  private readonly ambientMotion = new AmbientMotionController()
  /** A long look takes a blink with it. */
  private readonly gazeBlink = new GazeShiftBlink()
  private responseScale = 1
  private controlTime = 0
  private readonly occupancy = new PoseOccupancyController()
  private readonly poseGate = new PoseGateController()
  private readonly randomAction = new RandomActionController()
  private readonly thinkingMotion = new ThinkingMotionController()
  private readonly stylizedExpression = new StylizedExpressionMotionController()
  private readonly stylizedTargets: Anime25DStylizedTargets = {
    anger: 0,
    speechless: 0,
    maniac: 0,
    silly: 0,
    lovestruck: 0,
  }

  /** Reused so the per-frame pose composition never allocates. */
  private readonly composedPose = zeroOccupancyOffset()
  private readonly breathPose = { angleX: 0, angleY: 0, angleZ: 0, body: 0 }
  private standing = false
  private readonly cryMouth = {
    mouthOpen: 0,
    mouthForm: 0,
    mouthCY: 0,
    mouthScale: 0,
  }

  private pointerAuthority = 0

  constructor(
    /** The driver rendering reads; the composer is its only writer. */
    private readonly current: Anime25DDriver,
    private readonly closedEyes: ClosedEyePresentation,
    private readonly irisRebound: Anime25DIrisRebound,
  ) {}

  /** A standing figure also shifts its weight from leg to leg while idle. */
  setStanding(standing: boolean): void {
    this.standing = standing
  }

  directedBodyWeight(control: IndependentBodyControl): number {
    return this.directedAuthority[BODY_CONTROL_DRIVERS[control]]
  }

  /** Blink soon, then resume the natural rhythm. */
  blinkNow(time: number): void {
    this.blinkState.activeSeconds = 0
    this.blinkState.nextAtSeconds = time + 1.6 + Math.random() * 3.8
  }

  step(dt: number, input: MotionComposerInput): void {
    const t = input.time
    this.directedPose.setPolicy(input.policy)
    this.directedPose.setTouchShare(this.performanceExpression.getTouchShare())
    const target = input.target
    const pointer =
      target.mouse && allowsPointerGaze(input.policy.gaze)
        ? input.mouse
        : { x: 0, y: 0, inside: false }
    const pointerWanted = pointer.inside ? 1 : 0
    this.pointerAuthority +=
      (pointerWanted - this.pointerAuthority) *
      (1 -
        Math.exp(
          -(pointerWanted > this.pointerAuthority
            ? POINTER_ATTACK_RATE
            : POINTER_RELEASE_RATE) * dt,
        ))
    const tgt = prepareAnime25DWorkingTarget(
      this.workingTarget,
      target,
      pointer,
      this.pointerAuthority,
    )
    const controlTime = monotonicControlTime(
      this.controlTime,
      t,
      this.responseScale,
    )
    this.controlTime = controlTime
    const behaviorMotion = this.behaviorMotion.sample(controlTime)
    const speaking = input.speechActive || target.talk
    const singing = target.singing
    const vocalizing = speaking || (singing && (behaviorMotion.musicMode === 'sing' || behaviorMotion.musicMode === 'hum'))
    // Her standing acting gives way to speaking by itself, as she is taken up.
    this.directedPose.setSpeaking(vocalizing)
    this.directedPose.step(dt, t, tgt)
    const expressionTarget = Object.assign(this.expressionTarget, tgt)
    this.directedPose.write(expressionTarget, vocalizing)
    const semanticExpression = this.performanceExpression.sample(
      controlTime,
      expressionTarget,
      input.speechActive || target.talk,
    )
    this.directedPose.setTransientMotion(semanticExpression, this.performanceExpression.getActiveLevel())
    const stylizedTargets = resolveAnime25DStylizedTargets(
      this.stylizedTargets,
      expressionTarget,
      semanticExpression,
    )
    const stylized = this.stylizedExpression.sample(
      controlTime,
      stylizedTargets.anger,
      stylizedTargets.speechless,
      stylizedTargets.maniac,
      stylizedTargets.silly,
      stylizedTargets.lovestruck,
    )
    this.stylizedMotion = stylized
    const performanceMotionScale =
      this.performanceExpression.getAmbientMotionScale()
    const pointerDriven = target.mouse && pointer.inside
    const speech = this.speechMotion.sample(controlTime, target.talk)
    this.jawEmphasis = speech.browAccent
    const speechExpression = this.speechExpression.sample(
      controlTime,
      speaking,
      input.speechActive && !target.talk ? tgt.mouthOpen : null,
      speech.phraseActivity,
      speech.browAccent,
      speech.headAccent,
      behaviorMotion.coSpeechQuality,
      behaviorMotion.coSpeechGesture,
    )
    const groove = this.singingGroove.sample(
      t,
      singing,
      input.musicSignal,
      behaviorMotion.musicQuality,
      behaviorMotion.musicMode,
    )
    // A sticker face only takes the mouth once the character has stopped talking
    this.sillyMouthShare +=
      ((vocalizing ? 0 : 1) - this.sillyMouthShare) * (1 - Math.exp(-7 * dt))
    const sticker = Math.max(
      stylizedTargets.anger,
      stylizedTargets.speechless,
      stylizedTargets.maniac,
      stylizedTargets.silly,
      stylizedTargets.lovestruck,
    )
    const occupancy = this.occupancy.sample(dt, {
      speaking,
      singing,
      thinking: target.thinking,
      pointerDriven,
      automation: target.rand,
      sticker,
    })
    const randomAction = this.randomAction.sample(
      t,
      target.rand,
      this.performanceExpression.getActiveLevel() >= DIRECTED_BODY_BLOCK_LEVEL,
    )
    const ambient = this.ambientMotion.sample(t, target.rand)
    const thinking = this.thinkingMotion.sample(t, target.thinking && !speaking, tgt)
    const breath = idleBreathOffset(t, this.breathPose)
    if (this.standing) breath.body += standingWeightShift(t)
    const gate = this.poseGate.sample(
      dt,
      applyBehaviorMotionGate(
        resolvePoseGate(input.policy, occupancy, {
          performance: performanceMotionScale,
          stylized: stylized.ambientScale,
          randomAmbient: randomAction.ambientScale,
          touch: this.performanceExpression.getTouchShare(),
        }),
        behaviorMotion,
      ),
    )
    this.stylizedHeadShare = gate.stylized.headBody
    applyAnime25DComposedPose(
      tgt,
      gate,
      {
        ambient,
        randomAction,
        groove,
        thinking,
        breath,
        performance: semanticExpression,
        stylized,
        coSpeech: speechExpression,
      },
      this.composedPose,
    )
    applyAnime25DStylizedExpression(
      tgt,
      semanticExpression,
      stylized,
      this.sillyMouthShare,
      gate.performance.expression,
      gate.stylized.expression,
    )
    applyAnime25DCryMouth(tgt, this.current.eyeCry, t, dt, this.cryMouth)
    const gatedSpeechExpression = {
      brow: speechExpression.brow * gate.coSpeech.expression,
      eyeOpen: speechExpression.eyeOpen * gate.coSpeech.expression,
      angleY: speechExpression.angleY * gate.coSpeech.headBody,
    }
    gatedSpeechExpression.eyeOpen += expressiveEyeOpenOffset(
      gatedSpeechExpression,
    )
    applyAnime25DSpeechExtras(tgt, speech, gatedSpeechExpression)
    applyAnime25DSillyMouthOwnership(
      tgt,
      smoothAnime25DUnit(stylizedTargets.silly) * this.sillyMouthShare,
    )
    this.directedPose.setTouchShare(this.performanceExpression.getTouchShare())
    this.directedPose.write(tgt, vocalizing)
    projectAnime25DMotionEnvelope(
      tgt,
      input.motionEnvelopeProfile,
      this.motionEnvelopeResult,
    )
    this.closedEyes.step(tgt, dt)
    this.responseScale = resolvePoseResponseScale([
      {
        weight:
          this.performanceExpression.getActiveLevel() *
          gate.performance.headBody,
        quality: this.performanceExpression.getActiveQuality(),
      },
      {
        weight: behaviorMotion.coSpeech * gate.coSpeech.headBody,
        quality: behaviorMotion.coSpeechQuality,
      },
      {
        weight: behaviorMotion.music * gate.groove.headBody,
        quality: behaviorMotion.musicQuality,
      },
    ])
    // The smoothing works on the eyes as they would be without the blink;
    // the blink is laid over what it gives, at its own quick pace.
    this.current.eyeOpenL = this.unblinkedEyeOpenL
    this.current.eyeOpenR = this.unblinkedEyeOpenR
    stepAnime25DDriverResponse(
      this.current,
      target,
      tgt,
      this.poseResponse,
      dt,
      this.responseScale,
    )
    for (const control of INDEPENDENT_BODY_CONTROLS) {
      this.directedAuthorityTarget[BODY_CONTROL_DRIVERS[control]] = this.directedPose.weight(control)
    }
    this.directedAuthorityResponse.step(
      this.directedAuthority, this.directedAuthorityTarget, dt, this.responseScale,
    )
    this.unblinkedEyeOpenL = this.current.eyeOpenL
    this.unblinkedEyeOpenR = this.current.eyeOpenR
    const blinkSuppressed = stylizedTargets.maniac > 0.03 || stylizedTargets.silly > 0.03
    if (
      this.gazeBlink.step(
        this.current.angleX,
        this.current.angleY,
        this.current.eyeX,
        this.current.eyeY,
        dt,
        this.blinkState.activeSeconds >= 0,
        Math.random,
      ) &&
      target.blink &&
      !blinkSuppressed &&
      this.blinkState.activeSeconds < 0
    ) {
      this.blinkNow(t)
    }
    stepAnime25DBlink(
      this.current,
      this.blinkState,
      t,
      dt,
      target.blink,
      blinkSuppressed,
    )
    this.irisRebound.step(
      dt,
      Math.max(this.current.eyeOpenL, this.current.eyeOpenR),
      this.current.eyeWide,
      this.current.eyeX,
      this.current.eyeY,
      Math.max(
        stylizedTargets.maniac,
        stylizedTargets.silly,
        stylizedTargets.lovestruck,
        this.current.maniac,
        this.current.silly,
        this.current.lovestruck,
        tgt.eyeCry,
        tgt.eyeDizzy,
        tgt.eyeSqueeze,
        this.current.eyeCry,
        this.current.eyeDizzy,
        this.current.eyeSqueeze,
      ) > 0.03,
    )
  }
}
