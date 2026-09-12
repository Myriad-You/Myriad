import type { AttachmentMesh } from './attachmentMesh'
import type { Anime25DPlaybackLayer } from './types'
import { localToAtlasUv } from './atlasUv'

type Point = [number, number]
type Triangle = [Point, Point, Point]

/** Compile a rectangular drawing onto its host triangles, preserving its full coverage. */
export function buildContactSurfaceMesh(
  host: Pick<AttachmentMesh, 'rest' | 'indices'>,
  source: Pick<Anime25DPlaybackLayer, 'x' | 'y' | 'w' | 'h' | 'atlas'>,
): { rest: Float32Array; atlasUvs: Float32Array; indices: Uint16Array } | null {
  if (
    ![source.x, source.y, source.w, source.h, source.x + source.w, source.y + source.h].every(Number.isFinite) ||
    source.w <= 0 || source.h <= 0
  ) {
    return null
  }
  if (
    host.rest.length % 2 || host.indices.length % 3 || !host.rest.every(Number.isFinite) ||
    !host.indices.every(index => index < host.rest.length / 2)
  ) {
    return null
  }
  const triangles: Triangle[] = []
  let coveredArea = 0
  let largestEdge = 0
  for (let i = 0; i < host.indices.length; i += 3) {
    let polygon: Point[] = Iterator.from(host.indices.slice(i, i + 3))
      .map((vertex) => [host.rest[vertex * 2], host.rest[vertex * 2 + 1]] as Point)
      .toArray()
    polygon = clip(polygon, 0, source.x, true)
    polygon = clip(polygon, 0, source.x + source.w, false)
    polygon = clip(polygon, 1, source.y, true)
    polygon = clip(polygon, 1, source.y + source.h, false)
    for (let n = 1; n + 1 < polygon.length; n++) {
      const triangle: Triangle = [polygon[0], polygon[n], polygon[n + 1]]
      const area = signedArea(triangle)
      if (Math.abs(area) < 1e-7) continue
      if (area < 0) [triangle[1], triangle[2]] = [triangle[2], triangle[1]]
      coveredArea += Math.abs(area)
      for (let edge = 0; edge < 3; edge++) {
        const a = triangle[edge]
        const b = triangle[(edge + 1) % 3]
        largestEdge = Math.max(largestEdge, Math.hypot(a[0] - b[0], a[1] - b[1]))
      }
      triangles.push(triangle)
    }
  }
  // A host that does not cover this drawing cannot replace its mesh. In
  // particular, never trim a protruding sleeve to the torso's rectangle.
  const fullArea = source.w * source.h
  if (!(fullArea > 0) || !Number.isFinite(fullArea) || Math.abs(coveredArea - fullArea) > fullArea * 1e-5) return null

  // Same subdivision level on every face keeps shared edges conforming. Only
  // evidence-backed contacts reach this compiler; ordinary layers keep their grid.
  const levels = Math.max(0, Math.ceil(Math.log2(largestEdge / Math.max(1, source.w * 0.12))))
  if (triangles.length * 4 ** levels * 3 > 65535) return null
  const vertices: number[] = []
  const indices: number[] = []
  const vertexIds = new Map<string, number>()
  const append = (triangle: Triangle, remaining: number): void => {
    if (remaining > 0) {
      const [a, b, c] = triangle
      const ab = midpoint(a, b)
      const bc = midpoint(b, c)
      const ca = midpoint(c, a)
      append([a, ab, ca], remaining - 1)
      append([ab, b, bc], remaining - 1)
      append([ca, bc, c], remaining - 1)
      append([ab, bc, ca], remaining - 1)
      return
    }
    for (const [x, y] of triangle) {
      const fx = Math.fround(x)
      const fy = Math.fround(y)
      const key = `${fx}:${fy}`
      let id = vertexIds.get(key)
      if (id === undefined) {
        id = vertices.length / 2
        vertices.push(fx, fy)
        vertexIds.set(key, id)
      }
      indices.push(id)
    }
  }
  for (const triangle of triangles) append(triangle, levels)
  const rest = new Float32Array(vertices)
  const atlasUvs = new Float32Array(rest.length)
  for (let i = 0; i < rest.length; i += 2) {
    const [u, v] = localToAtlasUv(source.atlas, (rest[i] - source.x) / source.w, (rest[i + 1] - source.y) / source.h)
    atlasUvs[i] = u
    atlasUvs[i + 1] = v
  }
  return { rest, atlasUvs, indices: new Uint16Array(indices) }
}

function clip(input: Point[], axis: 0 | 1, bound: number, above: boolean): Point[] {
  const output: Point[] = []
  for (let i = 0; i < input.length; i++) {
    const a = input[i]
    const b = input[(i + 1) % input.length]
    const insideA = above ? a[axis] >= bound : a[axis] <= bound
    const insideB = above ? b[axis] >= bound : b[axis] <= bound
    if (insideA) output.push(a)
    if (insideA !== insideB) {
      const t = (bound - a[axis]) / (b[axis] - a[axis])
      const point: Point = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
      point[axis] = bound
      output.push(point)
    }
  }
  return output
}

function midpoint(a: Point, b: Point): Point {
  return [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2]
}

function signedArea([a, b, c]: Triangle): number {
  return ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])) / 2
}
