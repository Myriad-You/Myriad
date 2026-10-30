import type {
  Anime25DSourceReference,
  RasterLayer,
} from './anime25dImportTypes'
import type { EarAccessoryMasks } from './psdEarAccessory'
import type { RigRect } from './types'
import { earAccessoryMasks } from './psdEarAccessory'

/**
 * Why the rest composite disagrees with the source illustration at a spot:
 * - `buried`: a lower layer holds the right art but another layer covers it.
 * - `missing`: the illustration shows art there that no layer carries.
 * - `spurious`: a layer paints where the illustration shows only background.
 * - `mismatch`: the visible layer holds different art and none below fits.
 */
export type Anime25DPsdDefectKind =
  'buried' | 'missing' | 'spurious' | 'mismatch'

export interface Anime25DPsdDefectRegion {
  kind: Anime25DPsdDefectKind
  /** Pixels in PSD document space. */
  area: number
  box: RigRect
  /** The covered layer for `buried`, else the visible layer, if any. */
  role: string | null
}

export interface Anime25DPsdReconciliation {
  status: 'reconciled' | 'reference-mismatch'
  /** Share of opaque composite pixels whose colour matches the source. */
  agreement: number
  /** Opaque composite pixels compared against the source. */
  contentArea: number
  /** False when the source background is not flat enough to tell art from it. */
  backgroundKnown: boolean
  regions: Anime25DPsdDefectRegion[]
  /** Regions import already fixed; `regions` lists what remains after that. */
  repairs: { revealed: number; recovered: number }
  /** Source-shaded overview of the content with defect pixels coloured. */
  heatmap: { width: number; height: number; data: Uint8ClampedArray } | null
}

export const ANIME25D_PSD_DEFECT_COLORS: Readonly<
  Record<Anime25DPsdDefectKind, readonly [number, number, number]>
> = {
  buried: [168, 85, 247],
  missing: [239, 68, 68],
  spurious: [59, 130, 246],
  mismatch: [245, 158, 11],
}

export const ANIME25D_PSD_REPAIRED_COLOR = [34, 197, 94] as const

/** See-through re-renders colours; aligned art still differs by ~15-18 RGB. */
export const MATCH_DISTANCE = 70
const MISMATCH_DISTANCE = 98
const BACKGROUND_DISTANCE = 28
/** Below this the PSD was not decomposed from this illustration. */
const MIN_AGREEMENT = 0.8
const MAX_BACKGROUND_SPREAD = 20
const MIN_BACKGROUND_SAMPLES = 256
const MIN_REGION_SHARE = 0.0004
const MIN_REGION_PIXELS = 48
const MAX_REGIONS = 24
const HEATMAP_EDGE = 320
const OPAQUE = 0.5
export const ANIME25D_PSD_DEFECT_CODES: readonly Anime25DPsdDefectKind[] = [
  'buried',
  'missing',
  'spurious',
  'mismatch',
]

/**
 * Reconciles the layers visible at rest against the illustration the PSD was
 * decomposed from. Both must share the PSD document space. Diagnosis only:
 * nothing here edits the layers.
 */
export function reconcileAnime25DPsd(
  layers: readonly RasterLayer[],
  reference: Readonly<Anime25DSourceReference>,
): Anime25DPsdReconciliation | null {
  const analysis = analyzeAnime25DPsd(layers, reference)
  return analysis && reportAnime25DPsdAnalysis(analysis, reference)
}

export interface Anime25DPsdAnalysisRegion extends Anime25DPsdDefectRegion {
  /** Indices into the analysis bounds. */
  members: number[]
}

export interface Anime25DPsdAnalysis {
  /** The whole portrait, where art may be missing. */
  bounds: ContentBounds
  /** The figure: its layers with a margin, and every region found; what the overview shows. */
  view: ContentBounds
  status: Anime25DPsdReconciliation['status']
  agreement: number
  compared: number
  background: { known: boolean; color: [number, number, number] }
  /** Composite colour and coverage of the analysed layers. */
  color: Float32Array
  alpha: Float32Array
  /** Index of the topmost opaque layer per pixel, or -1. */
  top: Int16Array
  /** For `buried` pixels, the covered layer that matches the source. */
  covered: Int16Array
  /** Backdrop connected to the open edges; null when it is not flat. */
  outside: Uint8Array | null
  /** Per-pixel verdict codes before small regions are discarded. */
  flagged: Uint8Array
  classes: Uint8Array
  /** Smallest region worth reporting or repairing. */
  minArea: number
  /** Where an earring may be (earAccessoryMasks), so regionFloor is the bare pixel floor. */
  ear: EarAccessoryMasks
  /** Every region above the size floor, largest first. */
  regions: Anime25DPsdAnalysisRegion[]
}

