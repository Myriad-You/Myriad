import type {
  ChestDeformationRegion,
  ChestDynamicsTuning,
  ChestMotionGeometry,
  ChestSpatialField,
} from './chestPhysics'
import type { CollarMotionPose } from './collarRuntime'
import type { Anime25DDriver } from './driver'
import type { Anime25DHairSpringFrame } from './hairPhysics'
import type { HeadSilhouette, HeadTurn } from './headTurn'
import type { JellyElement } from './jellyVolume'
import type { LayerJelly } from './layerFrameDeformation'
import type { Anime25DGpuLayer } from './layerGpuBinding'
import type { Anime25DMotionComposer } from './motionComposer'
import type { Anime25DMotionEnvelopeProfile } from './motionEnvelope'
import type { Anime25DRenderFrame } from './renderer'
import type { Anime25DSecondaryDeformationFrame } from './secondaryDeformation'
import type { Anime25DShellRotation } from './shellDeformation'
import type {
  Anime25DTorsoShellRotation,
  Anime25DTorsoYawState,
} from './torsoDeformation'
import type { Anime25DPlayback, Anime25DShellProfile } from './types'
import { ArmChoreography, resolveDirectedArmIntent } from './armChoreography'
import { ArmDrape, ArmPendulum, ArmSegment, FOREARM, HAND, REACH } from './armPendulum'
import { applyBodyLift, BodyLiftResponse } from './bodyLift'
import {
  chestBodyExcitationY,
  chestBreathResidual,
  chestBreathTargetY,
  chestFollowMix,
  chestMotionTarget,
  chestResponseMix,
  createChestSpringState,
  resolveChestDeformationRegion,
  resolveChestDynamics,
  resolveChestSpatialField,
  stepChestSpring,
  topwearMotionAtChest,
} from './chestPhysics'
import { stepAnime25DHairLayerSprings } from './hairPhysics'
import { writeHairRootMotion } from './hairRootMotion'
import { createHeadTurn, headTurnRadians, updateHeadTurn } from './headTurn'
import {
  createJawMotionState,
  jawMotionTarget,
  jawTravelPixels,
  stepJawMotion,
} from './jawMotion'
import {
  chestVolumeStretch,
  HEAD_JELLY,
  JellyVolume,
  SLEEVE_JELLY,
} from './jellyVolume'
import { writeAnime25DLayerGlobalTransform } from './layerTransform'
import { writePoseCorrectionWeights } from './poseCorrections'
import { BODY_ROLL_RADIANS, HEAD_ROLL_RADIANS } from './poseScale'
import { deformAnime25DSecondaryPoint, shellTurnBlend } from './secondaryDeformation'
import { writeAnime25DShellRotation } from './shellDeformation'
import { Anime25DStanding } from './standing'
import {
  anime25DTorsoShellOffsetX,
  anime25DTorsoYawFollow,
  resolveAnime25DTorsoChestShape,
  stepAnime25DTorsoShellRotation,
} from './torsoDeformation'

/** Share of an arm with a bare forearm that is sleeve: only that much of it is soft. */
const SLEEVE_SOFT_REACH = 0.55

interface JellyPart extends LayerJelly {
  volume: JellyVolume
  /** The shoulder the sleeve hangs from. */
  side: 'L' | 'R'
}

/**
 * Her body between the composed driver and the drawn layers: head, torso and
 * shell pose, breath, the chest spring, the jaw, hanging arms, soft volumes
 * and hair springs. It writes the frames every layer's deformation reads;
 * the player owns the GPU, the mouth and the layers themselves.
 */
export class Anime25DBodyFrames {
  private shellProfile!: Anime25DShellProfile
  private anchors!: Anime25DPlayback['anchors']
  private chestProfile!: Anime25DPlayback['chestProfile']
  /** The frame the secondary (body, shell, hair) deformation of every layer reads. */
  secondaryDeformationFrame!: Anime25DSecondaryDeformationFrame
  collarMotion!: CollarMotionPose
  /** How deep the neck sits, for the collar's aperture mesh. */
  neckDepth = 0

