import type { Anime25DRiggerAnchors } from '../anime25drig/playback'
import type {
  Anime25DSourceReference,
  RasterLayer,
} from './anime25dImportTypes'
import { uniquePartId } from './anime25dRaster'

const ALPHA_COMPONENT_THRESHOLD = 16
const HIGH_COLLAR_MIN_COVERAGE = 0.88
const HIGH_COLLAR_MIN_UPPER_COVERAGE = 0.78
const HIGH_COLLAR_UPPER_FRACTION = 0.35
const COLLAR_COLOR_CLUSTER_COUNT = 5
const COLLAR_COLOR_ITERATIONS = 8
const COLLAR_COLOR_MIN_LIGHTNESS_GAP = 0.035
const COLLAR_COLOR_SEED_FRACTION = 0.18
const COLLAR_COLOR_MAX_REAR_FRACTION = 0.48
const COLLAR_STANDALONE_MIN_LIGHTNESS_GAP = 0.018
const COLLAR_STANDALONE_MIN_SEED_FRACTION = 0.12
const COLLAR_STANDALONE_MIN_GEOMETRY = 0.08
const COLLAR_REFERENCE_SAMPLE_TARGET = 4096
const COLLAR_REFERENCE_MATCH_DISTANCE = 40 * 40 * 7
const COLLAR_REFERENCE_MIN_LAYER_SAMPLES = 64
const COLLAR_REFERENCE_MIN_AGREEMENT = 0.38
const COLLAR_REFERENCE_REAR_CONFIDENCE = 0.08
const COLLAR_REFERENCE_FRONT_CONFIDENCE = 0.15
const COLLAR_REFERENCE_REAR_MATCH_DISTANCE = 32 * 32 * 7
const COLLAR_REFERENCE_FRONT_MATCH_DISTANCE = 48 * 48 * 7
const COLLAR_REFERENCE_REAR = 1
const COLLAR_REFERENCE_FRONT = 2
const COLLAR_BOUNDARY_CENTER = 0.78
const COLLAR_BOUNDARY_SIDE = 0.3
const COLLAR_BOUNDARY_CURVE = 1.35
const COLLAR_BOUNDARY_FEATHER = 3

interface CollarColorSample {
  x: number
  y: number
  lightness: number
  chromaA: number
  chromaB: number
}

interface CollarColorComponent {
  pixels: number[]
  span: number
  score: number
}

interface CollarColorMask {
  left: number
  top: number
  width: number
  height: number
  data: Uint8Array
}

type CollarReferenceMask = CollarColorMask

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}

/**
 * Recover the three-layer high-collar topology lost when See-through flattens
 * all clothing into one topwear raster:
 *
 *   rear collar -> neck -> front collar / remaining topwear
 *
 * Detection uses alpha overlap in the exposed neck corridor. Once detected,
 * perceptual color segmentation finds the broad, darker collar region joined
 * to the upper edge before any depth decision is made. The aligned master then
 * separates visible neck from clothing and protects front-facing detail.
 */
export function splitHighCollarOcclusion(
  layers: RasterLayer[],
  anchors: Anime25DRiggerAnchors,
  sourceReference?: Anime25DSourceReference,
): RasterLayer[] {
  const neck = layers.find((layer) => layer.role === 'neck')
  const topwear = layers.find((layer) => layer.role === 'topwear')
  if (!neck || !topwear) return layers

  const exposedTop = Math.ceil(Math.max(anchors.face.y1, neck.top))
  const exposedBottom = Math.floor(
    Math.min(anchors.neckBottom, neck.top + neck.height - 1),
  )
  if (exposedBottom - exposedTop < 8) return layers

  const upperBottom =
    exposedTop + (exposedBottom - exposedTop) * HIGH_COLLAR_UPPER_FRACTION
  let neckPixels = 0
  let overlapPixels = 0
  let upperNeckPixels = 0
  let upperOverlapPixels = 0
  for (let y = exposedTop; y <= exposedBottom; y += 1) {
    for (let x = Math.ceil(neck.left); x < neck.left + neck.width; x += 1) {
      if (rasterAlphaAt(neck, x, y) < ALPHA_COMPONENT_THRESHOLD) continue
      neckPixels += 1
      const upper = y <= upperBottom
      if (upper) upperNeckPixels += 1
      if (rasterAlphaAt(topwear, x, y) < ALPHA_COMPONENT_THRESHOLD) continue
      overlapPixels += 1
      if (upper) upperOverlapPixels += 1
    }
  }
  if (neckPixels < 64 || upperNeckPixels < 24) return layers
  if (
    overlapPixels / neckPixels < HIGH_COLLAR_MIN_COVERAGE ||
    upperOverlapPixels / upperNeckPixels < HIGH_COLLAR_MIN_UPPER_COVERAGE
  ) {
    return layers
  }
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
  // Keep the reference-assisted path stable. A standalone PSD needs a full
  // front/rear partition because there is no visible master pixel to classify
  // the overlap; a rear-only color mask would move the entire front collar
  // behind the neck.
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
  const insertionIndex = Math.min(neckIndex, topwearIndex)
  const output = layers.filter((layer) => layer !== neck && layer !== topwear)
  // Playback paints in array order rather than consulting semantic depth. Keep
  // the actual raster order identical to the intended collar topology so
  // uncertain topwear pixels cannot be painted back over the neck.
  output.splice(insertionIndex, 0, remainingTopwear, rearCollar, neck)
  if (frontCollar) output.splice(insertionIndex + 3, 0, frontCollar)
  return output
}

