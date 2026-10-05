import type { PerformanceBaseline } from '../../../services/agent/types'
import type { PresentedTouchReaction } from '../interaction/touchReaction'
import type { MotionChannelPolicy } from '../motion/policy'
import type { MeropeRigManifest } from '../rig/types'
import type { MusicMotionSignal } from '../singing/musicSignal'
import type { SpeechProsodyPlan } from '../speech/prosody'
import type { Anime25DMotionUnit } from './behaviorMotion'

import type { CollarClipMesh } from './collarRuntime'

import type { Anime25DDriver } from './driver'
import type { FaceFrameInput } from './faceFrames'
import type { LayerDeformationContext } from './layerFrameDeformation'
import type { Anime25DGpuLayer } from './layerGpuBinding'
import type { MotionComposerInput } from './motionComposer'
import type { Anime25DHandPose, Anime25DMotionEnvelopeProfile } from './motionEnvelope'

import type {
  Anime25DFrameWork,
} from './performanceTelemetry'
import type { Anime25DDebugSnapshot } from './playerDebug'
import type { PoseCorrection } from './poseCorrections'

import type { Anime25DRendererBindings, Anime25DRenderFrame } from './renderer'
import type {
  TouchAtlas,
  TouchPaintLayer,
  VisibleTouchHit,
} from './touchVisibility'
import type { Anime25DPlayback } from './types'
import { currentCopy } from '../../../i18n/localeCopy'
import { noteTurnTraceFrame } from '../events/turnTrace'
import { IDLE_MOTION_POLICY } from '../motion/policy'
import { boundRigCapabilities } from '../motion/rigStateSummary'
import { resolveAnime25DLayerSemantics } from '../rig/anime25dLayerSemantics'
import { Anime25DBodyFrames } from './bodyFrames'

import { ClosedEyePresentation } from './closedEyePresentation'

import {
  deformCollarClipMesh,
  uploadCollarClipMesh,
} from './collarRuntime'
import { IDENTITY_DRIVER, sanitizeDriverPatch } from './driver'
import { ThinkingExpressionOwnership } from './expressionPresets'

import { Anime25DFaceFrames } from './faceFrames'
import {
  animationCatchupSeconds,
  animationElapsedSeconds,
  animationSubstepCount,
} from './frameClock'
import { Anime25DIrisRebound } from './irisRebound'

import { deformLayers, settleDependentLayers } from './layerFrameDeformation'
import { Anime25DMotionComposer } from './motionComposer'

import {
  deriveAnime25DMotionEnvelopeProfile,
} from './motionEnvelope'
import {
  bearingDriverPatch,
} from './performanceExpression'
import { baselineDriverPatch, restEnergyDriverPatch } from './performanceMotion'
import {
  Anime25DPerformanceTelemetry,
  createAnime25DFrameWork,
} from './performanceTelemetry'
import { describeAnime25DPlayback } from './playerDebug'
import {
  prepareLivePackage,
  releaseCompiledGpu,
  touchLayersFor,
} from './playerPackage'

import { bindPoseCorrections, isPoseCorrections } from './poseCorrections'

import {
  createAnime25DRendererBindings,
  disposeAnime25DRendererBindings,
  drawAnime25DFrame,
} from './renderer'
import {
  resolveAnime25DRenderSurface,
  shouldApplyAnime25DResize,
} from './runtimePolicy'

import { ThinkingSticker } from './thinkingSticker'

import { touchPointInView } from './touchHitTest'
import { hitTestVisibleTouch } from './touchVisibility'
import { compileProgram, loadImage } from './webglRuntime'

export type { Anime25DDebugSnapshot } from './playerDebug'

export class Anime25DPlayer {
  private readonly thinkingSticker = new ThinkingSticker()
  private readonly gl: WebGL2RenderingContext
  private playback!: Anime25DPlayback
  private rigManifest: MeropeRigManifest | undefined
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
  private motionCapabilities: readonly string[] = []
  private atlasTexture: WebGLTexture | null = null
  private touchAtlas: TouchAtlas | null = null
  private touchLayers: TouchPaintLayer[] = []
  private readonly performanceTelemetry = new Anime25DPerformanceTelemetry()
  private readonly current: Anime25DDriver = { ...IDENTITY_DRIVER }
  private readonly target: Anime25DDriver = { ...IDENTITY_DRIVER }
  private readonly deformationPoint = { x: 0, y: 0 }
  private readonly jellyShift = { x: 0, y: 0 }

  private time = 0
  private readonly irisRebound = new Anime25DIrisRebound()
  private readonly closedEyes = new ClosedEyePresentation()
  private readonly motion = new Anime25DMotionComposer(
    this.current,
    this.closedEyes,
    this.irisRebound,
  )

  private readonly body = new Anime25DBodyFrames(
    this.current,
    this.renderFrame,
    this.motion,
  )

  private readonly face = new Anime25DFaceFrames(
    this.current,
    this.irisRebound,
    this.closedEyes,
  )

  private readonly thinkingExpression = new ThinkingExpressionOwnership()
  private musicSignal: MusicMotionSignal | null = null
  private presentedTouch: PresentedTouchReaction | null = null

  getPresentedTouch(): PresentedTouchReaction | null {
    return this.presentedTouch
  }

  private speechActive = false
  private motionEnvelopeProfile!: Anime25DMotionEnvelopeProfile
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