  private readonly deformationPoint = { x: 0, y: 0 }
  private headTurn: HeadTurn = createHeadTurn(null)
  private readonly shellRotation: Anime25DShellRotation = {
    active: false,
    yawCosine: 1,
    yawSine: 0,
    pitchCosine: 1,
    pitchSine: 0,
  }

  private readonly torsoYaw: Anime25DTorsoYawState = { value: 0, velocity: 0 }
  private bodyLiftResponse = new BodyLiftResponse()
  private bodyPitchResponse = new BodyLiftResponse()
  private readonly bodyLiftField = { centerX: 0, upperY: 0, lowerY: 1, amount: 0, pitch: 0, depth: 0, shoulderY: 0, groundY: 0, stanceShift: 0 }

  /** Each sleeve hangs from its shoulder; its swing is simulated, not authored. */
  private readonly armPendulums = { L: new ArmPendulum(1), R: new ArmPendulum(-1) } as const

  /** Splits the shared arm intent into what each arm does. */
  private readonly armChoreography = new ArmChoreography()
  private readonly armElbows = { L: false, R: false }
  private readonly armDrapes = { L: new ArmDrape(), R: new ArmDrape() } as const
  /** A forearm below a found elbow swings on after its upper arm. */
  private readonly forearms = { L: new ArmSegment(FOREARM), R: new ArmSegment(FOREARM) } as const
  /** A hand below a found wrist swings on after its forearm. */
  private readonly hands = { L: new ArmSegment(HAND), R: new ArmSegment(HAND) } as const
  /** How far each forearm has come forward. */
  private readonly reaches = { L: new ArmSegment(REACH), R: new ArmSegment(REACH) } as const

  private readonly armJoint = { x: 0, y: 0, reach: 0 }
  /** Each shoulder as the arm step last found it, for what else hangs there. */
  private readonly shoulders = { L: { x: 0, y: 0, found: false }, R: { x: 0, y: 0, found: false } }

  /** The head is one soft volume: it squashes and stretches about the chin. */
  private readonly headJelly = new JellyVolume(HEAD_JELLY)
  private headJellyElement: JellyElement | null = null
  /** Each hanging sleeve wobbles in its own volume as well. */
  private jellyParts = new Map<Anime25DGpuLayer, JellyPart>()
  private readonly jellyAnchor = { x: 0, y: 0 }
  private readonly headWorldTransform = new Float32Array(9)

  private readonly torsoShellRotation: Anime25DTorsoShellRotation = {
    active: false,
    yawCosine: 1,
    yawSine: 0,
  }

  private shellActivation = 0
  private hairSpringFrame!: Anime25DHairSpringFrame

  private readonly chest = createChestSpringState()
  private readonly chestTarget = { x: 0, y: 0 }
  private readonly chestSpringTarget = { x: 0, y: 0 }
  private readonly chestParentTarget = { x: 0, y: 0 }
  private chestDynamics!: ChestDynamicsTuning
  private chestField!: ChestSpatialField
  private chestGeometry!: ChestMotionGeometry
  private chestRegion!: ChestDeformationRegion
  private readonly jaw = createJawMotionState()
  private jawTravel = 0
  /** A full figure's stance on its ground; a bust has none. */
  private standing: Anime25DStanding | null = null

  constructor(
    /** The composed driver; the body only reads it. */
    private readonly current: Anime25DDriver,
    /** The render frame, whose body pivot and roll the body writes. */
    private readonly renderFrame: Anime25DRenderFrame,
    private readonly motion: Anime25DMotionComposer,
  ) {}

