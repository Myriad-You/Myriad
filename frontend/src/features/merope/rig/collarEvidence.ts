import type { Anime25DRiggerAnchors } from '../anime25drig/playback'
import type { Anime25DSourceReference, RasterLayer } from './anime25dImportTypes'
import { ALPHA_COMPONENT_THRESHOLD, HIGH_COLLAR_UPPER_FRACTION, perceptualColorDistance, rasterPixelIndex } from './collarCommon'

const HIGH_COLLAR_MIN_COVERAGE = 0.88
const HIGH_COLLAR_MIN_UPPER_COVERAGE = 0.78
const HIGH_COLLAR_MATERIAL_DISTANCE = 12 * 12 * 7
const HIGH_COLLAR_MIN_MATERIAL_ROW_COVERAGE = 0.35
const HIGH_COLLAR_MIN_MATERIAL_ROWS = 0.5
const COLLAR_REFERENCE_SAMPLE_TARGET = 4096
const COLLAR_REFERENCE_MATCH_DISTANCE = 40 * 40 * 7
const COLLAR_REFERENCE_MIN_LAYER_SAMPLES = 64
const COLLAR_REFERENCE_MIN_AGREEMENT = 0.38

export function hasHighCollarEvidence(
  neck: RasterLayer,
  topwear: RasterLayer,
  anchors: Anime25DRiggerAnchors,
): boolean {
  const exposedTop = Math.ceil(Math.max(anchors.face.y1, neck.top))
  const exposedBottom = Math.floor(
    Math.min(anchors.neckBottom, neck.top + neck.height - 1),
  )
  if (exposedBottom - exposedTop < 8) return false
  if (
    topwear.top > exposedBottom ||
    topwear.top + topwear.height <= exposedTop ||
    topwear.left >= neck.left + neck.width ||
    topwear.left + topwear.width <= neck.left
  ) {
    return false
  }
  const upperBottom =
    exposedTop + (exposedBottom - exposedTop) * HIGH_COLLAR_UPPER_FRACTION
  let neckPixels = 0
  let overlapPixels = 0
  let upperNeckPixels = 0
  let upperOverlapPixels = 0
  let upperRows = 0
  let materialRows = 0
  for (let y = exposedTop; y <= exposedBottom; y += 1) {
    const upper = y <= upperBottom
    let rowPixels = 0
    let distinctPixels = 0
    for (let x = Math.ceil(neck.left); x < neck.left + neck.width; x += 1) {
      const neckPixel = rasterPixelIndex(neck, x, y)
      if (neckPixel < 0 || neck.data[neckPixel + 3] < ALPHA_COMPONENT_THRESHOLD)
        continue
      neckPixels += 1
      rowPixels += 1
      if (upper) upperNeckPixels += 1
      const garmentPixel = rasterPixelIndex(topwear, x, y)
      if (
        garmentPixel < 0 ||
        topwear.data[garmentPixel + 3] < ALPHA_COMPONENT_THRESHOLD
      ) {
        continue
      }
      overlapPixels += 1
      if (!upper) continue
      upperOverlapPixels += 1
      // Compare co-located authored colours, never a hard-coded skin palette.
      if (
        perceptualColorDistance(
          neck.data,
          neckPixel,
          topwear.data,
          garmentPixel,
        ) > HIGH_COLLAR_MATERIAL_DISTANCE
      ) {
        distinctPixels += 1
      }
    }
    if (upper && rowPixels >= 4) {
      upperRows += 1
      if (distinctPixels / rowPixels >= HIGH_COLLAR_MIN_MATERIAL_ROW_COVERAGE)
        materialRows += 1
    }
  }
  return (
    neckPixels >= 64 &&
    upperNeckPixels >= 24 &&
    overlapPixels / neckPixels >= HIGH_COLLAR_MIN_COVERAGE &&
    upperOverlapPixels / upperNeckPixels >= HIGH_COLLAR_MIN_UPPER_COVERAGE &&
    upperRows >= 3 &&
    materialRows / upperRows >= HIGH_COLLAR_MIN_MATERIAL_ROWS
  )
}

export function sourceReferenceAgreesWithLayers(
  reference: Anime25DSourceReference,
  layers: RasterLayer[],
): boolean {
  const agreements: number[] = []
  for (const role of ['front-hair', 'back-hair', 'face', 'topwear'] as const) {
    const layer = layers.find(
      (candidate) => candidate.role === role && !candidate.synthetic,
    )
    if (!layer) continue
    const sampleStep = Math.max(
      1,
      Math.floor(
        Math.sqrt(
          (layer.width * layer.height) / COLLAR_REFERENCE_SAMPLE_TARGET,
        ),
      ),
    )
    let samples = 0
    let matches = 0
    for (let y = 0; y < layer.height; y += sampleStep) {
      for (let x = 0; x < layer.width; x += sampleStep) {
        const layerPixel = (y * layer.width + x) * 4
        if (layer.data[layerPixel + 3] < 200) continue
        const canvasX = Math.round(layer.left) + x
        const canvasY = Math.round(layer.top) + y
        if (
          canvasX < 0 ||
          canvasX >= reference.width ||
          canvasY < 0 ||
          canvasY >= reference.height
        ) {
          continue
        }
        const referencePixel = (canvasY * reference.width + canvasX) * 4
        if (reference.data[referencePixel + 3] < 200) continue
        samples += 1
        if (
          perceptualColorDistance(
            reference.data,
            referencePixel,
            layer.data,
            layerPixel,
          ) <= COLLAR_REFERENCE_MATCH_DISTANCE
        ) {
          matches += 1
        }
      }
    }
    if (samples >= COLLAR_REFERENCE_MIN_LAYER_SAMPLES) {
      agreements.push(matches / samples)
    }
  }
  if (agreements.length < 2) return false
  const ranked = agreements.toSorted((left, right) => left - right)
  const middle = Math.floor(ranked.length / 2)
  const median =
    ranked.length % 2 === 0
      ? (ranked[middle - 1] + ranked[middle]) / 2
      : ranked[middle]
  return median >= COLLAR_REFERENCE_MIN_AGREEMENT
}