export function analyzeAnime25DPsd(
  layers: readonly RasterLayer[],
  reference: Readonly<Anime25DSourceReference>,
): Anime25DPsdAnalysis | null {
  const bounds = contentBounds(layers, reference)
  if (!bounds) return null
  const { x0, y0, width, height } = bounds
  const count = width * height
  const color = new Float32Array(count * 3)
  const alpha = new Float32Array(count)
  const top = new Int16Array(count).fill(-1)
  for (const [index, layer] of layers.entries()) {
    const startX = Math.max(x0, layer.left)
    const startY = Math.max(y0, layer.top)
    const endX = Math.min(x0 + width, layer.left + layer.width)
    const endY = Math.min(y0 + height, layer.top + layer.height)
    for (let y = startY; y < endY; y += 1) {
      for (let x = startX; x < endX; x += 1) {
        const source = ((y - layer.top) * layer.width + (x - layer.left)) * 4
        const layerAlpha = layer.data[source + 3] / 255
        if (layerAlpha === 0) continue
        const target = (y - y0) * width + (x - x0)
        const below = alpha[target]
        const combined = layerAlpha + below * (1 - layerAlpha)
        for (let channel = 0; channel < 3; channel += 1) {
          color[target * 3 + channel] =
            (layer.data[source + channel] * layerAlpha +
              color[target * 3 + channel] * below * (1 - layerAlpha)) /
            combined
        }
        alpha[target] = combined
        if (layerAlpha >= OPAQUE) top[target] = index
      }
    }
  }

  const background = estimateBackground(bounds, alpha, reference)
  const outside = background.known
    ? floodBackground(bounds, alpha, reference, background.color)
    : null
  const classes = new Uint8Array(count)
  const covered = new Int16Array(count).fill(-1)
  let compared = 0
  let matched = 0
  for (let target = 0; target < count; target += 1) {
    const x = x0 + (target % width)
    const y = y0 + Math.floor(target / width)
    const referenceOffset = (y * reference.width + x) * 4
    if (reference.data[referenceOffset + 3] < 250) continue
    const coverage = alpha[target]
    const distance = distanceOver(
      color,
      target,
      coverage,
      background.color,
      reference.data,
      referenceOffset,
    )
    if (coverage > 0.9) {
      compared += 1
      if (distance < MATCH_DISTANCE) matched += 1
    }
    if (coverage < OPAQUE) {
      if (outside && !outside[target] && distance > MATCH_DISTANCE) {
        classes[target] = 2
      }
      continue
    }
    if (outside?.[target] && distance > MATCH_DISTANCE) {
      classes[target] = 3
      continue
    }
    if (distance <= MISMATCH_DISTANCE) continue
    const match = coveredMatch(
      layers,
      top[target],
      x,
      y,
      reference.data,
      referenceOffset,
    )
    if (match >= 0) {
      classes[target] = 1
      covered[target] = match
    } else {
      classes[target] = 4
    }
  }

  const agreement = compared > 0 ? matched / compared : 0
  const status = agreement < MIN_AGREEMENT ? 'reference-mismatch' : 'reconciled'
  const minArea = Math.max(
    MIN_REGION_PIXELS,
    Math.round(compared * MIN_REGION_SHARE),
  )
  const flagged = classes.slice()
  const ear = earAccessoryMasks(layers, bounds)
  const floor = (members: readonly number[]) => regionFloor({ minArea, ear }, members)
  const regions =
    status === 'reconciled'
      ? collectRegions(classes, bounds, floor, (target, kind) => {
          const index = kind === 'buried' ? covered[target] : top[target]
          return index >= 0 ? layers[index].role : null
        })
      : []
  return {
    bounds,
    view: viewOf(layerBounds(layers, reference) ?? bounds, regions),
    status,
    agreement,
    compared,
    background,
    color,
    alpha,
    top,
    covered,
    outside,
    flagged,
    classes,
    minArea,
    ear,
    regions,
  }
}

/** The smallest a region of these pixels may be and still count: the bare floor for an earring, else minArea. */
export function regionFloor(
  analysis: Readonly<Pick<Anime25DPsdAnalysis, 'minArea' | 'ear'>>,
  members: readonly number[],
): number {
  const { onEarring, belowEar } = analysis.ear
  const earring = members.some((target) => onEarring[target] || belowEar[target])
  return earring ? MIN_REGION_PIXELS : analysis.minArea
}