  /** Measure a new package: anchors, chest, jaw travel and the frames built on them. */
  bind(
    playback: Anime25DPlayback,
    motionEnvelopeProfile: Anime25DMotionEnvelopeProfile,
  ): void {
    this.shellProfile = playback.shellProfile
    this.anchors = playback.anchors
    this.chestProfile = playback.chestProfile
    this.neckDepth =
      playback.layers.find((layer) => layer.role === 'neck')?.depth ?? 0.95
    this.jawTravel = jawTravelPixels(playback)
    this.chestField = resolveChestSpatialField(playback.chestProfile)
    this.chestDynamics = resolveChestDynamics(
      playback.chestProfile,
      this.chestField,
    )
    const anchors = playback.anchors
    this.chestRegion = resolveChestDeformationRegion(playback.chestProfile)
    this.chestGeometry = {
      faceScale: anchors.faceScale,
      faceCenterY: anchors.face.cy,
      neckX: anchors.neckPivot.x,
      neckY: anchors.neckPivot.y,
      centerX: this.chestRegion.centerX,
      centerY: this.chestRegion.centerY,
      depth:
        playback.layers.find((layer) => layer.role === 'topwear')?.depth ?? 0.9,
    }
    const neckFollowTop = Math.min(
      anchors.neckBottom - 1,
      Math.max(anchors.neckTop, anchors.face.y1 + anchors.faceScale * 5),
    )
    this.secondaryDeformationFrame = {
      bodyRotationCosine: 1,
      bodyRotationSine: 0,
      expression: this.current,
      faceScale: anchors.faceScale,
      headAngleY: 0,
      headRotationCosine: 1,
      headRotationSine: 0,
      headRoll: 0,
      hairDrapeLength: (anchors.face.y1 - anchors.face.y0) * 1.5,
      bodyPivotY: anchors.bodyPivot.y,
      bodyBendHeight: Math.max(1, anchors.bodyPivot.y - anchors.neckBottom),
      neckPivotX: anchors.neckPivot.x,
      neckPivotY: anchors.neckPivot.y,
      neckBottom: anchors.neckBottom,
      neckFollowTop,
      neckFollowSpan: Math.max(1, anchors.neckBottom - neckFollowTop),
      faceCenterY: anchors.face.cy,
      bodyBreathOffset: 0,
      headBreathOffset: 0,
      torsoProfile: this.shellProfile.torso,
      torsoChestShape: resolveAnime25DTorsoChestShape(
        playback.chestProfile,
        this.chestField,
      ),
      torsoShellRotation: this.torsoShellRotation,
      torsoShellBlend: 0,
      torsoNeckOffsetX: 0,
      specialHeadOffset: 0,
      highCollar: motionEnvelopeProfile.highCollar,
      breath: 0,
      armAngleL: 0,
      armAngleR: 0,
      armDrapeL: 0,
      armDrapeR: 0,
      forearmL: 0,
      forearmR: 0,
      handL: 0,
      handR: 0,
      reachL: 0,
      reachR: 0,
      chestCenterX: this.chestRegion.centerX,
      chestRegionCenterY: this.chestRegion.centerY,
      chestMotionCenterY: this.chestRegion.centerY,
      chestRadiusY: this.chestRegion.radiusY,
      inverseChestRadiusX: 1 / this.chestRegion.radiusX,
      inverseChestRadiusY: 1 / this.chestRegion.radiusY,
      chestOffsetX: 0,
      chestOffsetY: 0,
      chestField: this.chestField,
      chestVolumeScale: 1,
      shellProfile: this.shellProfile,
      shellBlend: 0,
      shellActivation: 0,
      shellRotation: this.shellRotation,
      standing: null,
    }
    this.standing = Anime25DStanding.of(anchors)
    this.collarMotion = {
      neckPivotX: anchors.neckPivot.x,
      neckPivotY: anchors.neckPivot.y,
      neckFollowTop,
      neckFollowSpan: Math.max(1, anchors.neckBottom - neckFollowTop),
      faceCenterY: anchors.face.cy,
      faceScale: anchors.faceScale,
      angleX: 0,
      angleY: 0,
      headRotationCosine: 1,
      headRotationSine: 0,
      bodyBreathOffset: 0,
      headBreathOffset: 0,
      torsoProfile: this.shellProfile.torso,
      torsoShellBlend: 0,
      torsoNeckOffsetX: 0,
      torsoShellRotation: this.torsoShellRotation,
      headTurn: null,
      keyedHead: false,
    }
    this.hairSpringFrame = {
      enabled: true,
      idle: true,
      faceScale: anchors.faceScale,
      time: 0,
      frontSoft: 0,
      rearSoft: 0,
    }
  }

