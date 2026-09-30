import type { PerformanceBaseline } from '../../../services/agent/types'
import type { PresentedTouchReaction } from '../interaction/touchReaction'
import type { MotionChannelPolicy } from '../motion/policy'
import type { MeropeRigManifest } from '../rig/types'
import type { MusicMotionSignal } from '../singing/musicSignal'
import type { SpeechProsodyPlan } from '../speech/prosody'
import type { Anime25DMotionUnit } from './behaviorMotion'
import type {
  ChestDeformationRegion,
  ChestDynamicsTuning,
  ChestMotionGeometry,
  ChestSpatialField,
} from './chestPhysics'
import type { CollarClipMesh, CollarMotionPose } from './collarRuntime'
import type { Anime25DDeformationChangeState } from './deformationDependencies'
import type { Anime25DDriver } from './driver'

import type { Anime25DExpressionDeformationFrame } from './expressionDeformation'
import type { Anime25DHairSpringFrame } from './hairPhysics'
import type { HeadTurn } from './headTurn'
import type { JellyElement } from './jellyVolume'
import type { LayerDeformationContext } from './layerFrameDeformation'
import type { Anime25DGpuLayer } from './layerGpuBinding'
import type { MotionComposerInput } from './motionComposer'
import type { Anime25DHandPose, Anime25DMotionEnvelopeProfile } from './motionEnvelope'
import type { Anime25DMouthDeformationFrame } from './mouthDeformation'
import type {
  Anime25DMouthMorphSources,
  Anime25DOpacityFrame,
  MouthMorphState,
} from './mouthRuntime'
import type { SpeechMouthMaterial } from './mouthTransition'
import type {
  Anime25DFrameWork,
  Anime25DPerformanceSnapshot,
} from './performanceTelemetry'
import type { PoseCorrection } from './poseCorrections'
import type { Anime25DRendererBindings, Anime25DRenderFrame } from './renderer'
import type { Anime25DSecondaryDeformationFrame } from './secondaryDeformation'
import type { Anime25DShellRotation } from './shellDeformation'
import type {
  Anime25DTorsoShellRotation,
  Anime25DTorsoYawState,
} from './torsoDeformation'
import type {
  TouchAtlas,
  TouchPaintLayer,
  VisibleTouchHit,
} from './touchVisibility'
import type { Anime25DFade, Anime25DPlayback, Anime25DShellProfile } from './types'
import { currentCopy } from '../../../i18n/localeCopy'
import { noteTurnTraceFrame } from '../events/turnTrace'
import { resolveAnime25DFaceFrame } from '../expressionShapes/faceFrame'
import { IDLE_MOTION_POLICY } from '../motion/policy'
import { resolveAnime25DLayerSemantics } from '../rig/anime25dLayerSemantics'
import { ArmDrape, ArmPendulum } from './armPendulum'
import { applyBodyLift, BodyLiftResponse } from './bodyLift'
import {
  buildChestWeightField,
  chestBodyExcitationY,
  chestBreathResidual,
  chestBreathTargetY,
  chestFollowMix,
  chestMotionTarget,
  chestProfileUsesGeometryWeights,
  chestResponseMix,
  createChestSpringState,
  resolveChestDeformationRegion,
  resolveChestDynamics,
  resolveChestSpatialField,
  stepChestSpring,
  topwearMotionAtChest,
} from './chestPhysics'
import { ClosedEyePresentation } from './closedEyePresentation'
import {
  deformCollarClipMesh,
  disposeCollarClipMesh,
  uploadCollarClipMesh,
} from './collarRuntime'
import { ContinuousMouthTexture, prepareContinuousMouth } from './continuousMouthTexture'
import {
  captureAnime25DDeformationChanges,
  createAnime25DDeformationChangeState,
  markAnime25DLayerGeometryUpdated,
  shouldUpdateAnime25DLayerGeometry,
} from './deformationDependencies'
import { IDENTITY_DRIVER, sanitizeDriverPatch } from './driver'

import { ThinkingExpressionOwnership } from './expressionPresets'
import {
  animationCatchupSeconds,
  animationElapsedSeconds,
  animationSubstepCount,
} from './frameClock'
import { stepAnime25DHairLayerSprings } from './hairPhysics'
import { writeHairRootMotion } from './hairRootMotion'
import { createHeadTurn, updateHeadTurn } from './headTurn'
import { Anime25DIrisRebound } from './irisRebound'
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
import {
  deformLayerVertices,
  settleDependentLayers,
  writeRigidLayerTransform,
} from './layerFrameDeformation'
import { compileAnime25DGpuLayers } from './layerGpuBinding'
import { writeAnime25DLayerGlobalTransform } from './layerTransform'
import { Anime25DMotionComposer } from './motionComposer'
import {
  deriveAnime25DMotionEnvelopeProfile,
} from './motionEnvelope'

import {
  applyMouthTransitionBridge,
  compileAnime25DMouthMorphSources,
  createAnime25DOpacityFrame,
  fadeOpacityFromFrame,
  resolveMouthMorph,
  shouldDeformLayer,
  writeAnime25DOpacityFrame,
} from './mouthRuntime'
import { MouthTransitionController } from './mouthTransition'
import {
  bearingDriverPatch,
} from './performanceExpression'
import { baselineDriverPatch, restEnergyDriverPatch } from './performanceMotion'
import {
  Anime25DPerformanceTelemetry,
  createAnime25DFrameWork,
} from './performanceTelemetry'

import { bindPoseCorrections, isPoseCorrections, writePoseCorrectionWeights } from './poseCorrections'

