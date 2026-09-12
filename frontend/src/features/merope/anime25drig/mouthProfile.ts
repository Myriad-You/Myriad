import type {
  Anime25DMouthBridgeTuning,
  Anime25DMouthMaterial,
  Anime25DMouthProfile,
  Anime25DMouthSilhouette,
  Anime25DPlaybackAnchors,
} from './types'

export const ANIME25D_MOUTH_MATERIALS: readonly Anime25DMouthMaterial[] = [
  'mouthClose',
  'mouthOpen',
  'mouthWide',
  'mouthRound',
  'mouthNarrow',
  'mouthManiac',
]

interface MouthRasterLayer {
  role: string
  left: number
  top: number
  width: number
  height: number
  data: Uint8ClampedArray
}

interface MouthAnalysisFrame {
  x: number
  y: number
  width: number
  height: number
}

interface AnalyzedSilhouette {
  silhouette: Anime25DMouthSilhouette
  boxCenterX: number
  boxCenterY: number
}

interface BaseBridgeProfile {
  widthScale: number
  heightScale: number
  neutralization: number
  expectedWidthSimilarity: number
  expectedHeightSimilarity: number
  expectedDifference: number
}

export function analyzeAnime25DMouthProfile(
  layers: readonly MouthRasterLayer[],
  frame: Readonly<MouthAnalysisFrame>,
  fallback: Anime25DPlaybackAnchors['mouth'],
): Anime25DMouthProfile {
  const analyzed: AnalyzedSilhouette[] = []
  let usedBoundsFallback = false
  for (const material of ANIME25D_MOUTH_MATERIALS) {
    const role = roleForMaterial(material)
    const layer = layers.find((candidate) => candidate.role === role)
    const result = layer ? analyzeSilhouette(layer, material, frame) : null
    if (result) {
      analyzed.push(result)
    } else {
      usedBoundsFallback = true
      analyzed.push(fallbackSilhouette(material, fallback))
    }
  }
  return {
    version: 1,
    source: usedBoundsFallback ? 'bounds-fallback' : 'alpha-contour',
    silhouettes: analyzed.map((entry) => entry.silhouette),
    bridges: buildBridges(analyzed),
  }
}

function analyzeSilhouette(
  layer: Readonly<MouthRasterLayer>,
  material: Anime25DMouthMaterial,
  frame: Readonly<MouthAnalysisFrame>,
): AnalyzedSilhouette | null {
  if (
    layer.width <= 0 ||
    layer.height <= 0 ||
    layer.data.length !== layer.width * layer.height * 4
  ) {
    return null
  }
  const columns = new Float64Array(layer.width)
  const rows = new Float64Array(layer.height)
  let total = 0
  for (let y = 0; y < layer.height; y += 1) {
    for (let x = 0; x < layer.width; x += 1) {
      const alpha = layer.data[(y * layer.width + x) * 4 + 3]
      if (alpha <= 8) continue
      const weight = alpha / 255
      columns[x] += weight
      rows[y] += weight
      total += weight
    }
  }
  if (total <= 0) return null

  const minX = weightedQuantile(columns, total, 0.015)
  const maxX = weightedQuantile(columns, total, 0.985)
  const minY = weightedQuantile(rows, total, 0.015)
  const maxY = weightedQuantile(rows, total, 0.985)
  const width = Math.max(1, maxX - minX + 1)
  const height = Math.max(1, maxY - minY + 1)
  const cornerEdge = Math.max(1, Math.round(width * 0.22))
  let visibleWeight = 0
  let sumX = 0
  let sumY = 0
  let leftWeight = 0
  let leftSumY = 0
  let rightWeight = 0
  let rightSumY = 0
  for (let y = minY; y <= maxY; y += 1) {
    for (let x = minX; x <= maxX; x += 1) {
      const alpha = layer.data[(y * layer.width + x) * 4 + 3]
      if (alpha <= 8) continue
      const weight = alpha / 255
      visibleWeight += weight
      sumX += x * weight
      sumY += y * weight
      if (x < minX + cornerEdge) {
        leftWeight += weight
        leftSumY += y * weight
      }
      if (x > maxX - cornerEdge) {
        rightWeight += weight
        rightSumY += y * weight
      }
    }
  }
  if (visibleWeight <= 0) return null
  const localCenterX = sumX / visibleWeight
  const localCenterY = sumY / visibleWeight
  const centerX = layer.left - frame.x + localCenterX
  const centerY = layer.top - frame.y + localCenterY
  const leftCornerY =
    layer.top -
    frame.y +
    (leftWeight > 0 ? leftSumY / leftWeight : localCenterY)
  const rightCornerY =
    layer.top -
    frame.y +
    (rightWeight > 0 ? rightSumY / rightWeight : localCenterY)
  return {
    silhouette: {
      material,
      centerX,
      centerY,
      width,
      height,
      leftCornerY,
      rightCornerY,
      fillRatio: clamp(visibleWeight / (width * height), 0, 1),
      aperture: clamp(height / width, 0, 4),
    },
    boxCenterX: layer.left - frame.x + layer.width / 2,
    boxCenterY: layer.top - frame.y + layer.height / 2,
  }
}

