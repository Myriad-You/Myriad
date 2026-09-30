import type { bindArmRig, bindArmRigMesh } from './armRig'
import type { ChestWeightField } from './chestPhysics'
import type { FrontCollarContactModel } from './collarContact'
import type { CollarClipMesh } from './collarRuntime'
import type { Anime25DDriver } from './driver'
import type { headTurnFeatures } from './headTurn'
import type { Anime25DLayerBinding } from './layerBinding'
import type { Anime25DGpuLayer } from './layerGpuBinding'
import type { Anime25DPlayback, Anime25DShellProfile } from './types'
import type { AtlasPixelPatch, CroppedLayerPixels } from './webglRuntime'
import { isAnime25DRigidAttachment } from '../rig/anime25dLayerSemantics'
import { removeDuplicatedNeckComponents } from './accessoryComponents'
import { sampleChestWeight } from './chestPhysics'
import { bindCropBoundary } from './cropBoundary'
import { deriveCrownOcclusionBand } from './crownOcclusion'
import {
  createAnime25DLayerDeformationPlan,
  resolveAnime25DDeformationDependencies,
} from './deformationDependencies'
import { bindEarwearPhysics } from './earwearPhysics'
import { resolveAnime25DExpressionDeformation } from './expressionDeformation'
import { bindHairRootMotion } from './hairRootMotion'
import { bindHairSurface } from './hairSurface'
import { bindAnime25DLayerAttachment, bindNeckwearBridge } from './layerAttachment'
import { bindAnime25DUpstreamFeature } from './layerDeformation'
import { resolveAnime25DLayerDeformationPolicy } from './layerDeformationPolicy'
import { writeIdentityLayerTransform } from './layerTransform'
import { resolveAnime25DMouthDeformation } from './mouthDeformation'
import { resolveAnime25DNeckSurface } from './neckSurface'
import { canLiftNeckwearOverSkin } from './neckwearOcclusion'
import { bindPoseCorrections } from './poseCorrections'
import { createAnime25DSecondaryDeformationBinding } from './secondaryDeformation'
import {
  anime25DShellModeForLayer,
  sampleAnime25DHairlinePinWeights,
} from './shellDeformation'
import { shoulderContactWeights } from './shoulderContact'
import { fuseShoulderSurface } from './shoulderSurface'
import { bindSurfaceContact } from './surfaceContact'
import { buildContactSurfaceMesh } from './surfaceMesh'
import { anime25DTorsoShellModeForLayer } from './torsoDeformation'
import {
  createIndexedDeformableMesh,
} from './webglRuntime'

type PlaybackLayer = Anime25DPlayback['layers'][number]
type ReadPixels = (source: PlaybackLayer) => CroppedLayerPixels | null

/** What building one layer's GPU binding reads, shared by every layer. */
export interface GpuLayerBuildContext {
  gl: WebGL2RenderingContext
  program: WebGLProgram
  playback: Readonly<Anime25DPlayback>
  shellProfile: Readonly<Anime25DShellProfile>
  current: Anime25DDriver
  chestWeightField: ChestWeightField | null
  readBindingPixels: ReadPixels
  collarContacts: ReadonlyMap<PlaybackLayer, FrontCollarContactModel>
  collarClip: CollarClipMesh | null
  torso: PlaybackLayer | null
  torsoPixels: CroppedLayerPixels | null
  linkedArmAnchorX: number | undefined
  turnFeatures: ReturnType<typeof headTurnFeatures>
  bindingFor: (source: PlaybackLayer) => Anime25DLayerBinding
  armBinding: (source: PlaybackLayer, rest: Float32Array) => {
    arm: ReturnType<typeof bindArmRig> | null
    armMesh: ReturnType<typeof bindArmRigMesh> | null
  }
}

/**
 * Neck ornaments drawn twice (once on the headwear, once on their own layer):
 * the headwear copy is erased in the atlas, and its binding pixels replaced.
 */
