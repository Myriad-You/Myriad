import type {
  Anime25DPlaybackLayer,
  Anime25DShellCurvePoint,
  Anime25DShellEllipsoid,
  Anime25DShellProfile,
} from './types'

export type Anime25DShellMode = 'head' | 'front-hair' | 'back-hair'

const SHELL_YAW_RADIANS = 0.45
const SHELL_PITCH_RADIANS = 0.32
const FULL_CROWN_TURN = 1 - Math.cos(SHELL_YAW_RADIANS) * Math.cos(SHELL_PITCH_RADIANS)

export function anime25DShellModeForLayer(
  source: Pick<Anime25DPlaybackLayer, 'group' | 'role'>,
): Anime25DShellMode | null {
  if (source.group !== 'head') return null
  if (source.role === 'back-hair') return 'back-hair'
  if (source.role === 'front-hair' || source.role === 'headwear') {
    return 'front-hair'
  }
  return 'head'
}

export interface Anime25DShellRotation {
  active: boolean
  yawCosine: number
  yawSine: number
  pitchCosine: number
  pitchSine: number
}

export interface Anime25DMutableShellPoint {
  x: number
  y: number
}

export function writeAnime25DShellRotation(
  angleX: number,
  angleY: number,
  target: Anime25DShellRotation,
): void {
  const yaw = angleX * SHELL_YAW_RADIANS
  const pitch = angleY * SHELL_PITCH_RADIANS
  target.active = angleX !== 0 || angleY !== 0
  target.yawCosine = Math.cos(yaw)
  target.yawSine = Math.sin(yaw)
  target.pitchCosine = Math.cos(pitch)
  target.pitchSine = Math.sin(pitch)
}

export function anime25DHairlinePinWeight(
  x: number,
  y: number,
  profile: Readonly<Anime25DShellProfile>,
): number {
  const pin = profile.hair.hairlinePin
  if (!profile.enabled || !pin.enabled) return 0
  const head = profile.head
  const centerX = head.centerX + pin.centerX * head.radiusX
  const centerY = head.centerY + pin.centerY * head.radiusY
  const outsideX =
    Math.max(0, Math.abs(x - centerX) - pin.halfWidth * head.radiusX) /
    head.radiusX
  const outsideY =
    Math.max(0, Math.abs(y - centerY) - pin.halfHeight * head.radiusY) /
    head.radiusY
  const outside = Math.hypot(outsideX, outsideY)
  if (pin.feather <= 0.001) return outside <= 1e-6 ? 1 : 0
  return 1 - smoothstep(outside / pin.feather)
}

/** Authored profiles preserve the fork's calibrated rectangle */
export function sampleAnime25DHairlinePinWeights(
  rest: Float32Array,
  source: Readonly<Anime25DPlaybackLayer>,
  rows: number,
  profile: Readonly<Anime25DShellProfile>,
): Float32Array | null {
  const pin = profile.hair.hairlinePin
  if (!profile.enabled || !pin.enabled) return null
  const mode =
    pin.mode ??
    (profile.source === 'anchor-derived' ? 'strand-roots' : 'rectangle')
  const weights = new Float32Array(rest.length / 2)
  let hasWeight = false
  if (mode === 'rectangle') {
    for (let vertex = 0; vertex < weights.length; vertex += 1) {
      const weight = anime25DHairlinePinWeight(
        rest[vertex * 2],
        rest[vertex * 2 + 1],
        profile,
      )
      weights[vertex] = weight
      if (weight > 0) hasWeight = true
    }
    return hasWeight ? weights : null
  }

  const strands = Iterator.from(source.strands).toArray()
    .filter(
      (strand) => Number.isFinite(strand.x) && Number.isFinite(strand.rootY),
    )
    .toSorted((left, right) => left.x - right.x)
  const spacing = medianPositiveStrandGap(strands, source.w)
  const sigma = Math.max(1, spacing * 0.6)
  const releaseDistance = Math.max(1, (source.h / Math.max(1, rows)) * 2)
  const fallbackRootY = profile.head.centerY - profile.head.radiusY * 0.45
  for (let vertex = 0; vertex < weights.length; vertex += 1) {
    const x = rest[vertex * 2]
    const y = rest[vertex * 2 + 1]
    const rootY = interpolatedStrandRootY(x, strands, sigma, fallbackRootY)
    const weight = 1 - smoothstep((y - rootY) / releaseDistance)
    weights[vertex] = weight
    if (weight > 0) hasWeight = true
  }
  return hasWeight ? weights : null
}