import { BODY_ROLL_RADIANS, HEAD_ROLL_RADIANS } from './poseScale'

import {
  createAnime25DRendererBindings,
  disposeAnime25DRendererBindings,
  drawAnime25DFrame,
} from './renderer'
import {
  resolveAnime25DRenderSurface,
  shouldApplyAnime25DResize,
} from './runtimePolicy'
import {
  deformAnime25DSecondaryPoint,
} from './secondaryDeformation'
import { writeAnime25DShellRotation } from './shellDeformation'
import { ThinkingSticker } from './thinkingSticker'
import {
  anime25DTorsoShellOffsetX,
  anime25DTorsoYawFollow,
  resolveAnime25DTorsoChestShape,
  stepAnime25DTorsoShellRotation,
} from './torsoDeformation'
import { touchPointInView } from './touchHitTest'
import { hitTestVisibleTouch, readTouchAtlas } from './touchVisibility'
import { compileProgram, createAtlasTexture, loadImage, readLayerPixels } from './webglRuntime'

export interface Anime25DDebugSnapshot {
  layerCount: number
  hairLayerCount: number
  strandCount: number
  eyeOpenLayers: number
  eyeCloseLayers: number
  eyeDizzyLayers: number
  eyeSqueezeLayers: number
  eyeCryLayers: number
  eyeSillyLayers: number
  lovestruckHeartLayers: number
  lovestruckFaceLayers: number
  lovestruckDroolLayers: number
  maniacEyeShadowLayers: number
  angerMarkLayers: number
  speechlessSweatLayers: number
  mouthOpenLayers: number
  mouthWideLayers: number
  mouthRoundLayers: number
  mouthNarrowLayers: number
  mouthCloseLayers: number
  mouthCryLayers: number
  mouthManiacLayers: number
  mouthSillyLayers: number
  canvas: { width: number; height: number }
  motionEnvelope: {
    highCollar: boolean
    armMotion: boolean
    pitchLimit: number
    torsoLimit: number
    armLimit: number
    transferredEnergy: number
  }
  performance: Anime25DPerformanceSnapshot
  current: Anime25DDriver
}

function releaseCompiledGpu(
  gl: WebGL2RenderingContext,
  layers: readonly Anime25DGpuLayer[],
  collarClip: CollarClipMesh | null,
  texture: WebGLTexture | null,
): void {
  for (const layer of layers) {
    if (layer.vertexBuffer) gl.deleteBuffer(layer.vertexBuffer)
    if (layer.uvBuffer) gl.deleteBuffer(layer.uvBuffer)
    if (layer.indexBuffer) gl.deleteBuffer(layer.indexBuffer)
    if (layer.vao) gl.deleteVertexArray(layer.vao)
  }
  if (collarClip) disposeCollarClipMesh(gl, collarClip)
  if (texture) gl.deleteTexture(texture)
}

/** Share of an arm with a bare forearm that is sleeve: only that much of it is soft. */
const SLEEVE_SOFT_REACH = 0.55

interface JellyPart {
  element: JellyElement
  volume: JellyVolume
  /** The shoulder the sleeve hangs from. */
  side: 'L' | 'R'
}

/** Snapshot keys that count the layers fading in for one expression. */
const DEBUG_FADE_COUNTS = {
  eyeOpenLayers: 'eyeOpen',
  eyeCloseLayers: 'eyeClose',
  eyeDizzyLayers: 'eyeDizzy',
  eyeSqueezeLayers: 'eyeSqueeze',
  eyeCryLayers: 'eyeCry',
  eyeSillyLayers: 'eyeSilly',
  lovestruckHeartLayers: 'lovestruckHeart',
  lovestruckFaceLayers: 'lovestruckFace',
  lovestruckDroolLayers: 'lovestruckDrool',
  maniacEyeShadowLayers: 'maniacEyeShadow',
  angerMarkLayers: 'angerMark',
  speechlessSweatLayers: 'speechlessSweat',
  mouthOpenLayers: 'mouthOpen',
  mouthWideLayers: 'mouthWide',
  mouthRoundLayers: 'mouthRound',
  mouthNarrowLayers: 'mouthNarrow',
  mouthCloseLayers: 'mouthClose',
  mouthCryLayers: 'mouthCry',
  mouthManiacLayers: 'mouthManiac',
  mouthSillyLayers: 'mouthSilly',
} as const satisfies Record<string, Anime25DFade>

export class Anime25DPlayer {
  private readonly thinkingSticker = new ThinkingSticker()
  private readonly gl: WebGL2RenderingContext
  private playback!: Anime25DPlayback
  private rigManifest: MeropeRigManifest | undefined
  private shellProfile!: Anime25DShellProfile
  private readonly program: WebGLProgram
  private readonly rendererBindings: Anime25DRendererBindings
  private readonly renderFrame: Anime25DRenderFrame = {
    viewWidth: 1,
    viewHeight: 1,
    bodyPivotX: 0,
    bodyPivotY: 0,
    bodyRotationCosine: 1,
    bodyRotationSine: 0,
    bodyBendHeight: 0,
    time: 0,
    eyeCry: 0,
  }

  private layers: Anime25DGpuLayer[] = []
  private atlasTexture: WebGLTexture | null = null
  private touchAtlas: TouchAtlas | null = null
  private touchLayers: TouchPaintLayer[] = []
  private readonly performanceTelemetry = new Anime25DPerformanceTelemetry()
  private readonly current: Anime25DDriver = { ...IDENTITY_DRIVER }
  private readonly target: Anime25DDriver = { ...IDENTITY_DRIVER }
  private readonly mouthMorph: MouthMorphState = {
    centerX: 0,
    centerY: 0,
    width: 1,
    height: 1,
    openMix: 0,
    wide: 0,
    round: 0,
    narrow: 0,
    openCenterY: 0,
    openHeight: 1,
  }