export function patchNeckOrnaments(
  playback: Readonly<Anime25DPlayback>,
  atlasImage: HTMLImageElement,
  readBindingPixels: ReadPixels,
  bindingPixels: Map<PlaybackLayer, CroppedLayerPixels | null>,
): AtlasPixelPatch[] {
  const atlasPatches: AtlasPixelPatch[] = []
  const neckOrnaments = playback.layers.filter((l) => l.role === 'neckwear')
  if (neckOrnaments.length) {
    for (const source of playback.layers.filter(
      (l) => l.role === 'headwear',
    )) {
      if (
        !neckOrnaments.some(
          (l) =>
            source.x < l.x + l.w &&
            source.x + source.w > l.x &&
            source.y < l.y + l.h &&
            source.y + source.h > l.y,
        )
      ) {
        continue
}
      const art = readBindingPixels(source)
      if (!art) continue
      const targets = neckOrnaments.flatMap((layer) => {
        const image = readBindingPixels(layer)
        return image ? [{ layer, image }] : []
      })
      const patch = removeDuplicatedNeckComponents(
        source,
        art,
        targets,
        playback.anchors.neckTop,
      )
      if (patch) {
        bindingPixels.set(source, patch)
        atlasPatches.push({
          ...patch,
          x: Math.round(source.atlas.x * atlasImage.width),
          y: Math.round(source.atlas.y * atlasImage.height),
        })
      }
    }
}
  return atlasPatches
}