function copyRasterPixel(
  source: Uint8ClampedArray,
  target: Uint8ClampedArray,
  pixel: number,
  alpha: number,
): void {
  target[pixel] = source[pixel]
  target[pixel + 1] = source[pixel + 1]
  target[pixel + 2] = source[pixel + 2]
  target[pixel + 3] = Math.round(alpha)
}

function cropRasterPixels(
  source: Uint8ClampedArray,
  sourceWidth: number,
  left: number,
  top: number,
  width: number,
  height: number,
): Uint8ClampedArray {
  const output = new Uint8ClampedArray(width * height * 4)
  for (let y = 0; y < height; y += 1) {
    const sourceStart = ((top + y) * sourceWidth + left) * 4
    const sourceEnd = sourceStart + width * 4
    output.set(source.subarray(sourceStart, sourceEnd), y * width * 4)
  }
  return output
}

/**
 * A manually selected PSD can outlive the site's current master portrait.
 * Reject that stale pairing before using master pixels as semantic truth.
 * Median agreement across identity-bearing regions tolerates inpainting and
 * local occlusion while still separating different characters and outfits.
 */
function sourceReferenceAgreesWithLayers(
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
  agreements.sort((left, right) => left - right)
  const middle = Math.floor(agreements.length / 2)
  const median =
    agreements.length % 2 === 0
      ? (agreements[middle - 1] + agreements[middle]) / 2
      : agreements[middle]
  return median >= COLLAR_REFERENCE_MIN_AGREEMENT
}

/**
 * Split the collar palette before assigning depth. K-means operates in OKLab
 * so luminance differences remain useful across pale, saturated, and dark
 * outfits. A trusted reference keeps the conservative dark-component mask;
 * standalone PSDs instead follow the largest darker upper-edge material
 * through the collar geometry and classify the complementary overlap as the
 * front collar.
 */