  private mouthMorphSources!: Anime25DMouthMorphSources
  private readonly opacityFrame: Anime25DOpacityFrame =
    createAnime25DOpacityFrame()

  private readonly deformationChangeState: Anime25DDeformationChangeState =
    createAnime25DDeformationChangeState()

  private readonly deformationPoint = { x: 0, y: 0 }
  private deformationFrame!: Anime25DMouthDeformationFrame &
    Anime25DExpressionDeformationFrame

  private secondaryDeformationFrame!: Anime25DSecondaryDeformationFrame

  private headTurn: HeadTurn = createHeadTurn(null)
  /** The speaking mouth drawn live, when the portrait's speaking mouths are the importer's. */
  private continuousMouth: ContinuousMouthTexture | null = null

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
  private readonly bodyLiftField = { centerX: 0, upperY: 0, lowerY: 1, amount: 0, pitch: 0, depth: 0, shoulderY: 0 }

  /** Each sleeve hangs from its shoulder; its swing is simulated, not authored. */
  private readonly armPendulums = { L: new ArmPendulum(1), R: new ArmPendulum(-1) } as const

  private readonly armDrapes = { L: new ArmDrape(), R: new ArmDrape() } as const

  private readonly armJoint = { x: 0, y: 0, reach: 0 }
  /** Each shoulder as the arm step last found it, for what else hangs there. */
  private readonly shoulders = { L: { x: 0, y: 0, found: false }, R: { x: 0, y: 0, found: false } }

  /** The head is one soft volume: it squashes and stretches about the chin. */
  private readonly headJelly = new JellyVolume(HEAD_JELLY)
  private headJellyElement: JellyElement | null = null
  /** Each hanging sleeve wobbles in its own volume as well. */
  private jellyParts = new Map<Anime25DGpuLayer, JellyPart>()
  private readonly jellyShift = { x: 0, y: 0 }
  private readonly jellyAnchor = { x: 0, y: 0 }
  private readonly headWorldTransform = new Float32Array(9)

  private readonly torsoShellRotation: Anime25DTorsoShellRotation = {
    active: false,
    yawCosine: 1,
    yawSine: 0,
  }

  private shellActivation = 0
  private collarMotion!: CollarMotionPose
  private hairSpringFrame!: Anime25DHairSpringFrame

  private mouthTransition!: MouthTransitionController
  private activeMouthMaterial: SpeechMouthMaterial = 'mouthClose'

  private time = 0
  private readonly irisRebound = new Anime25DIrisRebound()
  private readonly closedEyes = new ClosedEyePresentation()
  private readonly motion = new Anime25DMotionComposer(
    this.current,
    this.closedEyes,
    this.irisRebound,
  )

  /** The eyes' openness before the blink is laid over it. */
  private readonly thinkingExpression = new ThinkingExpressionOwnership()
  private musicSignal: MusicMotionSignal | null = null
  private presentedTouch: PresentedTouchReaction | null = null

  getPresentedTouch(): PresentedTouchReaction | null {
    return this.presentedTouch
  }

  private speechActive = false
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
  private motionEnvelopeProfile!: Anime25DMotionEnvelopeProfile
  private neckDepth = 0
  private collarClip: CollarClipMesh | null = null
  private readonly mouse = { x: 0, y: 0, inside: false }
  private policy: MotionChannelPolicy = { ...IDLE_MOTION_POLICY }
  private disposed = false
  private atlasAbort: AbortController | null = null
  /** Per-frame inputs every layer's deformation reads; refilled, never reallocated. */
  private readonly layerContext: LayerDeformationContext = {
    anchors: undefined!,
    current: this.current,
    time: 0,
    deformationFrame: undefined!,
    secondaryDeformationFrame: undefined!,
    irisRebound: this.irisRebound,
    headJellyElement: null,
    headStretch: 0,
    deformationPoint: this.deformationPoint,
    jellyShift: this.jellyShift,
  }

  private readonly motionInput: MotionComposerInput = {
    time: 0,
    target: this.target,
    policy: this.policy,
    mouse: this.mouse,
    speechActive: false,
    musicSignal: null,
    motionEnvelopeProfile: undefined!,
  }

  constructor(
    canvas: HTMLCanvasElement,
    playback: Anime25DPlayback,
    rigManifest?: MeropeRigManifest,
  ) {
    const gl = canvas.getContext('webgl2', {
      alpha: true,
      premultipliedAlpha: true,
      stencil: true,
      antialias: true,
      preserveDrawingBuffer: true,
      powerPreference: 'low-power',
    })
    if (!gl) throw new Error(currentCopy().merope.anime25dWebglFailed)
    this.gl = gl
    this.program = compileProgram(gl)
    try {
      this.rendererBindings = createAnime25DRendererBindings(gl, this.program)
      this.applyPackage(playback, rigManifest)
    } catch (error) {
      // A failed constructor has no owner that can call dispose().
      gl.deleteProgram(this.program)
      throw error
    }
  }