function fallbackSilhouette(
  material: Anime25DMouthMaterial,
  mouth: Anime25DPlaybackAnchors['mouth'],
): AnalyzedSilhouette {
  const baseWidth = Math.max(1, mouth.x1 - mouth.x0)
  const baseHeight = Math.max(1, mouth.y1 - mouth.y0)
  const size = fallbackSize(material, baseWidth, baseHeight)
  return {
    silhouette: {
      material,
      centerX: mouth.cx,
      centerY: mouth.cy,
      width: size.width,
      height: size.height,
      leftCornerY: mouth.cy,
      rightCornerY: mouth.cy,
      fillRatio: material === 'mouthClose' ? 0.35 : 0.72,
      aperture: clamp(size.height / size.width, 0, 4),
    },
    boxCenterX: mouth.cx,
    boxCenterY: mouth.cy,
  }
}

function fallbackSize(
  material: Anime25DMouthMaterial,
  width: number,
  height: number,
): { width: number; height: number } {
  if (material === 'mouthOpen')
    return { width: width * 0.96, height: height * 2 }
  if (material === 'mouthWide')
    return { width: width * 1.24, height: height * 1.35 }
  if (material === 'mouthRound')
    return { width: width * 0.76, height: height * 2.2 }
  if (material === 'mouthNarrow')
    return { width: width * 1.08, height: height * 0.9 }
  if (material === 'mouthManiac')
    return { width: width * 3.2, height: height * 4.2 }
  return { width, height }
}

function buildBridges(
  analyzed: readonly AnalyzedSilhouette[],
): Anime25DMouthBridgeTuning[] {
  const bridges: Anime25DMouthBridgeTuning[] = []
  for (let firstIndex = 0; firstIndex < analyzed.length; firstIndex += 1) {
    for (
      let secondIndex = firstIndex + 1;
      secondIndex < analyzed.length;
      secondIndex += 1
    ) {
      bridges.push(buildBridge(analyzed[firstIndex], analyzed[secondIndex]))
    }
  }
  return bridges
}

function buildBridge(
  first: Readonly<AnalyzedSilhouette>,
  second: Readonly<AnalyzedSilhouette>,
): Anime25DMouthBridgeTuning {
  const a = first.silhouette
  const b = second.silhouette
  const base = baseBridge(a.material, b.material)
  const widthSimilarity = ratioSimilarity(a.width, b.width)
  const heightSimilarity = ratioSimilarity(a.height, b.height)
  const apertureDifference = normalizedDifference(a.aperture, b.aperture)
  const fillDifference = Math.abs(a.fillRatio - b.fillRatio)
  const firstCornerSlope = (a.rightCornerY - a.leftCornerY) / a.height
  const secondCornerSlope = (b.rightCornerY - b.leftCornerY) / b.height
  const cornerDifference = Math.min(
    1,
    Math.abs(firstCornerSlope - secondCornerSlope),
  )
  const difference =
    (1 - widthSimilarity) * 0.36 +
    (1 - heightSimilarity) * 0.36 +
    apertureDifference * 0.14 +
    fillDifference * 0.08 +
    cornerDifference * 0.06
  const centerOffsetX =
    ((a.centerX - first.boxCenterX + (b.centerX - second.boxCenterX)) / 2) *
    0.65
  const centerOffsetY =
    ((a.centerY - first.boxCenterY + (b.centerY - second.boxCenterY)) / 2) *
    0.65
  const maxOffsetX = Math.max(0.5, Math.min(a.width, b.width) * 0.08)
  const maxOffsetY = Math.max(0.5, Math.min(a.height, b.height) * 0.1)
  return {
    first: a.material,
    second: b.material,
    widthScale: clamp(
      base.widthScale + (widthSimilarity - base.expectedWidthSimilarity) * 0.1,
      base.widthScale - 0.035,
      Math.min(1, base.widthScale + 0.025),
    ),
    heightScale: clamp(
      base.heightScale +
        (heightSimilarity - base.expectedHeightSimilarity) * 0.1,
      base.heightScale - 0.045,
      Math.min(1, base.heightScale + 0.03),
    ),
    neutralization: clamp(
      base.neutralization + (difference - base.expectedDifference) * 0.45,
      Math.max(0, base.neutralization - 0.13),
      Math.min(1, base.neutralization + 0.13),
    ),
    centerOffsetX: clamp(centerOffsetX, -maxOffsetX, maxOffsetX),
    centerOffsetY: clamp(centerOffsetY, -maxOffsetY, maxOffsetY),
  }
}