/** `repaired` marks changed pixels in reference space for the overview. */
export function reportAnime25DPsdAnalysis(
  analysis: Readonly<Anime25DPsdAnalysis>,
  reference: Readonly<Anime25DSourceReference>,
  repairs: Anime25DPsdReconciliation['repairs'] = { revealed: 0, recovered: 0 },
  repaired: Uint8Array | null = null,
): Anime25DPsdReconciliation {
  const reconciled = analysis.status === 'reconciled'
  return {
    status: analysis.status,
    agreement: analysis.agreement,
    contentArea: analysis.compared,
    backgroundKnown: analysis.background.known,
    regions: analysis.regions
      .slice(0, MAX_REGIONS)
      .map(({ members: _members, ...region }) => region),
    repairs,
    heatmap: reconciled
      ? renderHeatmap(analysis.classes, analysis.bounds, analysis.view, reference, repaired)
      : null,
  }
}

export interface ContentBounds {
  x0: number
  y0: number
  width: number
  height: number
}

function contentBounds(
  layers: readonly RasterLayer[],
  reference: Readonly<Anime25DSourceReference>,
): ContentBounds | null {
  // The whole portrait: missing art can lie anywhere, just past every layer
  // (a dropped earring) or far below them (a skirt dropped whole, its hem
  // under the last layer that was drawn).
  if (layers.length === 0 || reference.width === 0 || reference.height === 0) return null
  return { x0: 0, y0: 0, width: reference.width, height: reference.height }
}

/** The layers' box with a margin: the figure as drawn. */
function layerBounds(
  layers: readonly RasterLayer[],
  reference: Readonly<Anime25DSourceReference>,
): ContentBounds | null {
  let left = reference.width
  let top = reference.height
  let right = 0
  let bottom = 0
  for (const layer of layers) {
    left = Math.min(left, layer.left)
    top = Math.min(top, layer.top)
    right = Math.max(right, layer.left + layer.width)
    bottom = Math.max(bottom, layer.top + layer.height)
  }
  const margin = Math.round(Math.max(right - left, bottom - top) * 0.04)
  const x0 = Math.max(0, Math.floor(left - margin))
  const y0 = Math.max(0, Math.floor(top - margin))
  const x1 = Math.min(reference.width, Math.ceil(right + margin))
  const y1 = Math.min(reference.height, Math.ceil(bottom + margin))
  return x1 > x0 && y1 > y0 ? { x0, y0, width: x1 - x0, height: y1 - y0 } : null
}

/** The figure's box grown to take in every region found. */
function viewOf(figure: ContentBounds, regions: readonly Anime25DPsdAnalysisRegion[]): ContentBounds {
  let x0 = figure.x0
  let y0 = figure.y0
  let x1 = figure.x0 + figure.width
  let y1 = figure.y0 + figure.height
  for (const { box } of regions) {
    x0 = Math.min(x0, box.x)
    y0 = Math.min(y0, box.y)
    x1 = Math.max(x1, box.x + box.width)
    y1 = Math.max(y1, box.y + box.height)
  }
  return { x0, y0, width: x1 - x0, height: y1 - y0 }
}

/** Flat studio backgrounds only; a scenic one cannot separate art from backdrop. */
function estimateBackground(
  bounds: ContentBounds,
  alpha: Float32Array,
  reference: Readonly<Anime25DSourceReference>,
): { known: boolean; color: [number, number, number] } {
  const channels: [number[], number[], number[]] = [[], [], []]
  const step = Math.max(1, Math.floor(Math.sqrt(alpha.length / 20000)))
  for (let y = 0; y < bounds.height; y += step) {
    for (let x = 0; x < bounds.width; x += step) {
      if (alpha[y * bounds.width + x] > 0) continue
      const offset = ((bounds.y0 + y) * reference.width + (bounds.x0 + x)) * 4
      if (reference.data[offset + 3] < 250) continue
      for (let channel = 0; channel < 3; channel += 1) {
        channels[channel].push(reference.data[offset + channel])
      }
    }
  }
  if (channels[0].length < MIN_BACKGROUND_SAMPLES) {
    return { known: false, color: [255, 255, 255] }
  }
  const color = channels.map(median) as [number, number, number]
  const spread = median(
    channels[0].map((_, index) =>
      Math.hypot(
        channels[0][index] - color[0],
        channels[1][index] - color[1],
        channels[2][index] - color[2],
      ),
    ),
  )
  return { known: spread <= MAX_BACKGROUND_SPREAD, color }
}

