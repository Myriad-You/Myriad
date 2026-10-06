import type { Anime25DDriver } from './driver'
import type { Anime25DExpressionDeformationFrame } from './expressionDeformation'
import type { Anime25DIrisRebound } from './irisRebound'
import type { JellyElement } from './jellyVolume'
import type { Anime25DGpuLayer } from './layerGpuBinding'
import type { Anime25DMouthDeformationFrame } from './mouthDeformation'
import type { Anime25DFrameWork } from './performanceTelemetry'
import type { Anime25DSecondaryDeformationFrame } from './secondaryDeformation'
import type { Anime25DPlaybackAnchors } from './types'
import { BODY_HEAD_FOLLOW } from './collarRuntime'
import { applyCropBoundary } from './cropBoundary'
import { cryTearHorizontalOffset, cryTearVerticalOffset } from './cryMotion'
import { markAnime25DLayerGeometryUpdated, shouldUpdateAnime25DLayerGeometry } from './deformationDependencies'
import { deformAnime25DExpressionPoint } from './expressionDeformation'
import { constrainHairSurface } from './hairSurface'
import { jellyDisplacement } from './jellyVolume'
import { deformNeckwearBridge, writeAnime25DAttachmentTransform } from './layerAttachment'
import { deformAnime25DUpstreamFeaturePoint } from './layerDeformation'
import { writeAnime25DLayerGlobalTransform } from './layerTransform'
import {
  deformAnime25DFaceJawPoint,
  deformAnime25DMouthPoint,
} from './mouthDeformation'
import { shouldDeformLayer } from './mouthRuntime'
import { writePoseCorrectionWeights } from './poseCorrections'
import { bodyLeanShare } from './poseScale'
import {
  deformAnime25DHairPoint,
  deformAnime25DSecondaryPoint,
} from './secondaryDeformation'
import { applySurfaceContact } from './surfaceContact'
import { deformAttachmentTurn, hostKeyRoll } from './turnKeyforms'

/** A soft volume a layer's vertices sway and squash with. */
export interface LayerJelly {
  element: JellyElement
  volume: { readonly stretch: number; readonly sway: number }
}

/** One frame's shared pose, read by every layer's deformation. */
export interface LayerDeformationContext {
  anchors: Anime25DPlaybackAnchors
  current: Anime25DDriver
  time: number
  deformationFrame: Anime25DMouthDeformationFrame & Anime25DExpressionDeformationFrame
  secondaryDeformationFrame: Anime25DSecondaryDeformationFrame
  irisRebound: Anime25DIrisRebound
  /** The head's soft volume, while physics runs. */
  headJellyElement: JellyElement | null
  headStretch: number
  /** Scratch space, reused so the per-vertex loop never allocates. */
  deformationPoint: { x: number; y: number }
  jellyShift: { x: number; y: number }
}

/**
 * A rigid part's whole-layer carry: head tilt, breath and turn, applied by
 * the shader instead of per vertex.
 */
export function writeRigidLayerTransform(
  layer: Anime25DGpuLayer,
  context: LayerDeformationContext,
): void {
  const A = context.anchors
  const e = context.current
  const secondaryDeformationFrame = context.secondaryDeformationFrame
  const { headJellyElement, headStretch } = context
  const source = layer.source
  const isHead = source.group === 'head'
  // A rigid body part takes the head tilt its centre would under the bend.
  const carriedRoll = isHead ? 1 : bodyLeanShare(source.y + source.h / 2,
    A.bodyPivot.y, secondaryDeformationFrame.bodyBendHeight ?? 0)
  const roll = (secondaryDeformationFrame.headRoll ?? 0) * carriedRoll
  writeAnime25DLayerGlobalTransform(
    {
      headFollow: isHead
        ? 1
        : source.group === 'body'
          ? BODY_HEAD_FOLLOW
          : 0,
      headRotationCosine: carriedRoll === 1 ? secondaryDeformationFrame.headRotationCosine : Math.cos(roll),
      headRotationSine: carriedRoll === 1 ? secondaryDeformationFrame.headRotationSine : Math.sin(roll),
      neckPivotX: A.neckPivot.x,
      neckPivotY: A.neckPivot.y,
      faceScale: A.faceScale,
      angleX: e.angleX,
      angleY: e.angleY,
      depthOffset: source.depth - 1,
      faceCenterY: A.face.cy,
      specialOffsetY: isHead ? secondaryDeformationFrame.specialHeadOffset : 0,
      breathOffset: isHead
        ? secondaryDeformationFrame.headBreathOffset
        : secondaryDeformationFrame.bodyBreathOffset,
    },
    layer.layerTransform,
  )
  if (isHead) layer.layerTransform[6] += secondaryDeformationFrame.torsoNeckOffsetX
  if (isHead && headJellyElement && headStretch !== 0) {
    // A small rigid feature rides the squashed head at its own centre.
    const m = layer.layerTransform
    const shift = jellyDisplacement(source.x + source.w / 2, source.y + source.h / 2,
      headJellyElement, headStretch, 0, context.jellyShift)
    m[6] += m[0] * shift.x + m[3] * shift.y
    m[7] += m[1] * shift.x + m[4] * shift.y
  }
}

