import type { RasterLayer } from './anime25dImportTypes'
import type { CollarColorMask, CollarReferenceMask } from './collarCommon'
import { ALPHA_COMPONENT_THRESHOLD, clamp, COLLAR_REFERENCE_FRONT, COLLAR_REFERENCE_REAR, HIGH_COLLAR_UPPER_FRACTION, rasterAlphaAt, rasterPixelIndex } from './collarCommon'

const COLLAR_COLOR_CLUSTER_COUNT = 5
const COLLAR_COLOR_ITERATIONS = 8
const COLLAR_COLOR_MIN_LIGHTNESS_GAP = 0.035
const COLLAR_COLOR_SEED_FRACTION = 0.18
const COLLAR_COLOR_MAX_REAR_FRACTION = 0.48
const COLLAR_STANDALONE_MIN_LIGHTNESS_GAP = 0.018
const COLLAR_STANDALONE_MIN_SEED_FRACTION = 0.12
const COLLAR_STANDALONE_MIN_GEOMETRY = 0.08
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

export function segmentRearCollarByColor(
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
  return components.toSorted((left, right) => right.score - left.score)
}

export function collarColorMaskAt(
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

/** Feather only the internal front/rear seam of a standalone PSD mask. */
export function collarStandaloneRearAmount(
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

export function collarBoundaryRearAmount(
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