function baseBridge(
  first: Anime25DMouthMaterial,
  second: Anime25DMouthMaterial,
): BaseBridgeProfile {
  if (first === 'mouthManiac' || second === 'mouthManiac') {
    return {
      widthScale: 0.9,
      heightScale: 0.76,
      neutralization: 0.66,
      expectedWidthSimilarity: 0.4,
      expectedHeightSimilarity: 0.32,
      expectedDifference: 0.5,
    }
  }
  if (first === 'mouthClose' || second === 'mouthClose') {
    return {
      widthScale: 0.97,
      heightScale: 0.82,
      neutralization: 0.5,
      expectedWidthSimilarity: 0.9,
      expectedHeightSimilarity: 0.22,
      expectedDifference: 0.42,
    }
  }
  const wideRound =
    (first === 'mouthWide' && second === 'mouthRound') ||
    (first === 'mouthRound' && second === 'mouthWide')
  if (wideRound) {
    return {
      widthScale: 0.9,
      heightScale: 0.88,
      neutralization: 0.72,
      expectedWidthSimilarity: 0.62,
      expectedHeightSimilarity: 0.72,
      expectedDifference: 0.34,
    }
  }
  if (first === 'mouthRound' || second === 'mouthRound') {
    return {
      widthScale: 0.93,
      heightScale: 0.9,
      neutralization: 0.58,
      expectedWidthSimilarity: 0.75,
      expectedHeightSimilarity: 0.72,
      expectedDifference: 0.29,
    }
  }
  if (first === 'mouthNarrow' || second === 'mouthNarrow') {
    return {
      widthScale: 0.95,
      heightScale: 0.9,
      neutralization: 0.44,
      expectedWidthSimilarity: 0.82,
      expectedHeightSimilarity: 0.58,
      expectedDifference: 0.29,
    }
  }
  return {
    widthScale: 0.98,
    heightScale: 0.94,
    neutralization: 0.3,
    expectedWidthSimilarity: 0.85,
    expectedHeightSimilarity: 0.78,
    expectedDifference: 0.19,
  }
}

function roleForMaterial(material: Anime25DMouthMaterial): string {
  if (material === 'mouthClose') return 'mouth-close'
  if (material === 'mouthOpen') return 'mouth-open'
  if (material === 'mouthWide') return 'mouth-wide'
  if (material === 'mouthRound') return 'mouth-round'
  if (material === 'mouthManiac') return 'mouth-maniac'
  return 'mouth-narrow'
}

function weightedQuantile(
  values: Float64Array,
  total: number,
  quantile: number,
): number {
  const threshold = total * quantile
  let cumulative = 0
  for (let index = 0; index < values.length; index += 1) {
    cumulative += values[index]
    if (cumulative >= threshold) return index
  }
  return Math.max(0, values.length - 1)
}

function ratioSimilarity(first: number, second: number): number {
  return Math.min(first, second) / Math.max(1e-5, Math.max(first, second))
}

function normalizedDifference(first: number, second: number): number {
  return Math.min(
    1,
    Math.abs(first - second) / Math.max(0.05, Math.max(first, second)),
  )
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