function segmentRearCollarByColor(
  neck: RasterLayer,
  topwear: RasterLayer,
  exposedTop: number,
  exposedBottom: number,
  standalone: boolean,
): CollarColorMask | undefined {
  const left = Math.ceil(neck.left)
  const right = Math.floor(neck.left + neck.width)
  const width = right - left
  const height = exposedBottom - exposedTop + 1
  if (width < 8 || height < 8) return undefined

  const samples: CollarColorSample[] = []
  for (let y = exposedTop; y <= exposedBottom; y += 1) {
    for (let x = left; x < right; x += 1) {
      if (rasterAlphaAt(neck, x, y) < ALPHA_COMPONENT_THRESHOLD) continue
      const topwearPixel = rasterPixelIndex(topwear, x, y)
      if (
        topwearPixel < 0 ||
        topwear.data[topwearPixel + 3] < ALPHA_COMPONENT_THRESHOLD
      ) {
        continue
      }
      const [lightness, chromaA, chromaB] = oklabAt(topwear.data, topwearPixel)
      samples.push({
        x: x - left,
        y: y - exposedTop,
        lightness,
        chromaA,
        chromaB,
      })
    }
  }
  if (samples.length < 64) return undefined

  const centroids: Array<[number, number, number]> = []
  const middle = samples[Math.floor(samples.length / 2)]
  centroids.push([middle.lightness, middle.chromaA, middle.chromaB])
  while (
    centroids.length < Math.min(COLLAR_COLOR_CLUSTER_COUNT, samples.length)
  ) {
    let farthest = samples[0]
    let farthestDistance = -1
    for (const sample of samples) {
      let nearestDistance = Number.POSITIVE_INFINITY
      for (const centroid of centroids) {
        nearestDistance = Math.min(
          nearestDistance,
          collarColorDistance(sample, centroid),
        )
      }
      if (nearestDistance > farthestDistance) {
        farthest = sample
        farthestDistance = nearestDistance
      }
    }
    if (farthestDistance <= Number.EPSILON) break
    centroids.push([farthest.lightness, farthest.chromaA, farthest.chromaB])
  }
  if (centroids.length < 2) return undefined

  const labels = new Int8Array(samples.length)
  for (let iteration = 0; iteration < COLLAR_COLOR_ITERATIONS; iteration += 1) {
    const sums = centroids.map(() => [0, 0, 0, 0])
    samples.forEach((sample, index) => {
      let label = 0
      let nearestDistance = Number.POSITIVE_INFINITY
      centroids.forEach((centroid, centroidIndex) => {
        const distance = collarColorDistance(sample, centroid)
        if (distance >= nearestDistance) return
        nearestDistance = distance
        label = centroidIndex
      })
      labels[index] = label
      const sum = sums[label]
      sum[0] += sample.lightness
      sum[1] += sample.chromaA
      sum[2] += sample.chromaB
      sum[3] += 1
    })
    sums.forEach((sum, index) => {
      if (sum[3] === 0) return
      centroids[index] = [sum[0] / sum[3], sum[1] / sum[3], sum[2] / sum[3]]
    })
  }

  const seedHeight = Math.max(1, Math.ceil(height * COLLAR_COLOR_SEED_FRACTION))
  const upperHeight = Math.max(
    seedHeight,
    Math.ceil(height * HIGH_COLLAR_UPPER_FRACTION),
  )
  const seedCounts = new Uint32Array(centroids.length)
  let upperLightness = 0
  let upperCount = 0
  samples.forEach((sample, index) => {
    if (sample.y < seedHeight) seedCounts[labels[index]] += 1
    if (sample.y >= upperHeight) return
    upperLightness += sample.lightness
    upperCount += 1
  })
  if (upperCount === 0) return undefined
  upperLightness /= upperCount

  let rearLabel = -1
  let rearSeedCount = 0
  if (standalone) {
    // The rear lining is normally darker than the upper collar as a whole,
    // but pale outfits can have only a small luminance gap. Prefer the largest
    // upper-edge cluster that is meaningfully darker; falling back to the
    // dominant cluster still supports nearly monochrome collars.
    centroids.forEach((centroid, index) => {
      const count = seedCounts[index]
      if (
        centroid[0] >= upperLightness - COLLAR_STANDALONE_MIN_LIGHTNESS_GAP ||
        count <= rearSeedCount
      ) {
        return
      }
      rearLabel = index
      rearSeedCount = count
    })
    if (rearLabel < 0) {
      seedCounts.forEach((count, index) => {
        if (count <= rearSeedCount) return
        rearLabel = index
        rearSeedCount = count
      })
    }
  } else {
    centroids.forEach((centroid, index) => {
      if (
        centroid[0] >= upperLightness - COLLAR_COLOR_MIN_LIGHTNESS_GAP ||
        seedCounts[index] <= rearSeedCount
      ) {
        return
      }
      rearLabel = index
      rearSeedCount = seedCounts[index]
    })
  }
  if (rearLabel < 0 || rearSeedCount < Math.max(12, width * 0.08)) {
    return undefined
  }
  if (standalone) {
    const seedSampleCount = seedCounts.reduce((sum, count) => sum + count, 0)
    if (
      rearSeedCount / Math.max(1, seedSampleCount) <
      COLLAR_STANDALONE_MIN_SEED_FRACTION
    ) {
      return undefined
    }
  }

  const labelGrid = new Int8Array(width * height)
  labelGrid.fill(-1)
  samples.forEach((sample, index) => {
    labelGrid[sample.y * width + sample.x] = labels[index]
  })
  const components = collarColorComponents(
    labelGrid,
    width,
    height,
    rearLabel,
    seedHeight,
    standalone,
  )
  const best = components[0]
  if (
    !best ||
    best.pixels.length < Math.max(24, samples.length * 0.01) ||
    best.span < width * 0.15
  ) {
    return undefined
  }

  const mask = new Uint8Array(width * height)
  for (const component of components) {
    if (component.score < best.score * 0.35) break
    const rowMinimum = new Int16Array(height)
    const rowMaximum = new Int16Array(height)
    rowMinimum.fill(width)
    rowMaximum.fill(-1)
    for (const pixel of component.pixels) {
      const y = Math.floor(pixel / width)
      const x = pixel - y * width
      rowMinimum[y] = Math.min(rowMinimum[y], x)
      rowMaximum[y] = Math.max(rowMaximum[y], x)
    }
    for (let y = 0; y < height; y += 1) {
      if (rowMaximum[y] < rowMinimum[y]) continue
      const start = Math.max(0, rowMinimum[y] - 1)
      const end = Math.min(width - 1, rowMaximum[y] + 1)
      for (let x = start; x <= end; x += 1) {
        if (
          standalone &&
          collarBoundaryRearAmount(
            x,
            y,
            width / 2,
            width / 2,
            0,
            Math.max(1, height - 1),
          ) < COLLAR_STANDALONE_MIN_GEOMETRY
        ) {
          continue
        }
        mask[y * width + x] = COLLAR_REFERENCE_REAR
      }
    }
  }
  if (standalone) {
    for (let pixel = 0; pixel < mask.length; pixel += 1) {
      if (labelGrid[pixel] >= 0 && mask[pixel] === 0) {
        mask[pixel] = COLLAR_REFERENCE_FRONT
      }
    }
  }
  return { left, top: exposedTop, width, height, data: mask }
}