/** One playback layer as the GPU draws and deforms it. */
export function buildGpuLayer(source: PlaybackLayer, context: GpuLayerBuildContext): Anime25DGpuLayer {
  const {
    gl,
    program,
    playback,
    shellProfile,
    current,
    chestWeightField,
    readBindingPixels,
    collarContacts,
    collarClip,
    torso,
    torsoPixels,
    linkedArmAnchorX,
    turnFeatures,
    bindingFor,
    armBinding,
  } = context
  const collarContact = collarContacts.get(source) ?? null
  const binding = bindingFor(source)
  const {
    rest: gridRest,
    atlasUvs: gridUvs,
    indices: gridIndices,
    cols: _cols,
    rows,
    extensions: _extensions,
    ...hair
  } = binding
  const hasShoulderContact = source.role === 'handwear' && source.phys !== 'hair' && torso && torsoPixels &&
    shoulderContactWeights(source, readBindingPixels(source), torso, torsoPixels, gridRest)
  const surfaceMesh = hasShoulderContact
    ? buildContactSurfaceMesh(bindingFor(torso!), source)
    : null
  const { rest, atlasUvs, indices } = surfaceMesh ?? { rest: gridRest, atlasUvs: gridUvs, indices: gridIndices }
  const chestWeights =
    source.role === 'topwear' && chestWeightField
      ? samplePlaybackChestWeights(
          chestWeightField,
          rest,
          playback.pixelCanvas.width,
        )
      : null
  const baseRole = anime25DLayerBaseName(source.role)
  const renderKind = anime25DRenderKind(source)
  const shellMode = anime25DShellModeForLayer(source)
  const torsoShellMode = anime25DTorsoShellModeForLayer(source)
  const hairlinePinWeights =
    shellMode === 'front-hair' && source.role === 'front-hair'
      ? sampleAnime25DHairlinePinWeights(rest, source, rows, shellProfile)
      : null
  const deformationPolicy = resolveAnime25DLayerDeformationPolicy({
    rigidAttachment: isAnime25DRigidAttachment(source),
    baseRole,
    fade: source.fade,
    hairPhysics: source.phys === 'hair',
    hasBangWeights: Boolean(hair.bangWeights),
    hasFrontHairParallax: Boolean(hair.frontHairParallaxScale),
    hasCollarContact: Boolean(collarContact),
    shellDeformation: Boolean(
      shellMode && shellProfile.enabled && shellProfile.blend > 0,
    ),
    torsoShellDeformation: Boolean(
      torsoShellMode &&
      shellProfile.enabled &&
      shellProfile.blend > 0 &&
      shellProfile.torso.enabled &&
      shellProfile.torso.blend > 0,
    ),
  })
  const eye =
    source.side === 'L'
      ? playback.anchors.eyeL
      : source.side === 'R'
        ? playback.anchors.eyeR
        : undefined
  const expressionDeformationKind = resolveAnime25DExpressionDeformation(
    source,
    Boolean(eye),
  )
  const expressionDeformation = expressionDeformationKind
    ? {
        kind: expressionDeformationKind,
        source,
        eye,
        centerX: source.x + source.w / 2,
        centerY: source.y + source.h / 2,
      }
    : null
  const upstreamFeature = bindAnime25DUpstreamFeature(
    source,
    eye,
    playback.anchors.faceScale,
    current,
  )
  const mouthDeformation = resolveAnime25DMouthDeformation(source.fade)
  const deformationPlan = createAnime25DLayerDeformationPlan(
    deformationPolicy.shaderGlobalTransform &&
      deformationPolicy.localDynamic,
    resolveAnime25DDeformationDependencies({
      baseRole,
      fade: source.fade,
      shaderGlobalTransform: deformationPolicy.shaderGlobalTransform,
      localDynamic: deformationPolicy.localDynamic,
      upstreamFeatureKind: upstreamFeature?.kind ?? null,
      expressionDeformationKind,
      mouthDeformationKind: mouthDeformation,
    }),
  )
  const secondaryDeformation = createAnime25DSecondaryDeformationBinding({
    poseCorrections: isAnime25DRigidAttachment(source) ? undefined :
      bindPoseCorrections(shellProfile.poseCorrections, shellMode, rest, shellProfile.head),
    source,
    baseRole,
    shaderGlobalTransform: deformationPolicy.shaderGlobalTransform,
    collarContact: Boolean(collarContact),
    frontHair: hair.frontHair,
    frontHairParallaxScale: hair.frontHairParallaxScale,
    chestWeights,
    bangWeights: hair.bangWeights,
    strandWeights: hair.strandWeights,
    alongStrand: hair.alongStrand,
    springs: hair.springs,
    shellMode,
    hairlinePinWeights,
    torsoShellMode,
    handwearAnchorX: linkedArmAnchorX,
    turnFeature: turnFeatures.get(source) ?? null,
    ...armBinding(source, rest),
  })
  const layerTransform = new Float32Array(9)
  writeIdentityLayerTransform(layerTransform)
  const hairRoots = bindHairRootMotion(source, secondaryDeformation, rest, indices)
  const mesh =
    renderKind === 'neck' && collarClip
      ? null
      : createIndexedDeformableMesh(gl, program, rest, atlasUvs, indices)
  return {
    source,
    baseRole,
    rest,
    deformed: deformationPolicy.localDynamic ? rest.slice() : rest,
    hairSurface: source.phys === 'hair' && ['front-hair', 'back-hair'].includes(source.role) && hair.springs && hair.alongStrand
      ? bindHairSurface(rest, indices, hair.alongStrand, hairlinePinWeights)
      : undefined,
    atlasUvs,
    indices,
    vao: mesh?.vao ?? null,
    vertexBuffer: mesh?.positionBuffer ?? null,
    uvBuffer: mesh?.uvBuffer ?? null,
    indexBuffer: mesh?.indexBuffer ?? null,
    indexCount: mesh ? indices.length : 0,
    layerTransform,
    ...deformationPolicy,
    upstreamFeature,
    mouthDeformation,
    expressionDeformation,
    secondaryDeformation,
    hairRoots,
    frameOpacity: source.fade ? 0 : 1,
    renderKind,
    retainWhenHidden: source.name.startsWith('eyewhite'),
    cryDirection:
      source.fade === 'eyeCry' ? (source.side === 'L' ? -1 : 1) : 0,
    chestWeights,
    ...hair,
    collarContact,
    deformationPlan,
    geometryDirty: false,
    attachment: null,
  }
}

/** The back hair covering the crown shows through the face's top edge. */
export function linkCrownOcclusion(layers: Anime25DGpuLayer[], playback: Readonly<Anime25DPlayback>, readBindingPixels: ReadPixels): void {
  const scalpFace = layers.find(layer => layer.source.role === 'face')
  const scalpHair = layers.find(layer => layer.source.role === 'back-hair')
  const fringe = layers.find(layer => layer.source.role === 'front-hair')
  const eyeTop = Math.min(playback.anchors.eyeL?.y0 ?? Infinity, playback.anchors.eyeR?.y0 ?? Infinity)
  if (scalpFace && scalpHair && fringe && layers.indexOf(scalpHair) < layers.indexOf(scalpFace)) {
    const band = deriveCrownOcclusionBand(scalpFace.source, fringe.source, scalpHair.source, eyeTop,
      readBindingPixels(scalpFace.source), readBindingPixels(fringe.source), readBindingPixels(scalpHair.source))
    if (band) scalpFace.crownOccluders = [{ layer: scalpHair, ...band }]
  }
}

