import type { ChestWeightField } from './chestPhysics'
import type { FrontCollarContactModel } from './collarContact'
import type { CollarClipMesh } from './collarRuntime'
import type { CropBoundary } from './cropBoundary'
import type { Anime25DLayerDeformationPlan } from './deformationDependencies'
import type { Anime25DDriver } from './driver'
import type { Anime25DExpressionDeformationBinding } from './expressionDeformation'
import type { HairRootMotion } from './hairRootMotion'
import type { HairSurface } from './hairSurface'
import type { Anime25DLayerAttachment, Anime25DNeckwearBridge } from './layerAttachment'
import type { Anime25DLayerBinding, Anime25DLayerSpringBinding } from './layerBinding'
import type { Anime25DUpstreamFeatureInput } from './layerDeformation'
import type { Anime25DLayerDeformationExtension } from './layerDeformationPolicy'
import type { Anime25DMouthDeformationKind } from './mouthDeformation'
import type { Anime25DRenderableLayer } from './renderer'
import type { Anime25DSecondaryDeformationBinding } from './secondaryDeformation'
import type { SurfaceContact } from './surfaceContact'
import type { Anime25DPlayback, Anime25DShellProfile } from './types'
import type { AtlasPixelPatch, CroppedLayerPixels } from './webglRuntime'
import { isAnime25DRigidAttachment } from '../rig/anime25dLayerSemantics'
import { removeDuplicatedNeckComponents, splitPairedEarwear } from './accessoryComponents'
import { duplicateAccessoryLayers } from './accessoryDuplicate'
import { bindArmRig, bindArmRigMesh } from './armRig'
import { sampleChestWeight } from './chestPhysics'
import { buildFrontCollarContactModel } from './collarContact'
import { createCollarClipMesh, disposeCollarClipMesh } from './collarRuntime'
import { bindCropBoundary } from './cropBoundary'
import { deriveCrownOcclusionBand } from './crownOcclusion'
import {
  createAnime25DLayerDeformationPlan,
  resolveAnime25DDeformationDependencies,
} from './deformationDependencies'
import { resolveAnime25DExpressionDeformation } from './expressionDeformation'
import { bindHairRootMotion } from './hairRootMotion'
import { bindHairSurface } from './hairSurface'
import { bindAnime25DLayerAttachment, bindNeckwearBridge } from './layerAttachment'
import { buildAnime25DLayerBinding } from './layerBinding'
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
    const armBinding = (source: Anime25DPlayback['layers'][number], rest: Float32Array) => {
      const arm = source.role === 'handwear'
        ? bindArmRig(source, readBindingPixels(source), playback.anchors, contentBottom) : null
      return { arm, armMesh: arm ? bindArmRigMesh(arm, rest) : null }
    }
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
    for (const source of renderSources) {
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
        ...armBinding(source, rest),
      })
      const layerTransform = new Float32Array(9)
      writeIdentityLayerTransform(layerTransform)
      const hairRoots = bindHairRootMotion(source, secondaryDeformation, rest, indices)
      const mesh =
        renderKind === 'neck' && collarClip
          ? null
          : createIndexedDeformableMesh(gl, program, rest, atlasUvs, indices)
      layers.push({
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
      })
    }
    const scalpFace = layers.find(layer => layer.source.role === 'face')
    const scalpHair = layers.find(layer => layer.source.role === 'back-hair')
    const fringe = layers.find(layer => layer.source.role === 'front-hair')
    const eyeTop = Math.min(playback.anchors.eyeL?.y0 ?? Infinity, playback.anchors.eyeR?.y0 ?? Infinity)
    if (scalpFace && scalpHair && fringe && layers.indexOf(scalpHair) < layers.indexOf(scalpFace)) {
      const band = deriveCrownOcclusionBand(scalpFace.source, fringe.source, scalpHair.source, eyeTop,
        readBindingPixels(scalpFace.source), readBindingPixels(fringe.source), readBindingPixels(scalpHair.source))
      if (band) scalpFace.crownOccluders = [{ layer: scalpHair, ...band }]
    }
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
    return { layers, collarClip, atlasPatches }
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