/**
 * Run every vertex of one layer through the deformation chain: face features,
 * expression, mouth, jaw, soft volumes, then body, shell and hair.
 * Returns whether any vertex moved at GPU precision.
 */
export function deformLayerVertices(
  layer: Anime25DGpuLayer,
  context: LayerDeformationContext,
  jellyPart: LayerJelly | undefined,
): boolean {
  const e = context.current
  const t = context.time
  const fs = context.anchors.faceScale
  const { deformationFrame, deformationPoint, secondaryDeformationFrame } = context
  const { headJellyElement, headStretch } = context
  const rest = layer.rest
  const deformed = layer.surfaceContact?.unconstrained ?? layer.hairSurface?.candidate ?? layer.deformed
  const vertexCount = rest.length / 2
  const source = layer.source
  const bn = layer.baseRole
  const isHead = source.group === 'head'
  const upstreamFeature = layer.upstreamFeature
  const cryLayer = source.fade === 'eyeCry'
  const tearVertical = cryLayer
    ? cryTearVerticalOffset(t, source.side, e.eyeCry, fs)
    : 0
  const tearHorizontal = cryLayer
    ? cryTearHorizontalOffset(t, source.side, e.eyeCry, fs)
    : 0
  const mouthDeformation = layer.mouthDeformation
  if (layer.secondaryDeformation.poseCorrections) {
    writePoseCorrectionWeights(layer.secondaryDeformation.poseCorrections, e)
  }
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
        context.irisRebound,
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
    if (isHead && headJellyElement && headStretch !== 0) {
      const shift = jellyDisplacement(rest[index], rest[index + 1], headJellyElement, headStretch, 0, context.jellyShift)
      x += shift.x
      y += shift.y
    }
    if (jellyPart) {
      const shift = jellyDisplacement(rest[index], rest[index + 1], jellyPart.element,
        jellyPart.volume.stretch, jellyPart.volume.sway, context.jellyShift)
      x += shift.x
      y += shift.y
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
      rest[index],
    )
    // Compare in the GPU buffer's precision. Comparing a double to last
    // frame's float marks an identical pose dirty forever.
    x = Math.fround(deformationPoint.x)
    y = Math.fround(deformationPoint.y)
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
  return geometryChanged
}

/**
 * What can only be settled once every host layer has moved: surface
 * contact, crop boundaries, and attachments riding their hosts.
 * Returns the upload bytes the contact pass found unchanged.
 */
export function settleDependentLayers(
  layers: readonly Anime25DGpuLayer[],
  secondaryDeformationFrame: Anime25DSecondaryDeformationFrame,
  time: number,
  current: Anime25DDriver,
): number {
  let savedUploadBytes = 0
  for (const layer of layers) {
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
    if (!layer.geometryDirty) savedUploadBytes += layer.deformed.byteLength
  }
  for (const layer of layers) {
    if (layer.cropBoundary && shouldDeformLayer(layer.source, layer.frameOpacity)) {
      layer.geometryDirty = applyCropBoundary(layer.cropBoundary, layer.deformed) || layer.geometryDirty
    }
  }
  // Hosts may be later in draw order. Resolve attachments only after all host
  // vertices include this frame's shell, breathing and hair physics.
  for (const layer of layers) {
    if (
      layer.neckwearBridge &&
      shouldDeformLayer(layer.source, layer.frameOpacity)
    ) {
      layer.geometryDirty = deformNeckwearBridge(
        layer.neckwearBridge,
        secondaryDeformationFrame,
        layer.rest,
        layer.deformed,
        layer.attachmentTurn,
      )
      layer.layerTransform.fill(0)
      layer.layerTransform[0] =
        layer.layerTransform[4] =
        layer.layerTransform[8] =
          1
    } else if (layer.attachment) {
      writeAnime25DAttachmentTransform(
        layer.attachment,
        secondaryDeformationFrame,
        layer.layerTransform,
      )
      layer.earwearPhysics?.apply(layer.attachment, layer.layerTransform, time, current.angleX, current.angleY, current.phys,
        layer.attachmentTurn ? hostKeyRoll(layer.attachmentTurn, secondaryDeformationFrame.headTurn?.amount ?? 0, secondaryDeformationFrame.headTurn?.nodAmount ?? 0) : null)
      if (layer.attachmentTurn && deformAttachmentTurn(layer.attachmentTurn, secondaryDeformationFrame.headTurn?.amount ?? 0, secondaryDeformationFrame.headTurn?.nodAmount ?? 0, layer.rest, layer.deformed))
        layer.geometryDirty = true
    }
  }
  return savedUploadBytes
}

/**
 * Every visible layer this frame: its rigid carry, then its vertices when
 * they can have moved. Counts the work when the frame is sampled.
 */
export function deformLayers(
  layers: readonly Anime25DGpuLayer[],
  context: LayerDeformationContext,
  /** What changed since last frame (`captureAnime25DDeformationChanges`). */
  deformationChanges: number,
  jellyFor: (layer: Anime25DGpuLayer) => LayerJelly | undefined,
  work: Anime25DFrameWork | undefined,
): void {
  for (const layer of layers) {
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
      jellyFor(layer),
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
}