function collarColorComponents(
  labels: Int8Array,
  width: number,
  height: number,
  rearLabel: number,
  seedHeight: number,
  standalone: boolean,
): CollarColorComponent[] {
  const maximumY = standalone
    ? height
    : Math.min(height, Math.ceil(height * COLLAR_COLOR_MAX_REAR_FRACTION))
  const visited = new Uint8Array(width * height)
  const components: CollarColorComponent[] = []
  for (let y = 0; y < seedHeight; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const start = y * width + x
      if (visited[start] || labels[start] !== rearLabel) continue
      const queue = [start]
      const pixels: number[] = []
      let cursor = 0
      let minimumX = width
      let maximumX = -1
      visited[start] = 1
      while (cursor < queue.length) {
        const pixel = queue[cursor]
        cursor += 1
        pixels.push(pixel)
        const currentY = Math.floor(pixel / width)
        const currentX = pixel - currentY * width
        minimumX = Math.min(minimumX, currentX)
        maximumX = Math.max(maximumX, currentX)
        const neighbours = [
          [currentX - 1, currentY],
          [currentX + 1, currentY],
          [currentX, currentY - 1],
          [currentX, currentY + 1],
        ]
        for (const [nextX, nextY] of neighbours) {
          if (nextX < 0 || nextX >= width || nextY < 0 || nextY >= maximumY) {
            continue
          }
          if (
            standalone &&
            collarBoundaryRearAmount(
              nextX,
              nextY,
              width / 2,
              width / 2,
              0,
              Math.max(1, height - 1),
            ) < COLLAR_STANDALONE_MIN_GEOMETRY
          ) {
            continue
          }
          const next = nextY * width + nextX
          if (visited[next] || labels[next] !== rearLabel) continue
          visited[next] = 1
          queue.push(next)
        }
      }
      const span = maximumX - minimumX + 1
      components.push({ pixels, span, score: pixels.length * span })
    }
  }
  return components.sort((left, right) => right.score - left.score)
}

function collarColorMaskAt(
  mask: CollarColorMask | undefined,
  x: number,
  y: number,
): number {
  if (!mask) return 0
  const localX = x - mask.left
  const localY = y - mask.top
  if (
    localX < 0 ||
    localX >= mask.width ||
    localY < 0 ||
    localY >= mask.height
  ) {
    return 0
  }
  return mask.data[localY * mask.width + localX]
}

/**
 * Feather only the internal front/rear seam of a standalone PSD mask. Pixels
 * outside the classified overlap are ignored, preserving the source layer's
 * own antialiased outer contour while avoiding a rigid cut between two meshes.
 */
