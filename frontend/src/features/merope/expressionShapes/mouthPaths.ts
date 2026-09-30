import type { MouthExpressionKind, Point, Rgb } from './mouthShared'
import { clamp } from './mouthShared'

/** The silhouette each generated mouth is drawn in; other painters reuse it so morphs match. */
export function mouthExpressionOuterPath(kind: MouthExpressionKind): readonly Point[] {
  return mouthOuterPath(kind)
}

/**
 * A consonant's narrow mouth is the open mouth drawn flatter, cavity and tongue
 * included, so speech that passes through it never swaps to unrelated art.
 */
export function mouthOuterPath(kind: MouthExpressionKind): Point[] {
  if (kind === 'wide') return wideOuterPath()
  if (kind === 'round') return roundOuterPath()
  if (kind === 'cry') return cryOuterPath()
  if (kind === 'maniac') return maniacOuterPath()
  if (kind === 'silly') return sillyOuterPath()
  return openOuterPath()
}

function openOuterPath(): Point[] {
  return [
    [-0.2, -0.82],
    [0.22, -0.8],
    [0.54, -0.62],
    [0.7, -0.27],
    [0.67, 0.2],
    [0.48, 0.58],
    [0.15, 0.78],
    [-0.2, 0.76],
    [-0.5, 0.56],
    [-0.67, 0.19],
    [-0.68, -0.28],
    [-0.51, -0.63],
  ]
}

function wideOuterPath(): Point[] {
  return [
    [-0.93, -0.06],
    [-0.76, -0.36],
    [-0.4, -0.53],
    [0, -0.55],
    [0.4, -0.53],
    [0.76, -0.36],
    [0.93, -0.06],
    [0.8, 0.26],
    [0.4, 0.46],
    [0, 0.5],
    [-0.4, 0.46],
    [-0.8, 0.26],
  ]
}

function roundOuterPath(): Point[] {
  return [
    [-0.4, -0.77],
    [-0.08, -0.91],
    [0.28, -0.79],
    [0.53, -0.51],
    [0.62, -0.08],
    [0.55, 0.38],
    [0.3, 0.72],
    [-0.04, 0.86],
    [-0.36, 0.72],
    [-0.58, 0.39],
    [-0.62, -0.06],
    [-0.57, -0.46],
  ]
}

function cryOuterPath(): Point[] {
  return [
    [-0.88, -0.18],
    [-0.78, -0.48],
    [-0.51, -0.61],
    [-0.25, -0.45],
    [0, -0.23],
    [0.24, -0.46],
    [0.53, -0.59],
    [0.8, -0.46],
    [0.9, -0.14],
    [0.84, 0.25],
    [0.61, 0.54],
    [0.26, 0.66],
    [-0.08, 0.69],
    [-0.43, 0.63],
    [-0.73, 0.48],
    [-0.89, 0.18],
  ]
}

function maniacOuterPath(): Point[] {
  const output: Point[] = []
  appendCubic(
    output,
    [-0.82, -0.62],
    [-0.5, -0.79],
    [0.5, -0.79],
    [0.82, -0.62],
  )
  appendCubic(output, [0.82, -0.62], [0.85, -0.18], [0.75, 0.45], [0.5, 0.72])
  appendCubic(output, [0.5, 0.72], [0.31, 1.05], [-0.31, 1.05], [-0.5, 0.72])
  appendCubic(
    output,
    [-0.5, 0.72],
    [-0.75, 0.45],
    [-0.85, -0.18],
    [-0.82, -0.62],
  )
  return output
}

function sillyOuterPath(): Point[] {
  const output: Point[] = []
  appendCubic(output, [-0.72, -0.36], [-0.5, -0.58], [-0.2, -0.48], [0, -0.38])
  appendCubic(output, [0, -0.38], [0.2, -0.5], [0.5, -0.58], [0.72, -0.34])
  appendCubic(output, [0.72, -0.34], [0.68, 0.2], [0.42, 0.68], [0, 0.79])
  appendCubic(output, [0, 0.79], [-0.42, 0.67], [-0.68, 0.2], [-0.72, -0.36])
  return output
}

function appendCubic(
  output: Point[],
  from: Point,
  controlA: Point,
  controlB: Point,
  to: Point,
): void {
  if (output.length === 0) output.push(from)
  for (let step = 1; step <= 7; step += 1) {
    const t = step / 7
    const inverse = 1 - t
    const fromWeight = inverse * inverse * inverse
    const controlAWeight = 3 * inverse * inverse * t
    const controlBWeight = 3 * inverse * t * t
    const toWeight = t * t * t
    output.push([
      from[0] * fromWeight +
        controlA[0] * controlAWeight +
        controlB[0] * controlBWeight +
        to[0] * toWeight,
      from[1] * fromWeight +
        controlA[1] * controlAWeight +
        controlB[1] * controlBWeight +
        to[1] * toWeight,
    ])
  }
}

export function insetPath(
  path: readonly Point[],
  scaleX: number,
  scaleY: number,
  offsetY: number,
): Point[] {
  return path.map(([x, y]) => [x * scaleX, y * scaleY + offsetY])
}

export function pointInPolygon(
  x: number,
  y: number,
  polygon: readonly Point[],
): boolean {
  let inside = false
  for (
    let index = 0, previous = polygon.length - 1;
    index < polygon.length;
    previous = index++
  ) {
    const currentPoint = polygon[index]
    const previousPoint = polygon[previous]
    const crosses =
      currentPoint[1] > y !== previousPoint[1] > y &&
      x <
        ((previousPoint[0] - currentPoint[0]) * (y - currentPoint[1])) /
          (previousPoint[1] - currentPoint[1]) +
          currentPoint[0]
    if (crosses) inside = !inside
  }
  return inside
}

export function pointToPolylineDistance(
  x: number,
  y: number,
  points: readonly Point[],
): number {
  let distance = Number.POSITIVE_INFINITY
  for (let index = 1; index < points.length; index += 1) {
    distance = Math.min(
      distance,
      pointToSegmentDistance(x, y, points[index - 1], points[index]),
    )
  }
  return distance
}

function pointToSegmentDistance(
  x: number,
  y: number,
  start: Point,
  end: Point,
): number {
  const deltaX = end[0] - start[0]
  const deltaY = end[1] - start[1]
  const lengthSquared = deltaX * deltaX + deltaY * deltaY
  const progress =
    lengthSquared > 0
      ? clamp(
          ((x - start[0]) * deltaX + (y - start[1]) * deltaY) / lengthSquared,
          0,
          1,
        )
      : 0
  return Math.hypot(
    x - (start[0] + deltaX * progress),
    y - (start[1] + deltaY * progress),
  )
}

export function paint(
  data: Uint8ClampedArray,
  offset: number,
  color: Readonly<Rgb>,
  alpha: number,
): void {
  const sourceAlpha = clamp(alpha, 0, 1)
  const destinationAlpha = data[offset + 3] / 255
  const outputAlpha = sourceAlpha + destinationAlpha * (1 - sourceAlpha)
  if (outputAlpha <= 0) return
  const retained = destinationAlpha * (1 - sourceAlpha)
  data[offset] = Math.round(
    (color.red * sourceAlpha + data[offset] * retained) / outputAlpha,
  )
  data[offset + 1] = Math.round(
    (color.green * sourceAlpha + data[offset + 1] * retained) / outputAlpha,
  )
  data[offset + 2] = Math.round(
    (color.blue * sourceAlpha + data[offset + 2] * retained) / outputAlpha,
  )
  data[offset + 3] = Math.round(outputAlpha * 255)
}
