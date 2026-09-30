import type { Anime25DRiggerAnchors } from '../anime25drig/playback'
import type { Anime25DSourceReference, RasterLayer } from './anime25dImportTypes'
import { uniquePartId } from './anime25dRaster'
import { collarBoundaryRearAmount, collarColorMaskAt, collarStandaloneRearAmount, segmentRearCollarByColor } from './collarColorSplit'
import { COLLAR_REFERENCE_FRONT, COLLAR_REFERENCE_REAR, copyRasterPixel, cropRasterPixels, rasterAlphaAt } from './collarCommon'
import { hasHighCollarEvidence, sourceReferenceAgreesWithLayers } from './collarEvidence'
import { buildCollarReferenceMask, collarReferenceMaskAt } from './collarReferenceMask'

export function splitHighCollarOcclusion(
  layers: RasterLayer[],
  anchors: Anime25DRiggerAnchors,
  sourceReference?: Anime25DSourceReference,
): RasterLayer[] {
  // Never split it again.
  if (
    layers.some(
      (layer) => layer.role === 'collar-front' || layer.role === 'collar-back',
    )
  ) {
    return layers
  }
  const candidates: Array<{ neck: RasterLayer; topwear: RasterLayer }> = []
  for (const neck of layers.filter((layer) => layer.role === 'neck')) {
    for (const topwear of layers.filter((layer) => layer.role === 'topwear')) {
      if (hasHighCollarEvidence(neck, topwear, anchors))
        candidates.push({ neck, topwear })
    }
  }
  // Several independently overlapping garments/necks cannot safely be inferred as that one topology.
  if (candidates.length !== 1) return layers
  const { neck, topwear } = candidates[0]

  const exposedTop = Math.ceil(Math.max(anchors.face.y1, neck.top))
  const exposedBottom = Math.floor(
    Math.min(anchors.neckBottom, neck.top + neck.height - 1),
  )
  const trustedSourceReference =
    sourceReference && sourceReferenceAgreesWithLayers(sourceReference, layers)
      ? sourceReference
      : undefined

  const remainingTopwear = {
    ...topwear,
    data: new Uint8ClampedArray(topwear.data),
  }
  const rearPixels = new Uint8ClampedArray(topwear.data.length)
  const frontPixels = new Uint8ClampedArray(topwear.data.length)
  const centerX = neck.left + neck.width / 2
  const halfWidth = Math.max(1, neck.width / 2)
  const exposedHeight = exposedBottom - exposedTop
  const referenceMask = trustedSourceReference
    ? buildCollarReferenceMask(
        trustedSourceReference,
        neck,
        topwear,
        exposedTop,
        exposedBottom,
      )
    : undefined
  // Keep the reference-assisted path stable.
  const colorRearMask = trustedSourceReference
    ? segmentRearCollarByColor(neck, topwear, exposedTop, exposedBottom, false)
    : undefined
  const standaloneMask = trustedSourceReference
    ? undefined
    : segmentRearCollarByColor(neck, topwear, exposedTop, exposedBottom, true)
  let minimumX = topwear.width
  let minimumY = topwear.height
  let maximumX = -1
  let maximumY = -1
  let frontMinimumX = topwear.width
  let frontMinimumY = topwear.height
  let frontMaximumX = -1
  let frontMaximumY = -1

  for (let y = exposedTop; y <= exposedBottom; y += 1) {
    for (let x = Math.ceil(neck.left); x < neck.left + neck.width; x += 1) {
      const neckAlpha = rasterAlphaAt(neck, x, y)
      if (neckAlpha === 0) continue
      const localX = x - Math.round(topwear.left)
      const localY = y - Math.round(topwear.top)
      if (
        localX < 0 ||
        localY < 0 ||
        localX >= topwear.width ||
        localY >= topwear.height
      ) {
        continue
      }
      const pixel = (localY * topwear.width + localX) * 4
      const sourceAlpha = topwear.data[pixel + 3]
      if (sourceAlpha === 0) continue

      const geometricRearAmount = collarBoundaryRearAmount(
        x,
        y,
        centerX,
        halfWidth,
        exposedTop,
        exposedHeight,
      )
      const colorRearAmount = collarColorMaskAt(colorRearMask, x, y)
      const referenceClass = collarReferenceMaskAt(referenceMask, x, y)
      const referenceRearAmount =
        referenceClass === COLLAR_REFERENCE_REAR ? 1 : 0
      const referenceFrontAmount =
        referenceClass === COLLAR_REFERENCE_FRONT ? 1 : 0
      const standaloneBlend = collarStandaloneRearAmount(standaloneMask, x, y)
      const standaloneRearAmount = standaloneBlend >= 0 ? standaloneBlend : 0
      const standaloneFrontAmount =
        standaloneBlend >= 0 ? 1 - standaloneBlend : 0
      const fallbackRearAmount =
        // Geometry partitions only a positively identified garment.
        !colorRearMask && !referenceMask && !standaloneMask
          ? geometricRearAmount
          : 0
      const fallbackFrontAmount =
        !colorRearMask && !referenceMask && !standaloneMask
          ? 1 - geometricRearAmount
          : 0
      const definitiveRearAmount = Math.max(
        colorRearAmount,
        referenceRearAmount,
      )
      const semanticRearAmount = Math.max(
        definitiveRearAmount,
        standaloneRearAmount,
        fallbackRearAmount,
      )
      const semanticFrontAmount =
        definitiveRearAmount > 0
          ? 0
          : Math.max(
              referenceFrontAmount,
              standaloneFrontAmount,
              fallbackFrontAmount,
            )
      const overlapAmount = neckAlpha / 255
      const rearAmount = semanticRearAmount * overlapAmount
      const frontAmount = semanticFrontAmount * overlapAmount
      if (rearAmount > 0) {
        copyRasterPixel(
          topwear.data,
          rearPixels,
          pixel,
          sourceAlpha * rearAmount,
        )
        minimumX = Math.min(minimumX, localX)
        minimumY = Math.min(minimumY, localY)
        maximumX = Math.max(maximumX, localX)
        maximumY = Math.max(maximumY, localY)
      }
      if (frontAmount > 0) {
        copyRasterPixel(
          topwear.data,
          frontPixels,
          pixel,
          sourceAlpha * frontAmount,
        )
        frontMinimumX = Math.min(frontMinimumX, localX)
        frontMinimumY = Math.min(frontMinimumY, localY)
        frontMaximumX = Math.max(frontMaximumX, localX)
        frontMaximumY = Math.max(frontMaximumY, localY)
      }
      remainingTopwear.data[pixel + 3] = Math.round(
        sourceAlpha * (1 - rearAmount - frontAmount),
      )
    }
  }
  if (maximumX < minimumX || maximumY < minimumY) return layers

  const rearWidth = maximumX - minimumX + 1
  const rearHeight = maximumY - minimumY + 1
  const rearData = cropRasterPixels(
    rearPixels,
    topwear.width,
    minimumX,
    minimumY,
    rearWidth,
    rearHeight,
  )
  const hasFrontCollar =
    frontMaximumX >= frontMinimumX && frontMaximumY >= frontMinimumY

  const usedIds = new Set(layers.map((layer) => layer.id))
  const rearCollar: RasterLayer = {
    id: uniquePartId('collar-back', usedIds),
    role: 'collar-back',
    sourceName: 'collar-back',
    order: topwear.order,
    side: null,
    group: 'body',
    left: topwear.left + minimumX,
    top: topwear.top + minimumY,
    width: rearWidth,
    height: rearHeight,
    data: rearData,
    synthetic: true,
  }
  const frontCollar: RasterLayer | undefined = hasFrontCollar
    ? {
        id: uniquePartId('collar-front', usedIds),
        role: 'collar-front',
        sourceName: 'collar-front',
        order: topwear.order,
        side: null,
        group: 'body',
        left: topwear.left + frontMinimumX,
        top: topwear.top + frontMinimumY,
        width: frontMaximumX - frontMinimumX + 1,
        height: frontMaximumY - frontMinimumY + 1,
        data: cropRasterPixels(
          frontPixels,
          topwear.width,
          frontMinimumX,
          frontMinimumY,
          frontMaximumX - frontMinimumX + 1,
          frontMaximumY - frontMinimumY + 1,
        ),
        synthetic: true,
      }
    : undefined

  const neckIndex = layers.indexOf(neck)
  const topwearIndex = layers.indexOf(topwear)
  // The group takes the topwear's slot. Anything painted between a lower neck
  // and the topwear (a full-length dress under a jacket) stays under the topwear.
  const insertionIndex = topwearIndex - (neckIndex < topwearIndex ? 1 : 0)
  const output = layers.filter((layer) => layer !== neck && layer !== topwear)
  // Playback paints in array order rather than consulting semantic depth.
  const withCore = output.toSpliced(
    insertionIndex,
    0,
    remainingTopwear,
    rearCollar,
    neck,
  )
  return frontCollar
    ? withCore.toSpliced(insertionIndex + 3, 0, frontCollar)
    : withCore
}
