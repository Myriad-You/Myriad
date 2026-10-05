import type { ChestWeightField } from './chestPhysics'
import type { FrontCollarContactModel } from './collarContact'
import type { CollarClipMesh } from './collarRuntime'
import type { CropBoundary } from './cropBoundary'
import type { Anime25DLayerDeformationPlan } from './deformationDependencies'
import type { Anime25DDriver } from './driver'
import type { EarwearPhysics } from './earwearPhysics'
import type { Anime25DExpressionDeformationBinding } from './expressionDeformation'
import type { HairRootMotion } from './hairRootMotion'
import type { HairSurface } from './hairSurface'
import type { HeadSilhouette } from './headTurn'
import type { Anime25DLayerAttachment, Anime25DNeckwearBridge } from './layerAttachment'
import type { Anime25DLayerBinding, Anime25DLayerSpringBinding } from './layerBinding'
import type { Anime25DUpstreamFeatureInput } from './layerDeformation'
import type { Anime25DLayerDeformationExtension } from './layerDeformationPolicy'
import type { GpuLayerBuildContext } from './layerGpuAssembly'
import type { Anime25DMouthDeformationKind } from './mouthDeformation'
import type { Anime25DRenderableLayer } from './renderer'
import type { Anime25DSecondaryDeformationBinding } from './secondaryDeformation'
import type { SurfaceContact } from './surfaceContact'
import type { AttachmentTurn } from './turnKeyforms'
import type { Anime25DPlayback, Anime25DShellProfile } from './types'
import type { AtlasPixelPatch, CroppedLayerPixels } from './webglRuntime'
import { splitPairedEarwear } from './accessoryComponents'
import { duplicateAccessoryLayers } from './accessoryDuplicate'
import { anime25DArmsTouch, anime25DHandTouchesHead, bindArmRig, bindArmRigMesh } from './armRig'
import { buildFrontCollarContactModel } from './collarContact'
import { createCollarClipMesh, disposeCollarClipMesh } from './collarRuntime'

import { headSilhouetteFromFace, headTurnFeatures } from './headTurn'
import { shadeHiddenBackHair } from './hiddenHair'
import { trimHiddenNeck } from './hiddenNeck'
import { fillHiddenSkin } from './hiddenSkin'
import { buildAnime25DLayerBinding } from './layerBinding'
import {
  bindCropBoundaries,
  bindLayerAttachments,
  bindShoulderContacts,
  buildGpuLayer,
  fuseShoulderPatch,
  liftNeckOverBody,
  linkCrownOcclusion,
  patchNeckOrnaments,
} from './layerGpuAssembly'

import {
  disposeIndexedDeformableMesh,
  readLayerPixels,
} from './webglRuntime'

export interface Anime25DGpuLayer extends Anime25DRenderableLayer {
  baseRole: string
  rest: Float32Array
  atlasUvs: Float32Array
  indices: Uint16Array
  deformed: Float32Array
  vertexBuffer: WebGLBuffer | null
  uvBuffer: WebGLBuffer | null
  indexBuffer: WebGLBuffer | null
  shaderGlobalTransform: boolean
  localDynamic: boolean
  deformationExtensions: Anime25DLayerDeformationExtension[]
  upstreamFeature: Anime25DUpstreamFeatureInput | null
  mouthDeformation: Anime25DMouthDeformationKind | null
  expressionDeformation: Anime25DExpressionDeformationBinding | null
  secondaryDeformation: Anime25DSecondaryDeformationBinding
  chestWeights: Float32Array | null
  frontHair: boolean
  frontHairParallaxScale: Float32Array | null
  strandWeights: Float32Array | null
  alongStrand: Float32Array | null
  bangWeights: Float32Array | null
  springs: Anime25DLayerSpringBinding[] | null
  hairRoots: HairRootMotion | null
  collarContact: FrontCollarContactModel | null
  deformationPlan: Anime25DLayerDeformationPlan
  geometryDirty: boolean
  attachment: Anime25DLayerAttachment | null
  /** A keyed accessory's own turn, less what its host's key already carries it at the anchor. */
  attachmentTurn?: AttachmentTurn | null
  earwearPhysics?: EarwearPhysics | null
  neckwearBridge?: Anime25DNeckwearBridge | null
  attachmentDependents?: Anime25DGpuLayer[]
  surfaceContact?: SurfaceContact
  hairSurface?: HairSurface
  cropBoundary?: CropBoundary
}