/** The neck drawn over the body, with neckwear lifted over both skins. */
export function liftNeckOverBody(
  initial: Anime25DGpuLayer[],
  playback: Readonly<Anime25DPlayback>,
  readBindingPixels: ReadPixels,
): Anime25DGpuLayer[] {
  let layers = initial
  const neckSurface = resolveAnime25DNeckSurface(
    playback.layers,
    playback.anchors,
    readBindingPixels,
  )
  if (neckSurface) {
    const neckIndex = layers.findIndex(
      (layer) => layer.source === neckSurface.neck,
    )
    const bodyIndex = layers.findIndex(
      (layer) => layer.source === neckSurface.body,
    )
    const neckLayer = layers[neckIndex]
    neckLayer.neckSurfaceFade = {
      start: neckSurface.fadeStart,
      end: neckSurface.fadeEnd,
      contour: neckSurface.contour,
    }
    if (neckIndex < bodyIndex) {
      layers = layers.toSpliced(neckIndex, 1)
      layers = layers.toSpliced(bodyIndex, 0, neckLayer)
    }
    // Moving only the neck would still bury that independent drawing under both skin surfaces.
    const recoveredNeckIndex = layers.indexOf(neckLayer)
    const accessories = layers.filter(
      (layer, index) =>
        index < recoveredNeckIndex &&
        layer.source.role === 'neckwear' &&
        layer.source.x < neckSurface.neck.x + neckSurface.neck.w &&
        layer.source.x + layer.source.w > neckSurface.neck.x &&
        layer.source.y < neckSurface.neck.y + neckSurface.neck.h &&
        layer.source.y + layer.source.h > neckSurface.neck.y &&
        canLiftNeckwearOverSkin(
          layer.source,
          neckSurface.neck,
          neckSurface.body,
          layers
            .slice(index + 1, recoveredNeckIndex)
            .map((entry) => entry.source),
          readBindingPixels,
        ),
    )
    for (const accessory of accessories) {
      const index = layers.indexOf(accessory)
      if (index >= 0) layers = layers.toSpliced(index, 1)
    }
    layers = layers.toSpliced(
      layers.indexOf(neckLayer) + 1,
      0,
      ...accessories,
    )
  }
  return layers
}

/** Sleeves resting on the torso keep to its surface. */
export function bindShoulderContacts(
  layers: Anime25DGpuLayer[],
  torso: PlaybackLayer | null,
  torsoPixels: CroppedLayerPixels | null,
  readBindingPixels: ReadPixels,
): void {
  const torsoLayer = layers.find((layer) => layer.source === torso)
  if (torsoLayer && torso && torsoPixels) {
    for (const layer of layers) {
      if (layer.source.role !== 'handwear') continue
      const weights = shoulderContactWeights(
        layer.source, readBindingPixels(layer.source), torso, torsoPixels, layer.rest,
      )
      if (!weights) continue
      layer.surfaceContact = bindSurfaceContact(
        {
          rest: torsoLayer.rest,
          deformed: torsoLayer.deformed,
          indices: torsoLayer.indices,
          transform: torsoLayer.layerTransform,
        },
        layer.rest,
        weights,
      )
      ;(torsoLayer.attachmentDependents ??= []).push(layer)
    }
  }
}