function collarStandaloneRearAmount(
  mask: CollarReferenceMask | undefined,
  x: number,
  y: number,
): number {
  if (!mask) return -1
  const localX = x - mask.left
  const localY = y - mask.top
  if (
    localX < 0 ||
    localX >= mask.width ||
    localY < 0 ||
    localY >= mask.height
  ) {
    return -1
  }
  const center = mask.data[localY * mask.width + localX]
  if (center === 0) return -1

  let rearWeight = 0
  let classifiedWeight = 0
  for (
    let sampleY = Math.max(0, localY - 1);
    sampleY <= Math.min(mask.height - 1, localY + 1);
    sampleY += 1
  ) {
    for (
      let sampleX = Math.max(0, localX - 1);
      sampleX <= Math.min(mask.width - 1, localX + 1);
      sampleX += 1
    ) {
      const value = mask.data[sampleY * mask.width + sampleX]
      if (value === 0) continue
      const horizontalDistance = Math.abs(sampleX - localX)
      const verticalDistance = Math.abs(sampleY - localY)
      const weight =
        horizontalDistance === 0 && verticalDistance === 0
          ? 4
          : horizontalDistance + verticalDistance === 1
            ? 2
            : 1
      classifiedWeight += weight
      if (value === COLLAR_REFERENCE_REAR) rearWeight += weight
    }
  }
  return classifiedWeight > 0 ? rearWeight / classifiedWeight : -1
}

function collarColorDistance(
  sample: CollarColorSample,
  centroid: [number, number, number],
): number {
  const lightness = sample.lightness - centroid[0]
  const chromaA = sample.chromaA - centroid[1]
  const chromaB = sample.chromaB - centroid[2]
  return lightness * lightness * 4 + chromaA * chromaA + chromaB * chromaB
}

function oklabAt(
  data: Uint8ClampedArray,
  pixel: number,
): [number, number, number] {
  const red = linearSrgb(data[pixel])
  const green = linearSrgb(data[pixel + 1])
  const blue = linearSrgb(data[pixel + 2])
  const long = Math.cbrt(
    0.4122214708 * red + 0.5363325363 * green + 0.0514459929 * blue,
  )
  const medium = Math.cbrt(
    0.2119034982 * red + 0.6806995451 * green + 0.1073969566 * blue,
  )
  const short = Math.cbrt(
    0.0883024619 * red + 0.2817188376 * green + 0.6299787005 * blue,
  )
  return [
    0.2104542553 * long + 0.793617785 * medium - 0.0040720468 * short,
    1.9779984951 * long - 2.428592205 * medium + 0.4505937099 * short,
    0.0259040371 * long + 0.7827717662 * medium - 0.808675766 * short,
  ]
}

function linearSrgb(value: number): number {
  const normalized = value / 255
  return normalized <= 0.04045
    ? normalized / 12.92
    : ((normalized + 0.055) / 1.055) ** 2.4
}

function collarBoundaryRearAmount(
  x: number,
  y: number,
  centerX: number,
  halfWidth: number,
  exposedTop: number,
  exposedHeight: number,
): number {
  const horizontal = clamp(Math.abs(x + 0.5 - centerX) / halfWidth, 0, 1)
  const boundaryFraction =
    COLLAR_BOUNDARY_CENTER -
    (COLLAR_BOUNDARY_CENTER - COLLAR_BOUNDARY_SIDE) *
      horizontal ** COLLAR_BOUNDARY_CURVE
  const boundaryY = exposedTop + exposedHeight * boundaryFraction
  const transition = clamp(
    (boundaryY + COLLAR_BOUNDARY_FEATHER - (y + 0.5)) /
      (COLLAR_BOUNDARY_FEATHER * 2),
    0,
    1,
  )
  return transition * transition * (3 - 2 * transition)
}

