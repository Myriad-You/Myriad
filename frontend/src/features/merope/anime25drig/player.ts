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
  ChestWeightField,
} from './chestPhysics'
import type { CollarClipMesh, CollarMotionPose } from './collarRuntime'
import type { Anime25DDeformationChangeState } from './deformationDependencies'
import type { Anime25DDriver } from './driver'
import type {
  Anime25DBlinkState,
  Anime25DStylizedTargets,
} from './driverComposition'
import type { Anime25DExpressionDeformationFrame } from './expressionDeformation'
import type { Anime25DHairSpringFrame } from './hairPhysics'
import type { Anime25DGpuLayer } from './layerGpuBinding'
import type { Anime25DMotionEnvelopeProfile } from './motionEnvelope'
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
import type { Anime25DRendererBindings, Anime25DRenderFrame } from './renderer'
import type { Anime25DSecondaryDeformationFrame } from './secondaryDeformation'
import type { Anime25DShellRotation } from './shellDeformation'
import type { StylizedExpressionMotion } from './stylizedExpressionMotion'
import type {
  Anime25DTorsoShellRotation,
  Anime25DTorsoYawState,
} from './torsoDeformation'
import type {
  TouchAtlas,
  TouchPaintLayer,
  VisibleTouchHit,
} from './touchVisibility'
import type { Anime25DPlayback, Anime25DShellProfile } from './types'
import { currentCopy } from '../../../i18n/localeCopy'
import { allowsPointerGaze, IDLE_MOTION_POLICY } from '../motion/policy'
import { resolveAnime25DLayerSemantics } from '../rig/anime25dLayerSemantics'
import { SingingGrooveController } from '../singing/singingGroove'
import { noteTurnTraceFrame } from '../turnTrace'
import { AmbientMotionController } from './ambientMotion'
import { ArmFollowController } from './armFollow'
import { Anime25DBehaviorMotionController } from './behaviorMotion'
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
  BODY_HEAD_FOLLOW,
  deformCollarClipMesh,
  disposeCollarClipMesh,
  uploadCollarClipMesh,
} from './collarRuntime'
import { cryTearHorizontalOffset, cryTearVerticalOffset } from './cryMotion'
import {
  captureAnime25DDeformationChanges,
  createAnime25DDeformationChangeState,
  markAnime25DLayerGeometryUpdated,
  shouldUpdateAnime25DLayerGeometry,
} from './deformationDependencies'
import { IDENTITY_DRIVER, sanitizeDriverPatch } from './driver'
import {
  applyAnime25DComposedPose,
  applyAnime25DCryMouth,
  applyAnime25DSillyMouthOwnership,
  applyAnime25DSpeechExtras,
  applyAnime25DStylizedExpression,
  captureAnime25DSecondaryMotion,
  prepareAnime25DWorkingTarget,
  resolveAnime25DStylizedTargets,
  smoothAnime25DUnit,
  stepAnime25DBlink,
  stepAnime25DDriverResponse,
} from './driverComposition'
import { deformAnime25DExpressionPoint } from './expressionDeformation'
import { releaseThinkingExpression } from './expressionPresets'
import { expressiveEyeOpenOffset } from './expressiveMotionEnvelope'
import {
  animationCatchupSeconds,
  animationElapsedSeconds,
  animationSubstepCount,
} from './frameClock'
import { stepAnime25DHairLayerSprings } from './hairPhysics'
import { constrainHairSurface } from './hairSurface'
import { idleBreathOffset } from './idleBreath'
import { Anime25DIrisRebound } from './irisRebound'
import {
  createJawMotionState,
  jawMotionTarget,
  jawTravelPixels,
  stepJawMotion,
} from './jawMotion'
import { deformNeckwearBridge, writeAnime25DAttachmentTransform } from './layerAttachment'
import { deformAnime25DUpstreamFeaturePoint } from './layerDeformation'
import { compileAnime25DGpuLayers } from './layerGpuBinding'
import { writeAnime25DLayerGlobalTransform } from './layerTransform'
import {
  deriveAnime25DMotionEnvelopeProfile,
  projectAnime25DMotionEnvelope,
} from './motionEnvelope'
import { monotonicControlTime } from './motionPrediction'
import {
  deformAnime25DFaceJawPoint,
  deformAnime25DMouthPoint,
} from './mouthDeformation'
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
  PerformanceExpressionController,
} from './performanceExpression'
import { baselineDriverPatch, restEnergyDriverPatch } from './performanceMotion'
import {
  Anime25DPerformanceTelemetry,
  createAnime25DFrameWork,
} from './performanceTelemetry'
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
import { createAnime25DRendererBindings, drawAnime25DFrame } from './renderer'
import {
  resolveAnime25DRenderSurface,
  shouldApplyAnime25DResize,
} from './runtimePolicy'
import {
  deformAnime25DHairPoint,
  deformAnime25DSecondaryPoint,
} from './secondaryDeformation'
import { writeAnime25DShellRotation } from './shellDeformation'
import { CoSpeechExpressionController } from './speechExpression'
import { AutoSpeechController } from './speechMotion'
import { StylizedExpressionMotionController } from './stylizedExpressionMotion'
import { applySurfaceContact } from './surfaceContact'
import { ThinkingMotionController } from './thinkingMotion'
import { ThinkingSticker } from './thinkingSticker'
import {
  anime25DTorsoYawFollow,
  resolveAnime25DTorsoChestShape,
  stepAnime25DTorsoShellRotation,
} from './torsoDeformation'
import { touchPointInView } from './touchHitTest'
import { hitTestVisibleTouch, readTouchAtlas } from './touchVisibility'
import { compileProgram, createAtlasTexture, loadImage, readLayerPixels } from './webglRuntime'