  /** The last outfit keeps drawing until the next atlas is bound. */
  async replaceLivePackage(
    playback: Anime25DPlayback,
    rigManifest: MeropeRigManifest | undefined,
    atlasUrl: string,
  ): Promise<void> {
    this.atlasAbort?.abort()
    const atlasAbort = new AbortController()
    this.atlasAbort = atlasAbort
    const image = await loadImage(atlasUrl, atlasAbort.signal)
    if (this.disposed || atlasAbort.signal.aborted) return
    const resolved: Anime25DPlayback = {
      ...playback,
      layers: playback.layers.map(resolveAnime25DLayerSemantics),
    }
    const chestWeightField = chestProfileUsesGeometryWeights(
      resolved.chestProfile,
    )
      ? buildChestWeightField(rigManifest)
      : null
    const compiled = compileAnime25DGpuLayers(
      this.gl,
      this.program,
      resolved,
      resolved.shellProfile,
      this.current,
      chestWeightField,
      image,
    )
    let nextTexture: WebGLTexture | null = null
    let touchAtlas: ReturnType<typeof readTouchAtlas>
    let linePixels: Uint8ClampedArray | undefined
    try {
      nextTexture = createAtlasTexture(this.gl, image, compiled.atlasPatches)
      touchAtlas = readTouchAtlas(image)
      const eyelash = resolved.layers.find((layer) => layer.role === 'eyelash')
      linePixels = eyelash ? readLayerPixels(image, eyelash)?.pixels : undefined
      if (this.disposed || atlasAbort.signal.aborted) {
        releaseCompiledGpu(this.gl, compiled.layers, compiled.collarClip, nextTexture)
        return
      }
      this.applyPackage(playback, rigManifest, {
        linked: compiled.armsLinked,
        touchingHead: compiled.handTouchesHead,
      })
    } catch (error) {
      // Keep the live outfit; the not-yet-owned replacement must be released.
      releaseCompiledGpu(
        this.gl,
        compiled.layers,
        compiled.collarClip,
        nextTexture,
      )
      throw error
    }
    releaseCompiledGpu(this.gl, this.layers, this.collarClip, this.atlasTexture)
    this.atlasTexture = nextTexture
    this.layers = compiled.layers
    this.collarClip = compiled.collarClip
    this.bindContinuousMouth(resolved, image)
    this.headTurn = createHeadTurn(compiled.headSilhouette ?? null)
    this.secondaryDeformationFrame.headTurn = this.headTurn
    this.bindJelly()
    this.touchAtlas = touchAtlas
    this.thinkingSticker.setLinePixels(linePixels)
    this.touchLayers = this.layers.map((layer) => {
      const mesh =
        layer.renderKind === 'neck' && this.collarClip ? this.collarClip : layer
      return {
        paint: layer,
        mesh: {
          positions: mesh.deformed,
          atlasUvs: mesh.atlasUvs,
          indices: mesh.indices,
          layerTransform: layer.layerTransform,
        },
      }
    })
  }

  async loadAtlas(url: string): Promise<void> {
    await this.replaceLivePackage(this.playback, this.rigManifest, url)
  }

  /** Workbench-only override. Never changes the immutable playback package. */
  previewPoseCorrections(corrections: readonly PoseCorrection[] | null): void {
    if (corrections !== null && !isPoseCorrections(corrections)) throw new Error('Invalid pose corrections')
    const profile = this.playback.shellProfile
    for (const layer of this.layers) {
      if (layer.attachment || layer.neckwearBridge || layer.shaderGlobalTransform) continue
      layer.secondaryDeformation.poseCorrections = bindPoseCorrections(
        corrections ?? profile.poseCorrections, layer.secondaryDeformation.shellMode, layer.rest, profile.head,
      )
    }
  }

  hitTestTouch(clientX: number, clientY: number): VisibleTouchHit | null {
    const canvas = this.gl.canvas
    if (
      this.disposed ||
      !this.touchAtlas ||
      !(canvas instanceof HTMLCanvasElement)
    ) {
      return null
}
    const point = touchPointInView(
      clientX,
      clientY,
      canvas.getBoundingClientRect(),
      {
        width: this.renderFrame.viewWidth,
        height: this.renderFrame.viewHeight,
      },
    )
    return point
      ? hitTestVisibleTouch(
          point.x,
          point.y,
          this.touchLayers,
          this.touchAtlas,
          this.renderFrame,
        )
      : null
  }

  private applyPackage(
    playback: Anime25DPlayback,
    rigManifest?: MeropeRigManifest,
    hands: Anime25DHandPose = {},
  ): void {
    playback = {
      ...playback,
      layers: playback.layers.map(resolveAnime25DLayerSemantics),
    }
    this.playback = playback
    this.closedEyes.bind(playback.layers)
    this.rigManifest = rigManifest
    this.shellProfile = playback.shellProfile
    this.motionEnvelopeProfile = deriveAnime25DMotionEnvelopeProfile(
      playback,
      rigManifest,
      hands,
    )
    this.motion.singingGroove.setArmMotion(
      this.motionEnvelopeProfile.armMotion && this.motionEnvelopeProfile.rigidArm.limit > 0,
    )
    this.neckDepth =
      playback.layers.find((layer) => layer.role === 'neck')?.depth ?? 0.95
    this.mouthTransition = new MouthTransitionController(playback.mouthProfile)
    this.mouthMorphSources = compileAnime25DMouthMorphSources(playback.layers)
    const faceFrame = resolveAnime25DFaceFrame(playback.anchors)
    this.deformationFrame = {
      mouth: playback.anchors.mouth,
      face: playback.anchors.face,
      faceAxes: { cos: faceFrame.cos, sin: faceFrame.sin },
      faceScale: playback.anchors.faceScale,
      morph: this.mouthMorph,
      mouthMorph: this.mouthMorph,
      expression: this.current,
      jawDrop: 0,
      jawOpen: 0,
      time: 0,
      stylizedMotion: null,
    }
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
      highCollar: this.motionEnvelopeProfile.highCollar,
      breath: 0,
      armAngleL: 0,
      armAngleR: 0,
      armDrapeL: 0,
      armDrapeR: 0,
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
    }
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