  /** Bind the new package's layers: the head turn's silhouette and the soft volumes. */
  bindLayers(layers: readonly Anime25DGpuLayer[], headSilhouette: HeadSilhouette | null, keyedHead = false): void {
    this.headTurn = createHeadTurn(headSilhouette, headTurnRadians(keyedHead))
    this.secondaryDeformationFrame.headTurn = this.headTurn
    this.secondaryDeformationFrame.keyedHead = keyedHead
    this.collarMotion.headTurn = this.headTurn
    this.collarMotion.keyedHead = keyedHead
    this.bindJelly(layers)
  }

  /** How far the jaw has dropped, in pixels, and how open it is. */
  /** The pose corrections are weighed against this frame. */
  poseCorrectionDriver(): Readonly<Anime25DDriver> {
    return this.current
  }

  /** The share of a pose correction's push the head shows this frame. */
  poseCorrectionGain(): number {
    const frame = this.secondaryDeformationFrame
    return frame.shellProfile.enabled ? shellTurnBlend(frame) : 0
  }

  jawDrop(): number {
    return this.jaw.value * this.jawTravel
  }

  jawOpen(): number {
    return Math.max(0, this.jaw.value)
  }

  /** The head's soft volume while physics runs, and how far it is squashed. */
  headJellyFor(physics: boolean): JellyElement | null {
    return physics ? this.headJellyElement : null
  }

  headStretch(): number {
    return this.headJelly.stretch
  }

  /** A hanging sleeve's soft volume. */
  jellyFor(layer: Anime25DGpuLayer): LayerJelly | undefined {
    return this.jellyParts.get(layer)
  }

  /** Posture after the driver moved: torso yaw, the shell coming in, lift and pitch. */
  stepPosture(dt: number, time: number): void {
    this.shellActivation = Math.min(1, this.shellActivation + dt * 8)
    stepAnime25DTorsoShellRotation(
      this.torsoYaw,
      this.current.angleX,
      this.current.body,
      dt,
      this.torsoShellRotation,
      anime25DTorsoYawFollow(this.shellProfile.torso, this.current.bodyYaw),
      this.current.torsoTurn,
      this.motion.directedBodyWeight('torsoTurn'),
    )
    // Explicit posture can be directed independently. Existing performances also
    // recruit the upper body, without adding an unrelated periodic oscillator.
    this.bodyLiftResponse.step(Math.max(-1, Math.min(1,
      this.current.angleY * 0.45 + this.current.armY * 0.2 +
      (this.current.idle || this.current.talk || this.current.singing ? chestBreathResidual(time) * 0.7 : 0),
    )), dt)
    this.bodyPitchResponse.step(Math.max(-1, Math.min(1,
      this.current.angleY * 0.35,
    )), dt)
  }

  /** One physics substep: jaw, chest, hair roots and springs, arms, soft volumes. */
  updateSprings(dt: number, layers: readonly Anime25DGpuLayer[], time: number): void {
    const anchors = this.anchors
    const faceScale = anchors.faceScale
    const e = this.current
    stepJawMotion(this.jaw, jawMotionTarget(e, this.motion.jawEmphasis), dt)
    const chestProfile = this.chestProfile
    if (chestProfile.enabled) {
      const chestTarget = chestMotionTarget(e, faceScale, this.chestTarget)
      chestTarget.y += chestBreathTargetY(
        chestBreathResidual(time),
        faceScale,
        this.chestDynamics.breathMotionScale,
      )
      this.chestSpringTarget.x = chestTarget.x
      this.chestSpringTarget.y =
        chestTarget.y +
        chestBodyExcitationY(
          e.body,
          faceScale,
          this.chestDynamics.bodyExcitationScale,
        )
      topwearMotionAtChest(e, this.chestGeometry, this.chestParentTarget)
      stepChestSpring(
        this.chest,
        this.chestSpringTarget.x,
        this.chestSpringTarget.y,
        dt,
        this.chestDynamics.frequencyScale,
        this.chestDynamics.dampingScale,
      )
    }
    const hairSpringFrame = this.hairSpringFrame
    hairSpringFrame.enabled = e.phys
    hairSpringFrame.idle = e.idle
    hairSpringFrame.frontSoft = e.fhSoft
    hairSpringFrame.rearSoft = e.soft
    if (e.phys) {
      this.prepareHeadFrame(time)
      for (const layer of layers) {
        const roots = layer.hairRoots
        if (!roots) continue
        if (roots.deformation.poseCorrections) writePoseCorrectionWeights(roots.deformation.poseCorrections, e)
        writeHairRootMotion(roots, this.secondaryDeformationFrame,
          anchors.bodyPivot.x, anchors.bodyPivot.y,
          this.renderFrame.bodyRotationCosine, this.renderFrame.bodyRotationSine, this.bodyLiftField)
      }
    }
    hairSpringFrame.time = time
    stepAnime25DHairLayerSprings(layers, hairSpringFrame, dt)
    this.stepArms(dt, layers, time)
    this.stepJelly(dt)
    if (this.standing) {
      this.secondaryDeformationFrame.standing = this.standing.step(dt, this.current.body, this.current.phys)
    }
  }