const POINTER_ATTACK_RATE = 16
const POINTER_RELEASE_RATE = 5.5

interface SecondaryMotionPose {
  angleX: number
  angleY: number
  angleZ: number
  body: number
}

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

export class Anime25DPlayer {
  private readonly thinkingSticker = new ThinkingSticker()
  private readonly thinkingStickerTransform = new Float32Array(9)
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
  private readonly workingTarget: Anime25DDriver = { ...IDENTITY_DRIVER }
  private readonly secondaryCurrent: SecondaryMotionPose = {
    angleX: 0,
    angleY: 0,
    angleZ: 0,
    body: 0,
  }

  private readonly poseResponse = new PoseResponseController()

  private readonly mouthMorph: MouthMorphState = {
    centerX: 0,
    centerY: 0,
    width: 1,
    height: 1,
    openMix: 0,
    wide: 0,
    round: 0,
    narrow: 0,
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

  private readonly shellRotation: Anime25DShellRotation = {
    active: false,
    yawCosine: 1,
    yawSine: 0,
    pitchCosine: 1,
    pitchSine: 0,
  }

  private readonly torsoYaw: Anime25DTorsoYawState = { value: 0 }

  /** Passive sleeve response to the torso, derived rather than authored. */
  private readonly armFollow = new ArmFollowController()

  private armSwing = 0

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
  private sillyMouthShare = 1

  private time = 0
  private readonly irisRebound = new Anime25DIrisRebound()
  private readonly closedEyes = new ClosedEyePresentation()
  private readonly blinkState: Anime25DBlinkState = {
    activeSeconds: -1,
    nextAtSeconds: 1.8,
  }

  private readonly ambientMotion = new AmbientMotionController()
  private readonly behaviorMotion = new Anime25DBehaviorMotionController()
  private responseScale = 1
  private controlTime = 0
  private readonly occupancy = new PoseOccupancyController()
  private readonly poseGate = new PoseGateController()
  private readonly randomAction = new RandomActionController()
  private readonly singingGroove = new SingingGrooveController()
  private musicSignal: MusicMotionSignal | null = null
  private readonly thinkingMotion = new ThinkingMotionController()
  private readonly stylizedExpression = new StylizedExpressionMotionController()
  private readonly stylizedTargets: Anime25DStylizedTargets = {
    anger: 0,
    speechless: 0,
    maniac: 0,
    silly: 0,
    lovestruck: 0,
  }

  private stylizedMotion: Readonly<StylizedExpressionMotion> | null = null
  private stylizedHeadShare = 1
  private readonly performanceExpression = new PerformanceExpressionController()
  private presentedTouch: PresentedTouchReaction | null = null

  getPresentedTouch(): PresentedTouchReaction | null {
    return this.presentedTouch
  }

  /** Reused so the per-frame pose composition never allocates. */
  private readonly composedPose = zeroOccupancyOffset()
  private readonly breathPose = { angleX: 0, angleY: 0, angleZ: 0, body: 0 }
  private readonly speechMotion = new AutoSpeechController()
  private readonly speechExpression = new CoSpeechExpressionController()
  private readonly cryMouth = {
    mouthOpen: 0,
    mouthForm: 0,
    mouthCY: 0,
    mouthScale: 0,
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
  private chestWeightField: ChestWeightField | null = null
  private readonly jaw = createJawMotionState()
  private jawTravel = 0
  private motionEnvelopeProfile!: Anime25DMotionEnvelopeProfile
  private readonly motionEnvelopeResult = {
    clippedEnergy: 0,
    transferredEnergy: 0,
  }

  private neckDepth = 0
  private collarClip: CollarClipMesh | null = null
  private jawEmphasis = 0
  private readonly mouse = { x: 0, y: 0, inside: false }
  private pointerAuthority = 0
  private policy: MotionChannelPolicy = { ...IDLE_MOTION_POLICY }
  private disposed = false
  private atlasAbort: AbortController | null = null

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
      this.applyPackage(playback, rigManifest)
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
    )
    this.singingGroove.setArmMotion(this.motionEnvelopeProfile.armMotion)
    this.neckDepth =
      playback.layers.find((layer) => layer.role === 'neck')?.depth ?? 0.95
    this.mouthTransition = new MouthTransitionController(playback.mouthProfile)
    this.mouthMorphSources = compileAnime25DMouthMorphSources(playback.layers)
    this.deformationFrame = {
      mouth: playback.anchors.mouth,
      face: playback.anchors.face,
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
    this.chestWeightField = chestProfileUsesGeometryWeights(
      playback.chestProfile,
    )
      ? buildChestWeightField(rigManifest)
      : null
    const neckFollowTop = Math.min(
      anchors.neckBottom - 1,
      Math.max(anchors.neckTop, anchors.face.y1 + anchors.faceScale * 5),
    )
    this.secondaryDeformationFrame = {
      expression: this.current,
      faceScale: anchors.faceScale,
      headAngleY: 0,
      headRotationCosine: 1,
      headRotationSine: 0,
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
      specialHeadOffset: 0,
      highCollar: this.motionEnvelopeProfile.highCollar,
      breath: 0,
      armSwing: 0,
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
      torsoShellRotation: this.torsoShellRotation,
    }
    this.hairSpringFrame = {
      enabled: true,
      idle: true,
      angleX: 0,
      angleZ: 0,
      faceScale: anchors.faceScale,
      neckPivotY: anchors.neckPivot.y,
      faceCenterY: anchors.face.cy,
      time: 0,
    }
  }

  setTarget(partial: Partial<Anime25DDriver>): void {
    Object.assign(this.target, sanitizeDriverPatch(partial))
    if (this.speechActive || this.target.talk) releaseThinkingExpression(this.target)
  }

  replaceTarget(driver: Anime25DDriver): void {
    Object.assign(this.target, IDENTITY_DRIVER, sanitizeDriverPatch(driver))
  }

  getTarget(): Anime25DDriver {
    return { ...this.target }
  }

  getCurrent(): Anime25DDriver {
    return { ...this.current }
  }

  blinkNow(): void {
    this.blinkState.activeSeconds = 0
    this.blinkState.nextAtSeconds = this.time + 1.6 + Math.random() * 3.8
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
    if (active) releaseThinkingExpression(this.target)
  }

  setSinging(active: boolean): void {
    this.target.singing = active
  }

  setSingingTrack(trackId: string | null): void {
    this.singingGroove.setTrack(trackId)
  }

  setMusicSignal(drive: MusicMotionSignal | null): void {
    this.musicSignal = drive
  }

  setSpeechProsody(plan: SpeechProsodyPlan | null): void {
    this.speechExpression.setProsody(plan, this.time)
  }

  enqueueSpeechText(text: string, locale?: string): void {
    this.speechMotion.enqueueText(text, locale)
  }

  clearSpeechText(): void {
    this.speechMotion.clear(this.time)
  }

  setBehaviorMotionUnits(
    units: readonly Anime25DMotionUnit[],
    nowMs: number,
  ): void {
    this.behaviorMotion.replace(units, nowMs, this.time)
    this.performanceExpression.playBehaviorUnits(units, this.time, nowMs)
  }

  clearBehaviorMotionUnits(): void {
    this.behaviorMotion.clear(this.time)
    this.performanceExpression.stopBehaviors(this.time)
  }

  setBearing(bearing: PerformanceBaseline | null): void {
    this.performanceExpression.setBearingAttention(bearing?.attention ?? null)
    this.setTarget({
      ...(bearing ? baselineDriverPatch(bearing) : restEnergyDriverPatch()),
      ...bearingDriverPatch(bearing),
    })
  }

  debugSnapshot(): Anime25DDebugSnapshot {
    const layers = this.playback.layers
    return {
      layerCount: layers.length,
      hairLayerCount: layers.filter((layer) => layer.phys === 'hair').length,
      strandCount: layers.reduce((sum, layer) => sum + layer.strands.length, 0),
      eyeOpenLayers: layers.filter((layer) => layer.fade === 'eyeOpen').length,
      eyeCloseLayers: layers.filter((layer) => layer.fade === 'eyeClose')
        .length,
      eyeDizzyLayers: layers.filter((layer) => layer.fade === 'eyeDizzy')
        .length,
      eyeSqueezeLayers: layers.filter((layer) => layer.fade === 'eyeSqueeze')
        .length,
      eyeCryLayers: layers.filter((layer) => layer.fade === 'eyeCry').length,
      eyeSillyLayers: layers.filter((layer) => layer.fade === 'eyeSilly')
        .length,
      lovestruckHeartLayers: layers.filter(
        (layer) => layer.fade === 'lovestruckHeart',
      ).length,
      lovestruckFaceLayers: layers.filter(
        (layer) => layer.fade === 'lovestruckFace',
      ).length,
      lovestruckDroolLayers: layers.filter(
        (layer) => layer.fade === 'lovestruckDrool',
      ).length,
      maniacEyeShadowLayers: layers.filter(
        (layer) => layer.fade === 'maniacEyeShadow',
      ).length,
      angerMarkLayers: layers.filter((layer) => layer.fade === 'angerMark')
        .length,
      speechlessSweatLayers: layers.filter(
        (layer) => layer.fade === 'speechlessSweat',
      ).length,
      mouthOpenLayers: layers.filter((layer) => layer.fade === 'mouthOpen')
        .length,
      mouthWideLayers: layers.filter((layer) => layer.fade === 'mouthWide')
        .length,
      mouthRoundLayers: layers.filter((layer) => layer.fade === 'mouthRound')
        .length,
      mouthNarrowLayers: layers.filter((layer) => layer.fade === 'mouthNarrow')
        .length,
      mouthCloseLayers: layers.filter((layer) => layer.fade === 'mouthClose')
        .length,
      mouthCryLayers: layers.filter((layer) => layer.fade === 'mouthCry')
        .length,
      mouthManiacLayers: layers.filter((layer) => layer.fade === 'mouthManiac')
        .length,
      mouthSillyLayers: layers.filter((layer) => layer.fade === 'mouthSilly')
        .length,
      canvas: { ...this.playback.pixelCanvas },
      motionEnvelope: {
        highCollar: this.motionEnvelopeProfile.highCollar,
        armMotion: this.motionEnvelopeProfile.armMotion,
        pitchLimit: this.motionEnvelopeProfile.pitch.limit,
        torsoLimit: this.motionEnvelopeProfile.torso.limit,
        armLimit: this.motionEnvelopeProfile.rigidArm.limit,
        transferredEnergy: this.motionEnvelopeResult.transferredEnergy,
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
    this.layers = []
    this.collarClip = null
    this.atlasTexture = null
    this.touchAtlas = null
    this.touchLayers = []
    this.gl.deleteProgram(this.program)
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
      this.speechActive || this.target.talk ? 0 : this.target.thinking ? 1 : this.performanceExpression.getThinkingLevel())
    this.shellActivation = Math.min(1, this.shellActivation + dt * 8)
    const t = this.time
    const pointer =
      this.target.mouse && allowsPointerGaze(this.policy.gaze)
        ? this.mouse
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
      this.target,
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
    const semanticExpression = this.performanceExpression.sample(
      controlTime,
      tgt,
      this.speechActive || this.target.talk,
    )
    const stylizedTargets = resolveAnime25DStylizedTargets(
      this.stylizedTargets,
      tgt,
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
    const pointerDriven = this.target.mouse && pointer.inside
    const speech = this.speechMotion.sample(controlTime, this.target.talk)
    this.jawEmphasis = speech.browAccent
    const speaking = this.speechActive || this.target.talk
    const speechExpression = this.speechExpression.sample(
      controlTime,
      speaking,
      this.speechActive && !this.target.talk ? tgt.mouthOpen : null,
      speech.phraseActivity,
      speech.browAccent,
      speech.headAccent,
      behaviorMotion.coSpeechQuality,
      behaviorMotion.coSpeechGesture,
    )
    const singing = this.target.singing
    const vocalizing =
      speaking ||
      (singing &&
        (behaviorMotion.musicMode === 'sing' ||
          behaviorMotion.musicMode === 'hum'))
    const groove = this.singingGroove.sample(
      t,
      singing,
      this.musicSignal,
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
      thinking: this.target.thinking,
      pointerDriven,
      automation: this.target.rand,
      sticker,
    })
    const randomAction = this.randomAction.sample(
      t,
      this.target.rand,
      this.performanceExpression.getActiveLevel() >= DIRECTED_BODY_BLOCK_LEVEL,
    )
    const ambient = this.ambientMotion.sample(t, this.target.rand)
    const thinking = this.thinkingMotion.sample(t, this.target.thinking && !speaking, tgt)
    const breath = idleBreathOffset(t, this.breathPose)
    const gate = this.poseGate.sample(
      dt,
      applyBehaviorMotionGate(
        resolvePoseGate(this.policy, occupancy, {
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
    const armFollow = this.armFollow.step(
      this.torsoYaw.value,
      dt,
      this.motionEnvelopeProfile.rigidArm.limit,
    )
    tgt.armY += armFollow.lift
    this.armSwing = armFollow.swing
    projectAnime25DMotionEnvelope(
      tgt,
      this.motionEnvelopeProfile,
      this.motionEnvelopeResult,
    )
    this.closedEyes.step(tgt, dt)
    stepAnime25DBlink(
      tgt,
      this.blinkState,
      this.time,
      dt,
      this.target.blink,
      stylizedTargets.maniac > 0.03 || stylizedTargets.silly > 0.03,
    )
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
    stepAnime25DDriverResponse(
      this.current,
      this.target,
      tgt,
      this.poseResponse,
      dt,
      this.responseScale,
    )
    this.irisRebound.step(
      this.time,
      this.blinkState.activeSeconds,
      !this.target.blink ||
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
      Math.max(this.current.eyeOpenL, this.current.eyeOpenR),
    )
    captureAnime25DSecondaryMotion(this.secondaryCurrent, this.current)
    stepAnime25DTorsoShellRotation(
      this.torsoYaw,
      this.current.angleX,
      this.current.body,
      dt,
      this.torsoShellRotation,
      anime25DTorsoYawFollow(this.shellProfile.torso, this.current.bodyYaw),
    )
  }

  private updateSprings(dt: number): void {
    const { anchors } = this.playback
    const faceScale = anchors.faceScale
    const e = this.current
    stepJawMotion(this.jaw, jawMotionTarget(e, this.jawEmphasis), dt)
    const secondary = this.secondaryCurrent
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
    hairSpringFrame.angleX = secondary.angleX
    hairSpringFrame.angleZ = secondary.angleZ
    hairSpringFrame.time = this.time
    stepAnime25DHairLayerSprings(this.layers, hairSpringFrame, dt)
  }

  private deform(work?: Anime25DFrameWork): void {
    const A = this.playback.anchors
    const e = this.current
    const fs = A.faceScale
    const t = this.time
    const breathResidual = chestBreathResidual(t)
    const breath = 0.5 + breathResidual
    const breathHead = 0.5 + 0.5 * Math.sin((t * Math.PI * 2) / 3.4 - 0.6)
    const npx = A.neckPivot.x
    const npy = A.neckPivot.y
    const bpx = A.bodyPivot.x
    const bpy = A.bodyPivot.y
    // Music extent is authored before composition/envelope/response, never after them.
    const az = e.angleZ * 0.07
    const ay = e.angleY
    const cz = Math.cos(az)
    const sz = Math.sin(az)
    const ab = e.body * 0.028
    const cb = Math.cos(ab)
    const sb = Math.sin(ab)
    this.renderFrame.bodyPivotX = bpx
    this.renderFrame.bodyPivotY = bpy
    this.renderFrame.bodyRotationCosine = cb
    this.renderFrame.bodyRotationSine = sb
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
    const specialHeadOffset = this.stylizedMotion
      ? (this.stylizedMotion.maniacHeadPulse * 80 +
          this.stylizedMotion.sillyHeadPulse * 8 +
          this.stylizedMotion.lovestruckHeadPulse * 5) *
        this.stylizedHeadShare *
        fs
      : 0
    const mouthTransition = this.mouthTransition.sample(e)
    this.activeMouthMaterial = mouthTransition.material
    resolveMouthMorph(
      this.mouthMorphSources,
      e,
      A.mouth,
      A.face,
      this.mouthMorph,
    )
    applyMouthTransitionBridge(this.mouthMorph, mouthTransition)
    const deformationFrame = this.deformationFrame
    deformationFrame.faceScale = fs
    deformationFrame.jawDrop = jawDrop
    deformationFrame.jawOpen = jawOpen
    deformationFrame.time = t
    deformationFrame.stylizedMotion = this.stylizedMotion
    const deformationPoint = this.deformationPoint
    const secondaryDeformationFrame = this.secondaryDeformationFrame
    writeAnime25DShellRotation(e.angleX, ay, this.shellRotation)
    secondaryDeformationFrame.headAngleY = ay
    secondaryDeformationFrame.headRotationCosine = cz
    secondaryDeformationFrame.headRotationSine = sz
    secondaryDeformationFrame.bodyBreathOffset = breath * 2
    secondaryDeformationFrame.headBreathOffset = breathHead * 1.6
    secondaryDeformationFrame.specialHeadOffset = specialHeadOffset
    secondaryDeformationFrame.breath = breath
    secondaryDeformationFrame.armSwing = this.armSwing
    secondaryDeformationFrame.chestMotionCenterY = chestCenterY
    if (secondaryDeformationFrame.torsoChestShape) {
      secondaryDeformationFrame.torsoChestShape.centerY = chestCenterY
    }
    secondaryDeformationFrame.inverseChestRadiusX = inverseChestRx
    secondaryDeformationFrame.inverseChestRadiusY = inverseChestRy
    secondaryDeformationFrame.chestOffsetX = chestOffsetX
    secondaryDeformationFrame.chestOffsetY = chestOffsetY
    secondaryDeformationFrame.chestVolumeScale =
      1 + breathResidual * this.chestDynamics.breathVolumeScale
    secondaryDeformationFrame.shellBlend =
      this.shellProfile.blend * this.shellActivation
    secondaryDeformationFrame.shellActivation = this.shellActivation
    secondaryDeformationFrame.torsoShellBlend =
      this.shellProfile.enabled && this.shellProfile.torso.enabled
        ? this.shellProfile.blend *
          this.shellProfile.torso.blend *
          this.shellActivation
        : 0
    writeAnime25DOpacityFrame(
      this.opacityFrame,
      e,
      this.activeMouthMaterial,
      this.sillyMouthShare,
    )
    const deformationChanges = captureAnime25DDeformationChanges(
      this.deformationChangeState,
      e,
      this.mouthMorph,
      jawDrop,
      jawOpen,
      this.stylizedMotion,
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
    if (this.collarClip) {
      deformCollarClipMesh(this.collarClip, collarMotion, this.neckDepth)
      if (work) {
        work.deformedLayers += 1
        work.deformedVertices += this.collarClip.rest.length / 2
      }
    }
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
      const rest = layer.rest
      const deformed = layer.surfaceContact?.unconstrained ?? layer.hairSurface?.candidate ?? layer.deformed
      const vertexCount = rest.length / 2
      const source = layer.source
      const bn = layer.baseRole
      const isHead = source.group === 'head'
      if (!layer.attachment && layer.shaderGlobalTransform) {
        writeAnime25DLayerGlobalTransform(
          {
            headFollow: isHead
              ? 1
              : source.group === 'body'
                ? BODY_HEAD_FOLLOW
                : 0,
            headRotationCosine: cz,
            headRotationSine: sz,
            neckPivotX: npx,
            neckPivotY: npy,
            faceScale: fs,
            angleX: e.angleX,
            angleY: ay,
            depthOffset: source.depth - 1,
            faceCenterY: A.face.cy,
            specialOffsetY: isHead ? specialHeadOffset : 0,
            breathOffset: isHead
              ? secondaryDeformationFrame.headBreathOffset
              : secondaryDeformationFrame.bodyBreathOffset,
          },
          layer.layerTransform,
        )
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
      const upstreamFeature = layer.upstreamFeature
      const cryLayer = source.fade === 'eyeCry'
      const tearVertical = cryLayer
        ? cryTearVerticalOffset(t, source.side, e.eyeCry, fs)
        : 0
      const tearHorizontal = cryLayer
        ? cryTearHorizontalOffset(t, source.side, e.eyeCry, fs)
        : 0
      const mouthDeformation = layer.mouthDeformation
      let geometryChanged = false
      for (let vertex = 0; vertex < vertexCount; vertex += 1) {
        const index = vertex * 2
        const previousX = deformed[index]
        const previousY = deformed[index + 1]
        let x = rest[index]
        let y = rest[index + 1]
        if (upstreamFeature) {
          deformationPoint.x = x
          deformationPoint.y = y
          deformAnime25DUpstreamFeaturePoint(
            deformationPoint,
            upstreamFeature,
            this.irisRebound,
          )
          x = deformationPoint.x
          y = deformationPoint.y
        }
        if (layer.expressionDeformation) {
          deformationPoint.x = x
          deformationPoint.y = y
          deformAnime25DExpressionPoint(
            deformationPoint,
            rest[index + 1],
            layer.expressionDeformation,
            tearHorizontal,
            tearVertical,
            deformationFrame,
          )
          x = deformationPoint.x
          y = deformationPoint.y
        }
        if (mouthDeformation) {
          deformationPoint.x = x
          deformationPoint.y = y
          deformAnime25DMouthPoint(
            deformationPoint,
            rest[index],
            rest[index + 1],
            source,
            deformationFrame,
            mouthDeformation,
          )
          x = deformationPoint.x
          y = deformationPoint.y
        }
        if (bn === 'face') {
          deformationPoint.x = x
          deformationPoint.y = y
          deformAnime25DFaceJawPoint(
            deformationPoint,
            rest[index + 1],
            deformationFrame,
          )
          x = deformationPoint.x
          y = deformationPoint.y
        }
        deformationPoint.x = x
        deformationPoint.y = y
        deformAnime25DSecondaryPoint(
          deformationPoint,
          rest[index],
          rest[index + 1],
          vertex,
          layer.secondaryDeformation,
          secondaryDeformationFrame,
        )
        if (layer.hairSurface) {
          layer.hairSurface.base[index] = deformationPoint.x
          layer.hairSurface.base[index + 1] = deformationPoint.y
        }
        deformAnime25DHairPoint(
          deformationPoint,
          vertex,
          layer.secondaryDeformation,
          secondaryDeformationFrame,
        )
        x = deformationPoint.x
        y = deformationPoint.y
        if (x !== previousX || y !== previousY) {
          geometryChanged = true
          deformed[index] = x
          deformed[index + 1] = y
        }
      }
      if (layer.hairSurface) {
        constrainHairSurface(layer.hairSurface)
        geometryChanged = false
        for (let i = 0; i < deformed.length; i++) {
          if (layer.deformed[i] !== deformed[i]) {
            layer.deformed[i] = deformed[i]
            geometryChanged = true
          }
        }
      }
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
    for (const layer of this.layers) {
      if (
        !layer.surfaceContact ||
        !shouldDeformLayer(layer.source, layer.frameOpacity)
      ) {
        continue
      }
      layer.geometryDirty = applySurfaceContact(
        layer.surfaceContact,
        layer.surfaceContact.unconstrained,
        layer.deformed,
      )
      if (work && !layer.geometryDirty)
        work.savedUploadBytes += layer.deformed.byteLength
    }
    // Hosts may be later in draw order. Resolve attachments only after all host
    // vertices include this frame's shell, breathing and hair physics.
    for (const layer of this.layers) {
      if (
        layer.neckwearBridge &&
        shouldDeformLayer(layer.source, layer.frameOpacity)
      ) {
        deformNeckwearBridge(
          layer.neckwearBridge,
          secondaryDeformationFrame,
          layer.rest,
          layer.deformed,
        )
        layer.layerTransform.fill(0)
        layer.layerTransform[0] =
          layer.layerTransform[4] =
          layer.layerTransform[8] =
            1
        layer.geometryDirty = true
      } else if (layer.attachment) {
        writeAnime25DAttachmentTransform(
          layer.attachment,
          secondaryDeformationFrame,
          layer.layerTransform,
        )
}
    }
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
    const sampled = this.performanceExpression.getSampledTouch()
    if (this.atlasTexture) {
      const a = this.playback.anchors
      const f = this.secondaryDeformationFrame
      const m = this.thinkingStickerTransform
      writeAnime25DLayerGlobalTransform({
        headFollow: 1, headRotationCosine: f.headRotationCosine,
        headRotationSine: f.headRotationSine, neckPivotX: f.neckPivotX,
        neckPivotY: f.neckPivotY, faceScale: f.faceScale,
        angleX: this.current.angleX, angleY: f.headAngleY, depthOffset: 0.3,
        faceCenterY: f.faceCenterY, specialOffsetY: f.specialHeadOffset,
        breathOffset: f.headBreathOffset,
      }, m)
      const faceWidth = a.face.x1 - a.face.x0
      const x = a.face.x1 - faceWidth * 0.04
      const y = a.face.y0 + (a.face.y1 - a.face.y0) * 0.08
      const frame = this.renderFrame
      const px = m[0] * x + m[3] * y + m[6] - frame.bodyPivotX
      const py = m[1] * x + m[4] * y + m[7] - frame.bodyPivotY
      this.thinkingSticker.draw(this.gl, this.time,
        this.speechActive || this.target.talk ? 0 : this.target.thinking ? 1 : this.performanceExpression.getThinkingLevel(),
        frame.bodyPivotX + px * frame.bodyRotationCosine - py * frame.bodyRotationSine,
        frame.bodyPivotY + px * frame.bodyRotationSine + py * frame.bodyRotationCosine,
        faceWidth * 0.155, frame.viewWidth, frame.viewHeight)
    }
    this.presentedTouch = sampled
      ? { ...sampled, atMs: performance.now() }
      : null
  }
}