export interface Anime25DCompiledGpuLayers {
  atlasPatches?: AtlasPixelPatch[]
  layers: Anime25DGpuLayer[]
  collarClip: CollarClipMesh | null
  /** Both sleeves touch: they move as one piece and take no arm gestures. */
  armsLinked?: boolean
  /** A hand rests on the head: the head keeps near its drawn pose. */
  handTouchesHead?: boolean
  /** The face's drawn outline, which the head turns on. */
  headSilhouette?: HeadSilhouette | null
}

export function compileAnime25DGpuLayers(
  gl: WebGL2RenderingContext,
  program: WebGLProgram,
  playback: Readonly<Anime25DPlayback>,
  shellProfile: Readonly<Anime25DShellProfile>,
  current: Anime25DDriver,
  chestWeightField: ChestWeightField | null,
  atlasImage: HTMLImageElement,
): Anime25DCompiledGpuLayers {
  let layers: Anime25DGpuLayer[] = []
  let collarClip: CollarClipMesh | null = null
  try {
    const bindingPixels = new Map<
      Anime25DPlayback['layers'][number],
      CroppedLayerPixels | null
    >()
    const readBindingPixels = (source: Anime25DPlayback['layers'][number]) => {
      if (!bindingPixels.has(source))
        bindingPixels.set(source, readLayerPixels(atlasImage, source))
      return bindingPixels.get(source) ?? null
    }
    const contentBottom = Math.max(...playback.layers.map((layer) => layer.y + layer.h))
    // Arms whose hands hold each other move as one piece with the torso: no
    // per-side swing, and one shared carry so the seam between them never opens.
    const leftArm = playback.layers.find((layer) => layer.role === 'handwear' && layer.side === 'L')
    const rightArm = playback.layers.find((layer) => layer.role === 'handwear' && layer.side === 'R')
    const armsLinked = Boolean(leftArm && rightArm && anime25DArmsTouch(
      leftArm, readBindingPixels(leftArm), rightArm, readBindingPixels(rightArm)))
    const handTouchesHead = anime25DHandTouchesHead(
      playback.layers.filter((layer) => layer.role === 'handwear')
        .map((layer) => ({ layer, image: readBindingPixels(layer) })),
      playback.anchors.face,
    )
    const face = playback.layers.find((layer) => layer.role === 'face')
    const headSilhouette = face ? headSilhouetteFromFace(face, readBindingPixels(face)) : null
    const turnFeatures = headTurnFeatures(playback.layers)
    const linkedArmAnchorX = armsLinked && leftArm && rightArm
      ? (Math.min(leftArm.x, rightArm.x) + Math.max(leftArm.x + leftArm.w, rightArm.x + rightArm.w)) / 2
      : undefined
    const armBinding = (source: Anime25DPlayback['layers'][number], rest: Float32Array) => {
      const arm = source.role === 'handwear' && !armsLinked
        ? bindArmRig(source, readBindingPixels(source), playback.anchors, contentBottom) : null
      return { arm, armMesh: arm ? bindArmRigMesh(arm, rest) : null }
    }
    const atlasPatches = patchNeckOrnaments(playback, atlasImage, readBindingPixels, bindingPixels)
    const neckLayer = playback.layers.find((layer) => layer.role === 'neck')
    const neckPixels = neckLayer ? readBindingPixels(neckLayer) : null
    const facePixels = face ? readBindingPixels(face) : null
    const trimmedNeck = neckLayer && neckPixels && face && facePixels
      ? trimHiddenNeck(neckLayer, neckPixels, face, facePixels)
      : null
    if (neckLayer && trimmedNeck) {
      bindingPixels.set(neckLayer, trimmedNeck)
      atlasPatches.push({
        ...trimmedNeck,
        x: Math.round(neckLayer.atlas.x * atlasImage.width),
        y: Math.round(neckLayer.atlas.y * atlasImage.height),
      })
    }
    const hairCovers = playback.layers
      .filter((layer) => layer.group === 'head' && (layer.role === 'front-hair' || layer.role === 'headwear'))
      .flatMap((layer) => {
        const image = readBindingPixels(layer)
        return image ? [{ layer, image }] : []
      })
    const backHair = playback.layers.find((layer) => layer.group === 'head' && layer.role === 'back-hair')
    const backHairPixels = backHair ? readBindingPixels(backHair) : null
    const headCovers = playback.layers
      .filter((layer) => layer.role === 'face' || layer.role === 'ears' || layer.role === 'neck')
      .flatMap((layer) => {
        const image = readBindingPixels(layer)
        return image ? [{ layer, image }] : []
      })
    // A keyed head carries the material its turn uncovers, drawn; the generic fills stand in without it.
    const drawnMaterial = Boolean(playback.turnKeyforms)
    const shadedHair = !drawnMaterial && backHair && backHairPixels ? shadeHiddenBackHair(backHair, backHairPixels, headCovers) : null
    if (backHair && shadedHair) {
      bindingPixels.set(backHair, shadedHair)
      atlasPatches.push({
        ...shadedHair,
        x: Math.round(backHair.atlas.x * atlasImage.width),
        y: Math.round(backHair.atlas.y * atlasImage.height),
      })
    }
    const plainSkin = !drawnMaterial && face && facePixels ? fillHiddenSkin(face, facePixels, hairCovers) : null
    if (face && plainSkin) {
      bindingPixels.set(face, plainSkin)
      atlasPatches.push({
        ...plainSkin,
        x: Math.round(face.atlas.x * atlasImage.width),
        y: Math.round(face.atlas.y * atlasImage.height),
      })
    }
    const duplicateAccessories = duplicateAccessoryLayers(
      playback.layers,
      readBindingPixels,
    )
    const renderSources = playback.layers.flatMap((source, index) => {
      if (duplicateAccessories.has(source)) return []
      const parts = source.role === 'earwear'
        ? splitPairedEarwear(source, readBindingPixels(source), playback.anchors.face.cx) : null
      return parts?.map(part => ({ ...part, z: source.z ?? index })) ?? [source]
    })
    const torsos = playback.layers.filter((layer) => layer.role === 'topwear')
    const torso = torsos.length === 1 ? torsos[0] : null
    const torsoPixels =
      torso && playback.layers.some((layer) => layer.role === 'handwear')
        ? readBindingPixels(torso)
        : null
    const neck = playback.layers.find((layer) => layer.role === 'neck')
    const collarContacts = new Map<
      Anime25DPlayback['layers'][number],
      FrontCollarContactModel
    >()
    for (const source of playback.layers) {
      if (source.role !== 'collar-front') continue
      const cropped = readLayerPixels(atlasImage, source)
      if (!cropped?.pixels) continue
      const contact = buildFrontCollarContactModel(
        cropped.pixels,
        cropped.width,
        cropped.height,
        source,
        playback.anchors.neckPivot.x,
      )
      if (contact) collarContacts.set(source, contact)
    }
    const clipCollar = collarContacts.entries().next().value as
      [Anime25DPlayback['layers'][number], FrontCollarContactModel] | undefined
    if (neck && clipCollar) {
      collarClip = createCollarClipMesh(
        gl,
        program,
        clipCollar[1],
        neck,
        clipCollar[0],
      )
    }

    const bindings = new Map<Anime25DPlayback['layers'][number], Anime25DLayerBinding>()
    const bindingFor = (source: Anime25DPlayback['layers'][number]) => {
      const cached = bindings.get(source)
      if (cached) return cached
      const collarContact = collarContacts.get(source) ?? null
      const binding = buildAnime25DLayerBinding({
        source,
        canvasWidth: playback.pixelCanvas.width,
        face: playback.anchors.face,
        layerZ:
          typeof source.z === 'number' && Number.isFinite(source.z)
            ? source.z
            : playback.layers.indexOf(source),
        extraGridX: collarContact?.gridX,
        extraGridY: collarContact?.gridY,
        faceShell: shellProfile.enabled && shellProfile.blend > 0 ? shellProfile : undefined,
      })
      bindings.set(source, binding)
      return binding
    }
    const context: GpuLayerBuildContext = {
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
    }
    for (const source of renderSources) layers.push(buildGpuLayer(source, context))
    linkCrownOcclusion(layers, playback, readBindingPixels)
    layers = liftNeckOverBody(layers, playback, readBindingPixels)
    bindShoulderContacts(layers, torso, torsoPixels, readBindingPixels)
    bindLayerAttachments(layers, collarClip, playback, chestWeightField, readBindingPixels)
    fuseShoulderPatch(layers, torso, torsoPixels, readBindingPixels, atlasImage, atlasPatches)
    bindCropBoundaries(layers, torso, torsoPixels, readBindingPixels)
    return { layers, collarClip, atlasPatches, armsLinked, handTouchesHead, headSilhouette }
  } catch (error) {
    disposeAnime25DGpuLayers(gl, { layers, collarClip })
    throw error
  }
}

export function disposeAnime25DGpuLayers(
  gl: WebGL2RenderingContext,
  compiled: Readonly<Anime25DCompiledGpuLayers>,
): void {
  for (const layer of compiled.layers) {
    disposeIndexedDeformableMesh(gl, {
      vao: layer.vao,
      positionBuffer: layer.vertexBuffer,
      uvBuffer: layer.uvBuffer,
      indexBuffer: layer.indexBuffer,
    })
  }
  if (compiled.collarClip) disposeCollarClipMesh(gl, compiled.collarClip)
}