function medianPositiveStrandGap(
  strands: readonly Anime25DPlaybackLayer['strands'][number][],
  layerWidth: number,
): number {
  const gaps: number[] = []
  for (let index = 1; index < strands.length; index += 1) {
    const gap = strands[index].x - strands[index - 1].x
    if (gap > 0) gaps.push(gap)
  }
  if (gaps.length === 0) {
    return Math.max(1, layerWidth / Math.max(1, strands.length))
  }
  const sorted = gaps.toSorted((left, right) => left - right)
  return sorted[sorted.length >> 1]
}

function interpolatedStrandRootY(
  x: number,
  strands: readonly Anime25DPlaybackLayer['strands'][number][],
  sigma: number,
  fallback: number,
): number {
  let total = 0
  let rootY = 0
  let nearestDistance = Number.POSITIVE_INFINITY
  let nearestRootY = fallback
  for (const strand of strands) {
    const distance = Math.abs(x - strand.x)
    if (distance < nearestDistance) {
      nearestDistance = distance
      nearestRootY = strand.rootY
    }
    const weight = Math.exp(-((distance / sigma) ** 2))
    total += weight
    rootY += strand.rootY * weight
  }
  return total > 1e-6 ? rootY / total : nearestRootY
}

/** Applies only the shell projection delta. */
export function deformAnime25DShellPoint(
  point: Anime25DMutableShellPoint,
  restY: number,
  mode: Anime25DShellMode,
  profile: Readonly<Anime25DShellProfile>,
  rotation: Readonly<Anime25DShellRotation>,
  depth: number,
): void {
  if (!profile.enabled || profile.blend <= 0 || !rotation.active) {
    return
  }
  const ellipsoid = mode === 'head' ? profile.head : profile.hair
  const normalizedX = (point.x - ellipsoid.centerX) / ellipsoid.radiusX
  const normalizedY = (point.y - ellipsoid.centerY) / ellipsoid.radiusY
  const radialDepth = Math.sqrt(
    Math.max(0, 1 - Math.min(1, normalizedX ** 2 + normalizedY ** 2)),
  )
  const depthOffset = depth - 1
  let normalizedDepth: number
  if (mode === 'front-hair') {
    normalizedDepth =
      profile.hair.frontBulge * radialDepth + profile.hair.frontGap
  } else if (mode === 'back-hair') {
    normalizedDepth = -profile.hair.backDepth * radialDepth - 0.05
  } else if (profile.faceProfile.enabled) {
    // Eye/jaw geometry has already moved locally. Depth is a field on that
    // resulting face, not a different field for each vertex's original row.
    const vertical =
      (point.y - profile.faceProfile.startY) /
      Math.max(1, profile.faceProfile.endY - profile.faceProfile.startY)
    const profileDepth = evaluateCurve(profile.faceProfile.points, vertical)
    const profileWidth = Math.max(1, profile.head.radiusX * 0.34)
    const profileX = (point.x - profile.head.centerX) / profileWidth
    normalizedDepth =
      radialDepth +
      profileDepth * Math.exp(-(profileX * profileX)) +
      depthOffset * 0.55
  } else {
    normalizedDepth = radialDepth + depthOffset * 0.55
  }

  const shellX = point.x
  applyProjectionDelta(point, ellipsoid, normalizedDepth, rotation)

  // A pinned root keeps the coiffure's depth and follows the same head rotation.
  // Pin weights suppress relative bang/spring motion, not this rest surface.
  if (mode === 'front-hair' && profile.hair.crownRound > 0) {
    const crownStart = profile.head.centerY - profile.head.radiusY * 0.18
    const crownSpan = Math.max(1, profile.head.radiusY * 0.62)
    const crown =
      smoothstep((crownStart - restY) / crownSpan) *
      Math.sqrt(Math.max(0, 1 - normalizedX * normalizedX))
    // A pose-space correction must vanish continuously at the reference pose.
    // Use orientation departure, not time smoothing or a nonzero-angle switch.
    const crownMix = clamp(
      profile.hair.crownRound * crown * 0.3 *
        smoothstep((1 - rotation.yawCosine * rotation.pitchCosine) / FULL_CROWN_TURN),
      0,
      0.6,
    )
    const crownX = profile.head.centerX + (shellX - profile.head.centerX) * 0.55
    const crownY = profile.head.centerY - profile.head.radiusY * 0.88
    point.x += (crownX - point.x) * crownMix
    point.y += (crownY - point.y) * crownMix
  }
}