  /** Shared primary pose for physics substeps and the final visible mesh. */
  prepareHeadFrame(time: number): void {
    const e = this.current
    const frame = this.secondaryDeformationFrame
    const anchors = this.anchors
    const breath = 0.5 + chestBreathResidual(time)
    const breathHead = 0.5 + 0.5 * Math.sin((time * Math.PI * 2) / 3.4 - 0.6)
    const faceHeight = anchors.face.y1 - anchors.face.y0
    const span = Math.max(1, anchors.bodyPivot.y - anchors.neckBottom)
    Object.assign(this.bodyLiftField, {
      centerX: anchors.neckPivot.x,
      upperY: anchors.neckBottom + Math.min(faceHeight * 0.65, span * 0.5),
      lowerY: anchors.bodyPivot.y,
      // Directed pose is already continuous in the composer: don't filter it
      // a second time here. Only the automatic body recruitment has a spring.
      amount: Math.max(-1, Math.min(1, e.bodyLift + this.bodyLiftResponse.value * (1 - this.motion.directedBodyWeight('torsoRise')))) * Math.min(faceHeight * 0.05, span * 0.035),
      pitch: Math.max(-1, Math.min(1, e.bodyPitch + this.bodyPitchResponse.value * (1 - this.motion.directedBodyWeight('torsoPitch')))) * 0.18,
      depth: this.shellProfile.torso.enabled ? Math.min(this.shellProfile.torso.radiusZ, span * 0.45) : 0,
      shoulderY: anchors.neckBottom,
      groundY: this.standing?.groundY ?? anchors.bodyPivot.y,
      stanceShift: this.standing?.shift(e.body) ?? 0,
    })
    this.renderFrame.bodyLift = this.bodyLiftField
    frame.headAngleY = e.angleY
    frame.headRoll = e.angleZ * HEAD_ROLL_RADIANS
    frame.headRotationCosine = Math.cos(frame.headRoll)
    frame.headRotationSine = Math.sin(frame.headRoll)
    frame.bodyBreathOffset = breath * 2
    frame.headBreathOffset = breathHead * 1.6
    frame.specialHeadOffset = this.motion.stylizedMotion
      ? (this.motion.stylizedMotion.maniacHeadPulse * 80 +
          this.motion.stylizedMotion.sillyHeadPulse * 8 +
          this.motion.stylizedMotion.lovestruckHeadPulse * 5) * this.motion.stylizedHeadShare * anchors.faceScale
      : 0
    frame.breath = breath
    frame.shellBlend = this.shellProfile.blend * this.shellActivation
    frame.shellActivation = this.shellActivation
    frame.torsoShellBlend = this.shellProfile.enabled && this.shellProfile.torso.enabled
      ? frame.shellBlend * this.shellProfile.torso.blend : 0
    frame.torsoNeckOffsetX = anime25DTorsoShellOffsetX(
      anchors.neckPivot.x, this.shellProfile.torso, this.torsoShellRotation, frame.torsoShellBlend,
    )
    writeAnime25DShellRotation(e.angleX, e.angleY, this.shellRotation)
    updateHeadTurn(this.headTurn, e.angleX, e.angleY)
    this.renderFrame.bodyPivotX = anchors.bodyPivot.x
    this.renderFrame.bodyPivotY = anchors.bodyPivot.y
    this.renderFrame.bodyBendHeight = frame.bodyBendHeight ?? 0
    this.renderFrame.bodyRotationCosine = Math.cos(e.body * BODY_ROLL_RADIANS)
    this.renderFrame.bodyRotationSine = Math.sin(e.body * BODY_ROLL_RADIANS)
    frame.bodyRotationCosine = this.renderFrame.bodyRotationCosine
    frame.bodyRotationSine = this.renderFrame.bodyRotationSine
  }