/** Accessories ride their hosts; hosts learn who depends on them. */
export function bindLayerAttachments(
  layers: Anime25DGpuLayer[],
  collarClip: CollarClipMesh | null,
  playback: Readonly<Anime25DPlayback>,
  chestWeightField: ChestWeightField | null,
  readBindingPixels: ReadPixels,
): void {
  // High-collar necks are rendered by the aperture mesh, not the retired
  // rectangular neck grid. Attachments must sample that same visible surface.
  const attachmentHosts = layers.map((layer) =>
    layer.renderKind === 'neck' && collarClip
      ? {
          ...layer,
          rest: collarClip.rest,
          deformed: collarClip.deformed,
          indices: collarClip.indices,
        }
      : layer,
  )
  for (const layer of layers) {
    layer.neckwearBridge = bindNeckwearBridge(
      layer.source,
      attachmentHosts,
      playback.anchors,
      chestWeightField,
      playback.pixelCanvas.width,
      layer.rest,
      readBindingPixels,
    )
    if (layer.neckwearBridge) layer.deformed = layer.rest.slice()
    layer.attachment = bindAnime25DLayerAttachment(
      layer.source,
      attachmentHosts,
      playback.anchors,
      chestWeightField,
      playback.pixelCanvas.width,
      readBindingPixels,
    )
    if (layer.source.role === 'earwear') {
      layer.earwearPhysics = bindEarwearPhysics(layer.source, layer.attachment,
        readBindingPixels(layer.source), playback.anchors.face.y1 - playback.anchors.face.y0)
    }
  }
  for (const child of layers) {
    const sources = new Set([
      child.attachment?.hostSource,
      child.neckwearBridge?.upper.hostSource,
      child.neckwearBridge?.lower.hostSource,
    ])
    for (const parent of layers) {
      if (sources.has(parent.source))
        (parent.attachmentDependents ??= []).push(child)
    }
  }
}

/** The torso's shoulders repainted where sleeves meet it. */
export function fuseShoulderPatch(
  layers: Anime25DGpuLayer[],
  torso: PlaybackLayer | null,
  torsoPixels: CroppedLayerPixels | null,
  readBindingPixels: ReadPixels,
  atlasImage: HTMLImageElement,
  atlasPatches: AtlasPixelPatch[],
): void {
  if (torso && torsoPixels) {
    const arms = layers
      .filter((layer) => layer.surfaceContact)
      .flatMap((layer) => {
        const image = readBindingPixels(layer.source)
        return image ? [{ layer: layer.source, image }] : []
      })
    const patch = fuseShoulderSurface(torso, torsoPixels, arms)
    if (patch) {
      atlasPatches.push({
        ...patch,
        x: Math.round(torso.atlas.x * atlasImage.width),
        y: Math.round(torso.atlas.y * atlasImage.height),
      })
}
  }
}

/** Hair and sleeves end where the torso is cropped by the frame. */
export function bindCropBoundaries(
  layers: Anime25DGpuLayer[],
  torso: PlaybackLayer | null,
  torsoPixels: CroppedLayerPixels | null,
  readBindingPixels: ReadPixels,
): void {
  const cropHost = layers.find((layer) => layer.source === torso)
  if (cropHost && !cropHost.shaderGlobalTransform) {
    const bottom = Math.max(...layers.map((layer) => layer.source.y + layer.source.h))
    for (const layer of layers) {
      if (!layer.localDynamic || layer.shaderGlobalTransform || layer.attachment || layer.neckwearBridge) continue
      if (!['back-hair', 'front-hair', 'handwear'].includes(layer.source.role)) continue
      const binding = bindCropBoundary(layer, cropHost, readBindingPixels(layer.source), torsoPixels, bottom)
      if (!binding) continue
      layer.cropBoundary = binding
      // A new correction always starts from primary geometry, never last frame's
      // corrected vertices. Keep its host fresh even when hidden by an outfit.
      layer.deformationPlan.cacheable = false
      ;(cropHost.attachmentDependents ??= []).push(layer)
    }
  }
}

function samplePlaybackChestWeights(
  field: ChestWeightField,
  rest: Float32Array,
  frameWidth: number,
): Float32Array {
  const scale = Math.max(1, frameWidth)
  const weights = new Float32Array(rest.length / 2)
  for (let vertex = 0; vertex < weights.length; vertex += 1) {
    weights[vertex] = sampleChestWeight(
      field,
      rest[vertex * 2] / scale,
      rest[vertex * 2 + 1] / scale,
    )
  }
  return weights
}

export function anime25DLayerBaseName(role: string): string {
  if (role === 'front-hair') return 'front hair'
  if (role === 'back-hair') return 'back hair'
  return role.replaceAll('-', '_')
}

function anime25DRenderKind(
  source: Anime25DPlayback['layers'][number],
): Anime25DGpuLayer['renderKind'] {
  if (source.role === 'neck') return 'neck'
  if (source.name.startsWith('eyewhite') || source.role === 'eye-silly-white') {
    return 'eyewhite'
  }
  if (
    source.name.startsWith('irides') ||
    source.role === 'iris-silly' ||
    source.role === 'lovestruck-heart'
  ) {
    return 'iris'
  }
  return 'ordinary'
}