function applyProjectionDelta(
  point: Anime25DMutableShellPoint,
  ellipsoid: Readonly<Anime25DShellEllipsoid>,
  normalizedDepth: number,
  rotation: Readonly<Anime25DShellRotation>,
): void {
  const x = point.x - ellipsoid.centerX
  const y = point.y - ellipsoid.centerY
  const z = normalizedDepth * ellipsoid.radiusZ
  const yawedX = x * rotation.yawCosine + z * rotation.yawSine
  const yawedZ = -x * rotation.yawSine + z * rotation.yawCosine
  const pitchedY = y * rotation.pitchCosine - yawedZ * rotation.pitchSine
  const pitchedZ = y * rotation.pitchSine + yawedZ * rotation.pitchCosine
  const focalLength = Math.max(1, ellipsoid.radiusZ * 5)
  const rotatedScale = focalLength / Math.max(1, focalLength - pitchedZ * 0.5)
  const restScale = focalLength / Math.max(1, focalLength - z * 0.5)
  point.x += yawedX * rotatedScale - x * restScale
  point.y += pitchedY * rotatedScale - y * restScale
}

function evaluateCurve(
  points: readonly Anime25DShellCurvePoint[],
  rawProgress: number,
): number {
  const progress = clamp(rawProgress, 0, 1)
  if (progress <= points[0].v) return points[0].z
  if (progress >= points.at(-1)!.v) return points.at(-1)!.z
  let index = 0
  while (index < points.length - 2 && progress > points[index + 1].v) {
    index += 1
  }
  const first = points[index]
  const second = points[index + 1]
  const span = Math.max(1e-6, second.v - first.v)
  const t = (progress - first.v) / span
  const squared = t * t
  const cubed = squared * t
  const firstSlope = faceCurveSlope(points, index) * span
  const secondSlope = faceCurveSlope(points, index + 1) * span
  return (2 * cubed - 3 * squared + 1) * first.z +
    (cubed - 2 * squared + t) * firstSlope +
    (-2 * cubed + 3 * squared) * second.z +
    (cubed - squared) * secondSlope
}

/** Shape-preserving nonuniform Hermite slopes (Fritsch–Butland, doi:10.1137/0905021). */
function faceCurveSlope(
  points: readonly Anime25DShellCurvePoint[],
  index: number,
): number {
  // Unlike extrapolating PCHIP, this depth field is constant outside its
  // landmarks. Zero end tangents make that extension slope-continuous too.
  if (index === 0 || index === points.length - 1) return 0
  const before = points[index - 1]
  const point = points[index]
  const after = points[index + 1]
  const leftSpan = Math.max(1e-6, point.v - before.v)
  const rightSpan = Math.max(1e-6, after.v - point.v)
  const leftSlope = (point.z - before.z) / leftSpan
  const rightSlope = (after.z - point.z) / rightSpan
  if (leftSlope * rightSlope <= 0) return 0
  const leftWeight = 2 * rightSpan + leftSpan
  const rightWeight = rightSpan + 2 * leftSpan
  return (leftWeight + rightWeight) / (leftWeight / leftSlope + rightWeight / rightSlope)
}

function smoothstep(value: number): number {
  const bounded = clamp(value, 0, 1)
  return bounded * bounded * (3 - 2 * bounded)
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