  /** The chest's place and swing in this frame, for the torso deformation. */
  prepareChestFrame(time: number): void {
    const e = this.current
    const fs = this.anchors.faceScale
    const breathResidual = chestBreathResidual(time)
    const chestCy = this.chestRegion.centerY
    const chestRx = this.chestRegion.radiusX
    const chestRy = this.chestRegion.radiusY
    const chestMotionMix = chestResponseMix(
      e.bust,
      this.chestDynamics.responseScale,
    )
    const chestFollow = chestFollowMix(e.bust, this.chestDynamics.followScale)
    const chestCenterY = chestCy + (e.bustY - 1) * 70 * fs
    const chestOffsetX =
      (this.chestTarget.x - this.chestParentTarget.x) * chestFollow +
      this.chest.offsetX * chestMotionMix * this.chestDynamics.inertiaGain
    const chestOffsetY =
      (this.chestTarget.y - this.chestParentTarget.y) * chestFollow +
      this.chest.offsetY * chestMotionMix * this.chestDynamics.inertiaGain
    const inverseChestRx = 1 / chestRx
    const inverseChestRy = 1 / chestRy
    const secondaryDeformationFrame = this.secondaryDeformationFrame
    secondaryDeformationFrame.chestMotionCenterY = chestCenterY
    if (secondaryDeformationFrame.torsoChestShape) {
      secondaryDeformationFrame.torsoChestShape.centerY = chestCenterY
    }
    secondaryDeformationFrame.inverseChestRadiusX = inverseChestRx
    secondaryDeformationFrame.inverseChestRadiusY = inverseChestRy
    secondaryDeformationFrame.chestOffsetX = chestOffsetX
    secondaryDeformationFrame.chestOffsetY = chestOffsetY
    secondaryDeformationFrame.chestStretch = e.phys
      ? chestVolumeStretch(this.chest.offsetY * chestMotionMix * this.chestDynamics.inertiaGain, chestRy) *
        this.chestDynamics.volumeScale
      : 0
    secondaryDeformationFrame.chestVolumeScale =
      1 + breathResidual * this.chestDynamics.breathVolumeScale
  }

  /** The collar follows the head and torso the same way their layers do. */
  prepareCollarMotion(): CollarMotionPose {
    const e = this.current
    const secondaryDeformationFrame = this.secondaryDeformationFrame
    const collarMotion = this.collarMotion
    collarMotion.angleX = e.angleX
    collarMotion.angleY = e.angleY
    collarMotion.headRotationCosine = secondaryDeformationFrame.headRotationCosine
    collarMotion.headRotationSine = secondaryDeformationFrame.headRotationSine
    collarMotion.bodyBreathOffset = secondaryDeformationFrame.bodyBreathOffset
    collarMotion.headBreathOffset = secondaryDeformationFrame.headBreathOffset
    collarMotion.torsoShellBlend = secondaryDeformationFrame.torsoShellBlend
    collarMotion.torsoNeckOffsetX = secondaryDeformationFrame.torsoNeckOffsetX
    return collarMotion
  }

  /** Where a point drawn on the head is now, head carry and body lean included. */
  headPointNow(x: number, y: number): { x: number; y: number } {
    this.writeHeadWorldTransform(this.headWorldTransform)
    this.headWorldPoint(x, y, this.jellyAnchor)
    return this.jellyAnchor
  }