/**
 * Background is what connects to the open backdrop. Pale skin or white hair
 * matches the backdrop colour too, but line art encloses it.
 */
function floodBackground(
  bounds: ContentBounds,
  alpha: Float32Array,
  reference: Readonly<Anime25DSourceReference>,
  color: readonly [number, number, number],
): Uint8Array {
  const { width, height } = bounds
  const outside = new Uint8Array(width * height)
  const backgroundLike = (target: number) => {
    const offset =
      ((bounds.y0 + Math.floor(target / width)) * reference.width +
        bounds.x0 +
        (target % width)) *
      4
    return (
      reference.data[offset + 3] >= 250 &&
      rgbDistance(reference.data, offset, color) < BACKGROUND_DISTANCE
    )
  }
  const stack: number[] = []
  for (let target = 0; target < outside.length; target += 1) {
    const x = target % width
    const y = Math.floor(target / width)
    const edge = x === 0 || y === 0 || x === width - 1 || y === height - 1
    if ((edge || alpha[target] === 0) && backgroundLike(target)) {
      outside[target] = 1
      stack.push(target)
    }
  }
  while (stack.length > 0) {
    const target = stack.pop()!
    const x = target % width
    const y = Math.floor(target / width)
    for (const next of [
      x > 0 ? target - 1 : -1,
      x < width - 1 ? target + 1 : -1,
      y > 0 ? target - width : -1,
      y < height - 1 ? target + width : -1,
    ]) {
      if (next < 0 || outside[next] || !backgroundLike(next)) continue
      outside[next] = 1
      stack.push(next)
    }
  }
  return outside
}

function coveredMatch(
  layers: readonly RasterLayer[],
  topIndex: number,
  x: number,
  y: number,
  reference: Uint8ClampedArray,
  referenceOffset: number,
): number {
  for (let index = topIndex - 1; index >= 0; index -= 1) {
    const layer = layers[index]
    const localX = x - layer.left
    const localY = y - layer.top
    if (
      localX < 0 ||
      localY < 0 ||
      localX >= layer.width ||
      localY >= layer.height
    ) {
      continue
    }
    const offset = (localY * layer.width + localX) * 4
    if (layer.data[offset + 3] < 128) continue
    const distance = Math.hypot(
      layer.data[offset] - reference[referenceOffset],
      layer.data[offset + 1] - reference[referenceOffset + 1],
      layer.data[offset + 2] - reference[referenceOffset + 2],
    )
    if (distance < MATCH_DISTANCE) return index
  }
  return -1
}

function collectRegions(
  classes: Uint8Array,
  bounds: ContentBounds,
  floor: (members: readonly number[]) => number,
  roleAt: (target: number, kind: Anime25DPsdDefectKind) => string | null,
): Anime25DPsdAnalysisRegion[] {
  const { width, height } = bounds
  const visited = new Uint8Array(classes.length)
  const stack: number[] = []
  const regions: Anime25DPsdAnalysisRegion[] = []
  for (let start = 0; start < classes.length; start += 1) {
    const code = classes[start]
    if (!code || visited[start]) continue
    const kind = ANIME25D_PSD_DEFECT_CODES[code - 1]
    const roles = new Map<string | null, number>()
    let area = 0
    let minX = width
    let minY = height
    let maxX = 0
    let maxY = 0
    visited[start] = 1
    stack.push(start)
    const members: number[] = []
    while (stack.length > 0) {
      const target = stack.pop()!
      members.push(target)
      const x = target % width
      const y = Math.floor(target / width)
      area += 1
      minX = Math.min(minX, x)
      minY = Math.min(minY, y)
      maxX = Math.max(maxX, x)
      maxY = Math.max(maxY, y)
      const role = roleAt(target, kind)
      roles.set(role, (roles.get(role) ?? 0) + 1)
      if (x > 0) visit(target - 1)
      if (x < width - 1) visit(target + 1)
      if (y > 0) visit(target - width)
      if (y < height - 1) visit(target + width)
    }
    if (area < floor(members)) {
      for (const member of members) classes[member] = 0
      continue
    }
    let role: string | null = null
    let best = 0
    for (const [candidate, votes] of roles) {
      if (votes > best) {
        best = votes
        role = candidate
      }
    }
    regions.push({
      kind,
      area,
      box: {
        x: bounds.x0 + minX,
        y: bounds.y0 + minY,
        width: maxX - minX + 1,
        height: maxY - minY + 1,
      },
      role,
      members,
    })

    function visit(next: number) {
      if (visited[next] || classes[next] !== code) return
      visited[next] = 1
      stack.push(next)
    }
  }
  return regions.toSorted((left, right) => right.area - left.area)
}