  setTarget(partial: Partial<Anime25DDriver>): void {
    const patch = sanitizeDriverPatch(partial)
    this.thinkingExpression.apply(this.target, patch, this.speechActive || (patch.talk ?? this.target.talk))
  }

  replaceTarget(driver: Anime25DDriver): void {
    this.thinkingExpression.clear()
    Object.assign(this.target, IDENTITY_DRIVER)
    // A full workbench replacement owns its authored state, including flags.
    this.thinkingExpression.apply(this.target, sanitizeDriverPatch(driver), false)
  }

  getTarget(): Anime25DDriver {
    return { ...this.target }
  }

  getCurrent(): Anime25DDriver {
    return { ...this.current }
  }

  blinkNow(): void {
    this.motion.blinkNow(this.time)
  }

  setMouse(x: number, y: number, inside: boolean): void {
    this.mouse.inside = inside
    if (!inside) return
    this.mouse.x = x
    this.mouse.y = y
  }

  setMotionPolicy(policy: MotionChannelPolicy): void {
    this.policy = policy
  }

  getMotionPolicy(): MotionChannelPolicy {
    return this.policy
  }

  setSpeechActive(active: boolean): void {
    this.speechActive = active
    if (active) this.thinkingExpression.release(this.target)
  }

  setSinging(active: boolean): void {
    this.target.singing = active
  }

  setSingingTrack(trackId: string | null): void {
    this.motion.singingGroove.setTrack(trackId)
  }

  setMusicSignal(drive: MusicMotionSignal | null): void {
    this.musicSignal = drive
  }

  setSpeechProsody(plan: SpeechProsodyPlan | null): void {
    this.motion.speechExpression.setProsody(plan, this.time)
  }

  enqueueSpeechText(text: string, locale?: string): void {
    this.motion.speechMotion.enqueueText(text, locale)
  }

  clearSpeechText(): void {
    this.motion.speechMotion.clear(this.time)
  }

  setBehaviorMotionUnits(
    units: readonly Anime25DMotionUnit[],
    nowMs: number,
  ): void {
    this.motion.behaviorMotion.replace(units, nowMs, this.time)
    this.motion.performanceExpression.playBehaviorUnits(units, this.time, nowMs)
  }

  clearBehaviorMotionUnits(): void {
    this.motion.behaviorMotion.clear(this.time)
    this.motion.performanceExpression.stopBehaviors(this.time)
  }

  setBearing(bearing: PerformanceBaseline | null): void {
    this.motion.performanceExpression.setBearingAttention(bearing?.attention ?? null)
    this.setTarget({
      ...(bearing ? baselineDriverPatch(bearing) : restEnergyDriverPatch()),
      ...bearingDriverPatch(bearing),
    })
  }

  debugSnapshot(): Anime25DDebugSnapshot {
    const layers = this.playback.layers
    const fadeCounts = Object.fromEntries(
      Object.entries(DEBUG_FADE_COUNTS).map(([key, fade]) => [
        key,
        layers.filter((layer) => layer.fade === fade).length,
      ]),
    ) as Record<keyof typeof DEBUG_FADE_COUNTS, number>
    return {
      layerCount: layers.length,
      hairLayerCount: layers.filter((layer) => layer.phys === 'hair').length,
      strandCount: layers.reduce((sum, layer) => sum + layer.strands.length, 0),
      ...fadeCounts,
      canvas: { ...this.playback.pixelCanvas },
      motionEnvelope: {
        highCollar: this.motionEnvelopeProfile.highCollar,
        armMotion: this.motionEnvelopeProfile.armMotion,
        pitchLimit: this.motionEnvelopeProfile.pitch.limit,
        torsoLimit: this.motionEnvelopeProfile.torso.limit,
        armLimit: this.motionEnvelopeProfile.rigidArm.limit,
        transferredEnergy: this.motion.motionEnvelopeResult.transferredEnergy,
      },
      performance: this.performanceTelemetry.observe(),
      current: this.getCurrent(),
    }
  }

  resize(cssWidth: number, cssHeight: number, devicePixelRatio: number): void {
    if (!shouldApplyAnime25DResize(cssWidth, cssHeight)) return
    const { width: pixelWidth, height: pixelHeight } = this.playback.pixelCanvas
    const surface = resolveAnime25DRenderSurface({
      sourceWidth: pixelWidth,
      sourceHeight: pixelHeight,
      cssWidth,
      cssHeight,
      devicePixelRatio,
    })
    const canvas = this.gl.canvas
    if (canvas instanceof HTMLCanvasElement) {
      if (canvas.width !== surface.bufferWidth)
        canvas.width = surface.bufferWidth
      if (canvas.height !== surface.bufferHeight)
        canvas.height = surface.bufferHeight
      const nextWidth = `${surface.displayWidth}px`
      const nextHeight = `${surface.displayHeight}px`
      if (canvas.style.width !== nextWidth) canvas.style.width = nextWidth
      if (canvas.style.height !== nextHeight) canvas.style.height = nextHeight
    }
    this.renderFrame.viewWidth = pixelWidth
    this.renderFrame.viewHeight = pixelHeight
    this.gl.viewport(0, 0, surface.bufferWidth, surface.bufferHeight)
  }