function buildCollarReferenceMask(
  reference: Anime25DSourceReference,
  neck: RasterLayer,
  topwear: RasterLayer,
  exposedTop: number,
  exposedBottom: number,
): CollarReferenceMask {
  const left = Math.ceil(neck.left)
  const right = Math.floor(neck.left + neck.width)
  const width = right - left
  const height = exposedBottom - exposedTop + 1
  const raw = new Uint8Array(width * height)
  for (let y = exposedTop; y <= exposedBottom; y += 1) {
    for (let x = left; x < right; x += 1) {
      if (x < 0 || y < 0 || x >= reference.width || y >= reference.height) {
        continue
      }
      const referencePixel = (y * reference.width + x) * 4
      if (reference.data[referencePixel + 3] < ALPHA_COMPONENT_THRESHOLD) {
        continue
      }
      const neckPixel = rasterPixelIndex(neck, x, y)
      const topwearPixel = rasterPixelIndex(topwear, x, y)
      if (
        neckPixel < 0 ||
        topwearPixel < 0 ||
        neck.data[neckPixel + 3] < ALPHA_COMPONENT_THRESHOLD ||
        topwear.data[topwearPixel + 3] < ALPHA_COMPONENT_THRESHOLD
      ) {
        continue
      }
      const neckDistance = perceptualColorDistance(
        reference.data,
        referencePixel,
        neck.data,
        neckPixel,
      )
      const topwearDistance = perceptualColorDistance(
        reference.data,
        referencePixel,
        topwear.data,
        topwearPixel,
      )
      const confidence =
        Math.abs(neckDistance - topwearDistance) /
        (neckDistance + topwearDistance + 1)
      const pixel = (y - exposedTop) * width + x - left
      if (
        neckDistance < topwearDistance &&
        neckDistance <= COLLAR_REFERENCE_REAR_MATCH_DISTANCE &&
        confidence >= COLLAR_REFERENCE_REAR_CONFIDENCE
      ) {
        raw[pixel] = COLLAR_REFERENCE_REAR
      } else if (
        topwearDistance < neckDistance &&
        topwearDistance <= COLLAR_REFERENCE_FRONT_MATCH_DISTANCE &&
        confidence >= COLLAR_REFERENCE_FRONT_CONFIDENCE
      ) {
        raw[pixel] = COLLAR_REFERENCE_FRONT
      }
    }
  }

  const cleaned = new Uint8Array(raw.length)
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const pixel = y * width + x
      const value = raw[pixel]
      if (value === 0) continue
      const neighbours = collarReferenceNeighbourCounts(
        raw,
        width,
        height,
        x,
        y,
      )
      if (
        (value === COLLAR_REFERENCE_REAR && neighbours.rear >= 3) ||
        (value === COLLAR_REFERENCE_FRONT && neighbours.front >= 3)
      ) {
        cleaned[pixel] = value
      }
    }
  }
  const filled = new Uint8Array(cleaned)
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const pixel = y * width + x
      if (cleaned[pixel] !== 0) continue
      const neighbours = collarReferenceNeighbourCounts(
        cleaned,
        width,
        height,
        x,
        y,
      )
      if (neighbours.front >= 6 && neighbours.rear <= 1) {
        filled[pixel] = COLLAR_REFERENCE_FRONT
      } else if (neighbours.rear >= 6 && neighbours.front <= 1) {
        filled[pixel] = COLLAR_REFERENCE_REAR
      }
    }
  }
  return { left, top: exposedTop, width, height, data: filled }
}

function collarReferenceNeighbourCounts(
  data: Uint8Array,
  width: number,
  height: number,
  centerX: number,
  centerY: number,
): { rear: number; front: number } {
  let rear = 0
  let front = 0
  for (
    let y = Math.max(0, centerY - 1);
    y <= Math.min(height - 1, centerY + 1);
    y += 1
  ) {
    for (
      let x = Math.max(0, centerX - 1);
      x <= Math.min(width - 1, centerX + 1);
      x += 1
    ) {
      const value = data[y * width + x]
      if (value === COLLAR_REFERENCE_REAR) rear += 1
      else if (value === COLLAR_REFERENCE_FRONT) front += 1
    }
  }
  return { rear, front }
}

function collarReferenceMaskAt(
  mask: CollarReferenceMask | undefined,
  x: number,
  y: number,
): number {
  if (!mask) return 0
  const localX = x - mask.left
  const localY = y - mask.top
  if (
    localX < 0 ||
    localX >= mask.width ||
    localY < 0 ||
    localY >= mask.height
  ) {
    return 0
  }
  return mask.data[localY * mask.width + localX]
}

function perceptualColorDistance(
  left: Uint8ClampedArray,
  leftPixel: number,
  right: Uint8ClampedArray,
  rightPixel: number,
): number {
  const red = left[leftPixel] - right[rightPixel]
  const green = left[leftPixel + 1] - right[rightPixel + 1]
  const blue = left[leftPixel + 2] - right[rightPixel + 2]
  return red * red * 2 + green * green * 4 + blue * blue
}

function rasterPixelIndex(
  layer: RasterLayer,
  canvasX: number,
  canvasY: number,
): number {
  const x = canvasX - Math.round(layer.left)
  const y = canvasY - Math.round(layer.top)
  if (x < 0 || y < 0 || x >= layer.width || y >= layer.height) return -1
  return (y * layer.width + x) * 4
}

function rasterAlphaAt(layer: RasterLayer, canvasX: number, canvasY: number) {
  const pixel = rasterPixelIndex(layer, canvasX, canvasY)
  return pixel < 0 ? 0 : layer.data[pixel + 3]
}