  private bindJelly(layers: readonly Anime25DGpuLayer[]): void {
    this.bodyLiftResponse = new BodyLiftResponse()
    this.bodyPitchResponse = new BodyLiftResponse()
    const anchors = this.anchors
    const face = anchors.face
    const faceHeight = face.y1 - face.y0
    this.jellyParts = new Map()
    this.headJellyElement = faceHeight > 0
      ? { anchorX: face.cx, anchorY: face.y1, axisX: 0, axisY: -1, length: faceHeight * 1.5, cutY: null }
      : null
    if (!this.headJellyElement) return
    for (const layer of layers) {
      const binding = layer.secondaryDeformation
      if (binding.handwear && binding.arm && binding.arm.scale === 1 && binding.handwearSide) {
        // A hanging sleeve's cloth hangs from the shoulder joint.
        const arm = binding.arm
        this.jellyParts.set(layer, {
          element: {
            anchorX: arm.pivotX, anchorY: arm.pivotY, axisX: 0, axisY: 1, length: arm.length, cutY: arm.cutY,
            // Cloth all the way down is soft all the way; a bare forearm and hand are not.
            softReach: arm.drape ? 1 : SLEEVE_SOFT_REACH,
          },
          volume: new JellyVolume(SLEEVE_JELLY),
          side: binding.handwearSide === 'L' ? 'L' : 'R',
        })
      }
    }
  }

  private stepJelly(dt: number): void {
    const element = this.headJellyElement
    if (!element) return
    const dynamic = this.current.phys
    const frame = this.secondaryDeformationFrame
    this.writeHeadWorldTransform(this.headWorldTransform)
    // Directions in the drawing turn with the head and body roll.
    const roll = (frame.headRoll ?? 0) + Math.atan2(frame.bodyRotationSine, frame.bodyRotationCosine)
    const downX = -Math.sin(roll)
    const downY = Math.cos(roll)
    this.headWorldPoint(element.anchorX, element.anchorY, this.jellyAnchor)
    this.headJelly.step(this.jellyAnchor.x, this.jellyAnchor.y, -downX, -downY, element.length, dt, dynamic)
    for (const part of this.jellyParts.values()) {
      const shoulder = this.shoulders[part.side]
      if (!shoulder.found) continue
      part.volume.step(shoulder.x, shoulder.y, downX, downY, part.element.length, dt, dynamic)
    }
  }

  /** The rigid head carry the shader gives a head-following layer. */
  private writeHeadWorldTransform(m: Float32Array): void {
    const f = this.secondaryDeformationFrame
    writeAnime25DLayerGlobalTransform({
      headFollow: 1, headRotationCosine: f.headRotationCosine,
      headRotationSine: f.headRotationSine, neckPivotX: f.neckPivotX,
      neckPivotY: f.neckPivotY, faceScale: f.faceScale,
      angleX: this.current.angleX, angleY: f.headAngleY, depthOffset: 0.3,
      faceCenterY: f.faceCenterY, specialOffsetY: f.specialHeadOffset,
      breathOffset: f.headBreathOffset,
    }, m)
    m[6] += f.torsoNeckOffsetX
  }

  /** A head point through `headWorldTransform`, then the body lean the shoulders take. */
  private headWorldPoint(x: number, y: number, out: { x: number; y: number }): void {
    const m = this.headWorldTransform
    const frame = this.renderFrame
    const px = m[0] * x + m[3] * y + m[6] - frame.bodyPivotX
    const py = m[1] * x + m[4] * y + m[7] - frame.bodyPivotY
    out.x = frame.bodyPivotX + px * frame.bodyRotationCosine - py * frame.bodyRotationSine
    out.y = frame.bodyPivotY + px * frame.bodyRotationSine + py * frame.bodyRotationCosine
    applyBodyLift(out, this.bodyLiftField)
  }