function renderHeatmap(
  classes: Uint8Array,
  bounds: ContentBounds,
  view: ContentBounds,
  reference: Readonly<Anime25DSourceReference>,
  repaired: Uint8Array | null,
): Anime25DPsdReconciliation['heatmap'] {
  // Show the figure and what was found, of the illustration itself, not the
  // square padding around it.
  let left = bounds.width
  let top = bounds.height
  let right = 0
  let bottom = 0
  const fromY = Math.max(0, view.y0 - bounds.y0)
  const toY = Math.min(bounds.height, view.y0 + view.height - bounds.y0)
  const fromX = Math.max(0, view.x0 - bounds.x0)
  const toX = Math.min(bounds.width, view.x0 + view.width - bounds.x0)
  for (let y = fromY; y < toY; y += 1) {
    for (let x = fromX; x < toX; x += 1) {
      const offset = ((bounds.y0 + y) * reference.width + (bounds.x0 + x)) * 4
      if (reference.data[offset + 3] < 250) continue
      left = Math.min(left, x)
      top = Math.min(top, y)
      right = Math.max(right, x + 1)
      bottom = Math.max(bottom, y + 1)
    }
  }
  if (right <= left || bottom <= top) return null
  const viewWidth = right - left
  const viewHeight = bottom - top
  const scale = Math.min(1, HEATMAP_EDGE / Math.max(viewWidth, viewHeight))
  const width = Math.max(1, Math.round(viewWidth * scale))
  const height = Math.max(1, Math.round(viewHeight * scale))
  const data = new Uint8ClampedArray(width * height * 4)
  for (let y = 0; y < height; y += 1) {
    const fromY = top + Math.floor(y / scale)
    const toY = Math.max(
      fromY + 1,
      Math.min(bottom, top + Math.floor((y + 1) / scale)),
    )
    for (let x = 0; x < width; x += 1) {
      const fromX = left + Math.floor(x / scale)
      const toX = Math.max(
        fromX + 1,
        Math.min(right, left + Math.floor((x + 1) / scale)),
      )
      let code = 0
      let fixed = false
      for (let sy = fromY; sy < toY && !code; sy += 1) {
        for (let sx = fromX; sx < toX && !code; sx += 1) {
          code = classes[sy * bounds.width + sx]
          fixed ||= Boolean(
            repaired?.[(bounds.y0 + sy) * reference.width + bounds.x0 + sx],
          )
        }
      }
      const target = (y * width + x) * 4
      if (code) {
        data.set(
          ANIME25D_PSD_DEFECT_COLORS[ANIME25D_PSD_DEFECT_CODES[code - 1]],
          target,
        )
      } else if (fixed) {
        data.set(ANIME25D_PSD_REPAIRED_COLOR, target)
      } else {
        const offset =
          ((bounds.y0 + fromY) * reference.width + (bounds.x0 + fromX)) * 4
        const shade =
          ((reference.data[offset] +
            reference.data[offset + 1] +
            reference.data[offset + 2]) /
            3) *
            0.55 +
          60
        data[target] = shade
        data[target + 1] = shade
        data[target + 2] = shade
      }
      data[target + 3] = 255
    }
  }
  return { width, height, data }
}

function distanceOver(
  color: Float32Array,
  target: number,
  coverage: number,
  background: readonly [number, number, number],
  reference: Uint8ClampedArray,
  referenceOffset: number,
): number {
  let sum = 0
  for (let channel = 0; channel < 3; channel += 1) {
    const composite =
      color[target * 3 + channel] * coverage +
      background[channel] * (1 - coverage)
    const difference = composite - reference[referenceOffset + channel]
    sum += difference * difference
  }
  return Math.sqrt(sum)
}

export function rgbDistance(
  data: Uint8ClampedArray,
  offset: number,
  color: readonly [number, number, number],
): number {
  return Math.hypot(
    data[offset] - color[0],
    data[offset + 1] - color[1],
    data[offset + 2] - color[2],
  )
}

function median(values: number[]): number {
  const sorted = values.toSorted((left, right) => left - right)
  return sorted[sorted.length >> 1] ?? 0
}