  tick(deltaSeconds: number): void {
    if (this.disposed || this.layers.length === 0) return
    const elapsed = animationElapsedSeconds(deltaSeconds)
    const catchup = animationCatchupSeconds(elapsed)
    const substeps = animationSubstepCount(catchup)
    const dt = catchup / substeps
    const dropped = elapsed > 0.05
    this.time += elapsed - catchup
    if (!this.performanceTelemetry.shouldSample()) {
      for (let step = 0; step < substeps; step += 1) {
        this.time += dt
        this.smoothDriver(dt)
        this.updateSprings(dt)
      }
      this.deform()
      this.uploadGeometry()
      this.draw()
      if (dropped) noteTurnTraceFrame({ dropped: true })
      return
    }
    const work = createAnime25DFrameWork()
    const frameStarted = performance.now()
    let driverMs = 0
    let springsMs = 0
    for (let step = 0; step < substeps; step += 1) {
      this.time += dt
      const driverStarted = performance.now()
      this.smoothDriver(dt)
      const driverFinished = performance.now()
      this.updateSprings(dt)
      const springsFinished = performance.now()
      driverMs += driverFinished - driverStarted
      springsMs += springsFinished - driverFinished
    }
    const springsFinished = performance.now()
    this.deform(work)
    this.uploadGeometry(work)
    const deformFinished = performance.now()
    this.draw(work)
    const drawFinished = performance.now()
    this.performanceTelemetry.record({
      ...work,
      frameCpuMs: drawFinished - frameStarted,
      driverMs,
      springsMs,
      deformMs: deformFinished - springsFinished,
      drawSubmitMs: drawFinished - deformFinished,
    })
    noteTurnTraceFrame({
      dropped,
      cpuMs: drawFinished - frameStarted,
    })
  }

  dispose(): void {
    if (this.disposed) return
    this.disposed = true
    this.atlasAbort?.abort()
    this.atlasAbort = null
    releaseCompiledGpu(this.gl, this.layers, this.collarClip, this.atlasTexture)
    this.continuousMouth?.dispose()
    this.continuousMouth = null
    this.layers = []
    this.collarClip = null
    this.atlasTexture = null
    this.touchAtlas = null
    this.touchLayers = []
    this.gl.deleteProgram(this.program)
    disposeAnime25DRendererBindings(this.gl, this.rendererBindings)
    this.thinkingSticker.dispose(this.gl)
    // A canvas still in the document must keep the context
    try {
      const surface = this.gl.canvas
      if (surface instanceof HTMLCanvasElement && surface.isConnected) return
      this.gl.getExtension('WEBGL_lose_context')?.loseContext()
    } catch {
      /* ignore */
    }
  }

  private smoothDriver(dt: number): void {
    this.thinkingSticker.update(this.time,
      this.speechActive || this.target.talk ? 0 : this.target.thinking ? 1 : this.motion.performanceExpression.getThinkingLevel())
    this.shellActivation = Math.min(1, this.shellActivation + dt * 8)
    const input = this.motionInput
    input.time = this.time
    input.policy = this.policy
    input.speechActive = this.speechActive
    input.musicSignal = this.musicSignal
    input.motionEnvelopeProfile = this.motionEnvelopeProfile
    this.motion.step(dt, input)
    stepAnime25DTorsoShellRotation(
      this.torsoYaw,
      this.current.angleX,
      this.current.body,
      dt,
      this.torsoShellRotation,
      anime25DTorsoYawFollow(this.shellProfile.torso, this.current.bodyYaw),
    )
    // Explicit posture can be directed independently. Existing performances also
    // recruit the upper body, without adding an unrelated periodic oscillator.
    this.bodyLiftResponse.step(Math.max(-1, Math.min(1,
      this.target.bodyLift + this.current.angleY * 0.45 + this.current.armY * 0.2 +
      (this.current.idle || this.current.talk || this.current.singing ? chestBreathResidual(this.time) * 0.7 : 0),
    )), dt)
    this.bodyPitchResponse.step(Math.max(-1, Math.min(1,
      this.target.bodyPitch + this.current.angleY * 0.35,
    )), dt)
  }