  private stepArms(dt: number, layers: readonly Anime25DGpuLayer[], time: number): void {
    const e = this.current
    const frame = this.secondaryDeformationFrame
    if (!e.phys) this.prepareHeadFrame(time)
    const bodyRoll = e.body * BODY_ROLL_RADIANS
    const sleeves = {
      L: layers.find((layer) => layer.secondaryDeformation.arm && layer.secondaryDeformation.handwearSide === 'L'),
      R: layers.find((layer) => layer.secondaryDeformation.arm && layer.secondaryDeformation.handwearSide === 'R'),
    }
    this.armElbows.L = Boolean(sleeves.L?.secondaryDeformation.arm?.elbow)
    this.armElbows.R = Boolean(sleeves.R?.secondaryDeformation.arm?.elbow)
    const choreography = this.armChoreography
    choreography.step(
      {
        open: e.armY,
        sway: e.armPos,
        headTurn: e.angleX,
        // The hips go against the lean: their weight is on the leg they move over.
        weight: this.standing ? -e.body : null,
        dynamic: e.phys,
      },
      this.armElbows,
      dt,
    )
    for (const side of ['L', 'R'] as const) {
      const joint = this.writeArmJoint(sleeves[side]) ? this.armJoint : null
      const shoulder = this.shoulders[side]
      shoulder.found = joint !== null
      shoulder.x = this.armJoint.x
      shoulder.y = this.armJoint.y
      const own = choreography[side]
      const raiseShare = this.motion.directedBodyWeight(side === 'L' ? 'leftArmRaise' : 'rightArmRaise')
      const swingShare = this.motion.directedBodyWeight(side === 'L' ? 'leftArmSwing' : 'rightArmSwing')
      const raised = side === 'L' ? e.armRaiseL : e.armRaiseR
      const swung = side === 'L' ? e.armSwingL : e.armSwingR
      const intent = resolveDirectedArmIntent(own, e.armPos, raised, swung, raiseShare, swingShare, this.armElbows[side])
      const input = { open: intent.open, sway: intent.sway, bodyRoll, dynamic: e.phys }
      const angle = this.armPendulums[side].step(input, joint, dt)
      const drape = this.armDrapes[side].step(angle, input.bodyRoll, input.dynamic, dt)
      // Away from the body is the arm's own outward turn.
      const outward = side === 'L' ? 1 : -1
      const forearm = this.forearms[side].step(angle, input.bodyRoll, input.dynamic, dt, outward * intent.bend)
      const hand = this.hands[side].step(angle + forearm, input.bodyRoll, input.dynamic, dt)
      const reach = Math.max(0, this.reaches[side].step(0, 0, input.dynamic, dt, intent.reach))
      if (side === 'L') {
        frame.armAngleL = angle
        frame.armDrapeL = drape
        frame.forearmL = forearm
        frame.handL = hand
        frame.reachL = reach
      } else {
        frame.armAngleR = angle
        frame.armDrapeR = drape
        frame.forearmR = forearm
        frame.handR = hand
        frame.reachR = reach
      }
    }
  }

  /** The shoulder joint after all primary motion, including the shader's body roll. */
  private writeArmJoint(layer: Anime25DGpuLayer | undefined): boolean {
    const binding = layer?.secondaryDeformation
    if (!layer || !binding?.arm || !binding.armMesh) return false
    const vertex = binding.armMesh.jointVertex
    const restX = layer.rest[vertex * 2]
    const restY = layer.rest[vertex * 2 + 1]
    const point = this.deformationPoint
    point.x = restX
    point.y = restY
    deformAnime25DSecondaryPoint(point, restX, restY, vertex, binding, this.secondaryDeformationFrame)
    const x = binding.arm.pivotX + point.x - restX
    const y = binding.arm.pivotY + point.y - restY
    const { bodyPivotX, bodyPivotY, bodyRotationCosine: c, bodyRotationSine: s } = this.renderFrame
    this.armJoint.x = bodyPivotX + (x - bodyPivotX) * c - (y - bodyPivotY) * s
    this.armJoint.y = bodyPivotY + (x - bodyPivotX) * s + (y - bodyPivotY) * c
    applyBodyLift(this.armJoint, this.bodyLiftField)
    this.armJoint.reach = binding.arm.reach
    return true
  }
}