  private readonly faceInput: FaceFrameInput = {
    time: 0,
    jawDrop: 0,
    jawOpen: 0,
    stylizedMotion: null,
    sillyMouthShare: 1,
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
    const prepared = prepareLivePackage(this.gl, this.program, playback, rigManifest, image, this.current)
    const { resolved, compiled } = prepared
    try {
      if (this.disposed || atlasAbort.signal.aborted) {
        releaseCompiledGpu(this.gl, compiled.layers, compiled.collarClip, prepared.texture)
        return
      }
      this.applyPackage(playback, rigManifest, {
        linked: compiled.armsLinked,
        touchingHead: compiled.handTouchesHead,
      })
    } catch (error) {
      // Keep the live outfit; the not-yet-owned replacement must be released.
      releaseCompiledGpu(this.gl, compiled.layers, compiled.collarClip, prepared.texture)
      throw error
    }
    releaseCompiledGpu(this.gl, this.layers, this.collarClip, this.atlasTexture)
    this.atlasTexture = prepared.texture
    this.layers = compiled.layers
    this.collarClip = compiled.collarClip
    this.face.bindAtlas(this.gl, resolved, image, this.layers)
    this.body.bindLayers(this.layers, compiled.headSilhouette ?? null)
    const shell = resolved.shellProfile
    this.motionCapabilities = boundRigCapabilities(rigManifest, {
      torsoVolume: shell.enabled && shell.blend > 0 && shell.torso.enabled && shell.torso.blend > 0,
      arms: this.motionEnvelopeProfile.rigidArm.limit > 0
        ? this.layers.flatMap(layer => {
            const binding = layer.secondaryDeformation
            return binding.arm && binding.armMesh && (binding.handwearSide === 'L' || binding.handwearSide === 'R')
              ? [binding.handwearSide] : []
          }) : [],
    })
    this.motion.directedPose.setCapabilities(this.motionCapabilities)
    this.touchAtlas = prepared.touchAtlas
    this.thinkingSticker.setLinePixels(prepared.linePixels)
    this.touchLayers = touchLayersFor(this.layers, this.collarClip)
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
    this.motionEnvelopeProfile = deriveAnime25DMotionEnvelopeProfile(
      playback,
      rigManifest,
      hands,
    )
    this.motion.singingGroove.setArmMotion(
      this.motionEnvelopeProfile.armMotion && this.motionEnvelopeProfile.rigidArm.limit > 0,
    )
    this.motion.setStanding(playback.anchors.groundY !== undefined)
    this.body.bind(playback, this.motionEnvelopeProfile)
    this.face.bind(playback)
  }

  getMotionCapabilities(): readonly string[] { return this.motionCapabilities }

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

  setScore(score: import('../motion/scoreTimeline').ResolvedScore | null, nowMs: number): void { if (score) this.motion.directedPose.setScore(score, this.time, nowMs) }

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
    this.motion.directedPose.set(bearing?.pose, this.time)
    this.motion.performanceExpression.setBearingAttention(bearing?.attention ?? null)
    this.setTarget({
      ...(bearing ? baselineDriverPatch(bearing) : restEnergyDriverPatch()),
      ...bearingDriverPatch(bearing),
    })
  }

  debugSnapshot(): Anime25DDebugSnapshot {
    return describeAnime25DPlayback(
      this.playback,
      this.motionEnvelopeProfile,
      this.motion.motionEnvelopeResult.transferredEnergy,
      this.performanceTelemetry.observe(),
      this.getCurrent(),
    )
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
        this.body.updateSprings(dt, this.layers, this.time)
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
      this.body.updateSprings(dt, this.layers, this.time)
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
    this.face.dispose()
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
    const input = this.motionInput
    input.time = this.time
    input.policy = this.policy
    input.speechActive = this.speechActive
    input.musicSignal = this.musicSignal
    input.motionEnvelopeProfile = this.motionEnvelopeProfile
    this.motion.step(dt, input)
    this.body.stepPosture(dt, this.time)
  }

  /**
   * Write this frame's shared deformation inputs: chest, jaw and mouth frames,
   * layer opacity, and the collar clip. Returns what changed since last frame.
   */
  private prepareDeformationFrames(work?: Anime25DFrameWork) {
    const body = this.body
    body.prepareHeadFrame(this.time)
    body.prepareChestFrame(this.time)
    const faceInput = this.faceInput
    faceInput.time = this.time
    faceInput.jawDrop = body.jawDrop()
    faceInput.jawOpen = body.jawOpen()
    faceInput.stylizedMotion = this.motion.stylizedMotion
    faceInput.sillyMouthShare = this.motion.sillyMouthShare
    const deformationChanges = this.face.prepare(faceInput, this.layers)
    const collarMotion = body.prepareCollarMotion()
    if (this.collarClip) {
      deformCollarClipMesh(this.collarClip, collarMotion, body.neckDepth)
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
    const secondaryDeformationFrame = this.body.secondaryDeformationFrame
    const context = this.layerContext
    context.anchors = this.playback.anchors
    context.current = e
    context.time = t
    context.deformationFrame = this.face.deformationFrame
    context.secondaryDeformationFrame = secondaryDeformationFrame
    context.headJellyElement = this.body.headJellyFor(e.phys)
    context.headStretch = this.body.headStretch()
    deformLayers(
      this.layers,
      context,
      deformationChanges,
      (layer) => (e.phys ? this.body.jellyFor(layer) : undefined),
      work,
    )
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
      const faceWidth = a.face.x1 - a.face.x0
      const point = this.body.headPointNow(a.face.x1 - faceWidth * 0.04, a.face.y0 + (a.face.y1 - a.face.y0) * 0.08)
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