  private updateSprings(dt: number): void {
    const { anchors } = this.playback
    const faceScale = anchors.faceScale
    const e = this.current
    stepJawMotion(this.jaw, jawMotionTarget(e, this.motion.jawEmphasis), dt)
    const chestProfile = this.playback.chestProfile
    if (chestProfile.enabled) {
      const chestTarget = chestMotionTarget(e, faceScale, this.chestTarget)
      chestTarget.y += chestBreathTargetY(
        chestBreathResidual(this.time),
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
      this.prepareHeadDeformationFrame()
      for (const layer of this.layers) {
        const roots = layer.hairRoots
        if (!roots) continue
        if (roots.deformation.poseCorrections) writePoseCorrectionWeights(roots.deformation.poseCorrections, e)
        writeHairRootMotion(roots, this.secondaryDeformationFrame,
          anchors.bodyPivot.x, anchors.bodyPivot.y,
          this.renderFrame.bodyRotationCosine, this.renderFrame.bodyRotationSine, this.bodyLiftField)
      }
    }
    hairSpringFrame.time = this.time
    stepAnime25DHairLayerSprings(this.layers, hairSpringFrame, dt)
    this.stepArms(dt)
    this.stepJelly(dt)
  }

  private bindContinuousMouth(playback: Readonly<Anime25DPlayback>, atlas: HTMLImageElement): void {
    this.continuousMouth?.dispose()
    this.continuousMouth = null
    const setup = prepareContinuousMouth(
      playback.layers,
      playback.anchors,
      atlas.naturalWidth || atlas.width,
      atlas.naturalHeight || atlas.height,
      (layer) => readLayerPixels(atlas, layer),
    )
    const layer = setup && this.layers.find((candidate) => candidate.source === setup.layer)
    if (!setup || !layer) return
    this.continuousMouth = new ContinuousMouthTexture(this.gl, setup)
    layer.ownTexture = this.continuousMouth.own
  }

  private bindJelly(): void {
    this.bodyLiftResponse = new BodyLiftResponse()
    this.bodyPitchResponse = new BodyLiftResponse()
    const anchors = this.playback.anchors
    const face = anchors.face
    const faceHeight = face.y1 - face.y0
    this.jellyParts = new Map()
    this.headJellyElement = faceHeight > 0
      ? { anchorX: face.cx, anchorY: face.y1, axisX: 0, axisY: -1, length: faceHeight * 1.5, cutY: null }
      : null
    if (!this.headJellyElement) return
    for (const layer of this.layers) {
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

  private stepArms(dt: number): void {
    const e = this.current
    const frame = this.secondaryDeformationFrame
    if (!e.phys) this.prepareHeadDeformationFrame()
    const input = { open: e.armY, sway: e.armPos, bodyRoll: e.body * BODY_ROLL_RADIANS, dynamic: e.phys }
    for (const side of ['L', 'R'] as const) {
      const joint = this.writeArmJoint(side) ? this.armJoint : null
      const shoulder = this.shoulders[side]
      shoulder.found = joint !== null
      shoulder.x = this.armJoint.x
      shoulder.y = this.armJoint.y
      const angle = this.armPendulums[side].step(input, joint, dt)
      const drape = this.armDrapes[side].step(angle, input.bodyRoll, input.dynamic, dt)
      if (side === 'L') {
        frame.armAngleL = angle
        frame.armDrapeL = drape
      } else {
        frame.armAngleR = angle
        frame.armDrapeR = drape
      }
    }
  }

  /** The shoulder joint after all primary motion, including the shader's body roll. */
  private writeArmJoint(side: 'L' | 'R'): boolean {
    const layer = this.layers.find((candidate) =>
      candidate.secondaryDeformation.arm && candidate.secondaryDeformation.handwearSide === side)
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

  /** Shared primary pose for physics substeps and the final visible mesh. */
  private prepareHeadDeformationFrame(): void {
    const e = this.current
    const frame = this.secondaryDeformationFrame
    const anchors = this.playback.anchors
    const breath = 0.5 + chestBreathResidual(this.time)
    const breathHead = 0.5 + 0.5 * Math.sin((this.time * Math.PI * 2) / 3.4 - 0.6)
    const faceHeight = anchors.face.y1 - anchors.face.y0
    const span = Math.max(1, anchors.bodyPivot.y - anchors.neckBottom)
    Object.assign(this.bodyLiftField, {
      centerX: anchors.neckPivot.x,
      upperY: anchors.neckBottom + Math.min(faceHeight * 0.65, span * 0.5),
      lowerY: anchors.bodyPivot.y,
      amount: this.bodyLiftResponse.value * Math.min(faceHeight * 0.05, span * 0.035),
      pitch: this.bodyPitchResponse.value * 0.18,
      depth: this.shellProfile.torso.enabled ? Math.min(this.shellProfile.torso.radiusZ, span * 0.45) : 0,
      shoulderY: anchors.neckBottom,
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

  /**
   * Write this frame's shared deformation inputs: chest, jaw and mouth frames,
   * layer opacity, and the collar clip. Returns what changed since last frame.
   */
  private prepareDeformationFrames(work?: Anime25DFrameWork) {
    this.prepareHeadDeformationFrame()
    const A = this.playback.anchors
    const e = this.current
    const fs = A.faceScale
    const t = this.time
    const breathResidual = chestBreathResidual(t)
    const cz = this.secondaryDeformationFrame.headRotationCosine
    const sz = this.secondaryDeformationFrame.headRotationSine
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
    const jawDrop = this.jaw.value * this.jawTravel
    const jawOpen = Math.max(0, this.jaw.value)
    const mouthTransition = this.mouthTransition.sample(e, this.time)
    this.activeMouthMaterial = mouthTransition.material
    resolveMouthMorph(
      this.mouthMorphSources,
      e,
      A.mouth,
      A.face,
      this.mouthMorph,
    )
    applyMouthTransitionBridge(this.mouthMorph, mouthTransition)
    if (this.continuousMouth && this.mouthMorph.openMix > 0) this.continuousMouth.paint(this.mouthMorph)
    const deformationFrame = this.deformationFrame
    deformationFrame.faceScale = fs
    deformationFrame.jawDrop = jawDrop
    deformationFrame.jawOpen = jawOpen
    deformationFrame.time = t
    deformationFrame.stylizedMotion = this.motion.stylizedMotion
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
    writeAnime25DOpacityFrame(
      this.opacityFrame,
      e,
      this.activeMouthMaterial,
      this.motion.sillyMouthShare,
      mouthTransition,
      this.continuousMouth !== null,
    )
    const deformationChanges = captureAnime25DDeformationChanges(
      this.deformationChangeState,
      e,
      this.mouthMorph,
      jawDrop,
      jawOpen,
      this.motion.stylizedMotion,
      this.irisRebound,
    )
    for (const layer of this.layers) {
      layer.frameOpacity =
        fadeOpacityFromFrame(layer.source, this.opacityFrame) *
        this.closedEyes.opacity(layer.source)
    }
    const collarMotion = this.collarMotion
    collarMotion.angleX = e.angleX
    collarMotion.angleY = e.angleY
    collarMotion.headRotationCosine = cz
    collarMotion.headRotationSine = sz
    collarMotion.bodyBreathOffset = secondaryDeformationFrame.bodyBreathOffset
    collarMotion.headBreathOffset = secondaryDeformationFrame.headBreathOffset
    collarMotion.torsoShellBlend = secondaryDeformationFrame.torsoShellBlend
    collarMotion.torsoNeckOffsetX = secondaryDeformationFrame.torsoNeckOffsetX
    if (this.collarClip) {
      deformCollarClipMesh(this.collarClip, collarMotion, this.neckDepth)
      if (work) {
        work.deformedLayers += 1
        work.deformedVertices += this.collarClip.rest.length / 2
      }
    }
    return deformationChanges
  }

  private deform(work?: Anime25DFrameWork): void {
    const deformationChanges = this.prepareDeformationFrames(work)
    const e = this.current
    const t = this.time
    const secondaryDeformationFrame = this.secondaryDeformationFrame
    const context = this.layerContext
    context.anchors = this.playback.anchors
    context.current = e
    context.time = t
    context.deformationFrame = this.deformationFrame
    context.secondaryDeformationFrame = secondaryDeformationFrame
    context.headJellyElement = e.phys ? this.headJellyElement : null
    context.headStretch = this.headJelly.stretch
    for (const layer of this.layers) {
      const visible =
        shouldDeformLayer(layer.source, layer.frameOpacity) ||
        Boolean(
          layer.attachmentDependents?.some((child) =>
            shouldDeformLayer(child.source, child.frameOpacity),
          ),
        )
      const updateLocalGeometry = layer.deformationPlan.cacheable
        ? shouldUpdateAnime25DLayerGeometry(
            layer.deformationPlan,
            deformationChanges,
            visible,
          )
        : true
      if (!visible) continue
      const deformed = layer.surfaceContact?.unconstrained ?? layer.hairSurface?.candidate ?? layer.deformed
      const vertexCount = layer.rest.length / 2
      if (!layer.attachment && layer.shaderGlobalTransform) {
        writeRigidLayerTransform(layer, context)
      }
      // Do not deform/mark it dirty and later upload into a null binding.
      if (!layer.vertexBuffer) {
        layer.geometryDirty = false
        continue
      }
      if (!layer.localDynamic) {
        if (work) {
          work.shaderOnlyLayers += 1
          work.skippedVertices += vertexCount
          work.savedUploadBytes += deformed.byteLength
        }
        continue
      }
      if (!updateLocalGeometry) {
        if (work) {
          work.skippedVertices += vertexCount
          work.savedUploadBytes += deformed.byteLength
        }
        continue
      }
      if (work) {
        work.deformedLayers += 1
        work.deformedVertices += vertexCount
      }
      const geometryChanged = deformLayerVertices(
        layer,
        context,
        e.phys ? this.jellyParts.get(layer) : undefined,
      )
      if (layer.deformationPlan.cacheable) {
        markAnime25DLayerGeometryUpdated(layer.deformationPlan)
      }
      // Contact layers compare only their final output, never the intermediate
      // free arm, against the surface retained by attachments and the GPU.
      if (layer.surfaceContact) continue
      if (!geometryChanged) {
        layer.geometryDirty = false
        if (work) work.savedUploadBytes += deformed.byteLength
        continue
      }
      layer.geometryDirty = true
    }
    const settledBytes = settleDependentLayers(
      this.layers,
      secondaryDeformationFrame,
      t,
      e,
    )
    if (work) work.savedUploadBytes += settledBytes
  }

  private uploadGeometry(work?: Anime25DFrameWork): void {
    const { gl } = this
    if (this.collarClip) {
      const uploadStarted = work ? performance.now() : 0
      uploadCollarClipMesh(gl, this.collarClip)
      if (work) {
        work.uploadedBytes += this.collarClip.deformed.byteLength
        work.uploadSubmitMs += performance.now() - uploadStarted
      }
    }
    for (const layer of this.layers) {
      if (!layer.geometryDirty) continue
      gl.bindBuffer(gl.ARRAY_BUFFER, layer.vertexBuffer)
      const uploadStarted = work ? performance.now() : 0
      gl.bufferSubData(gl.ARRAY_BUFFER, 0, layer.deformed)
      layer.geometryDirty = false
      if (work) {
        work.uploadedBytes += layer.deformed.byteLength
        work.uploadSubmitMs += performance.now() - uploadStarted
      }
    }
  }

  private draw(work?: Anime25DFrameWork): void {
    this.renderFrame.time = this.time
    this.renderFrame.eyeCry = this.current.eyeCry
    drawAnime25DFrame(
      this.gl,
      this.program,
      this.rendererBindings,
      this.layers,
      this.atlasTexture,
      this.collarClip,
      this.renderFrame,
      work,
    )
    const sampled = this.motion.performanceExpression.getSampledTouch()
    if (this.atlasTexture) {
      const a = this.playback.anchors
      this.writeHeadWorldTransform(this.headWorldTransform)
      const faceWidth = a.face.x1 - a.face.x0
      const point = this.jellyAnchor
      this.headWorldPoint(a.face.x1 - faceWidth * 0.04, a.face.y0 + (a.face.y1 - a.face.y0) * 0.08, point)
      const frame = this.renderFrame
      this.thinkingSticker.draw(this.gl, this.time,
        this.speechActive || this.target.talk ? 0 : this.target.thinking ? 1 : this.motion.performanceExpression.getThinkingLevel(),
        point.x, point.y, faceWidth * 0.155, frame.viewWidth, frame.viewHeight)
    }
    this.presentedTouch = sampled
      ? { ...sampled, atMs: performance.now() }
      : null
  }
}
