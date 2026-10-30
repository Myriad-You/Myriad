const MIN_SECONDARY_AREA_RATIO = 0.2
// Allow painted locks some give, but not elastic-strip elongation. This is an
// art-directed 2D tolerance relative to the current projected pose, not a
// physical hair material constant or a limit on overall movement.
const MAX_SECONDARY_EDGE_RATIO = 1.25
// Shared vertices can re-compress a neighbour corrected earlier in the pass.
// Dense long-hair meshes need more propagation than a single strand; safe
// surfaces still exit on the first pass without changing their amplitude.
const MAX_PROJECTION_PASSES = 32

/** Local shape constraints after existing hair motion; not a second physics clock. */
export interface HairSurface {
  base: Float32Array
  candidate: Float32Array
  mobility: Float32Array
  indices: Uint16Array
  edges: Uint16Array
  maxEdgeLengths: Float32Array
}

export function bindHairSurface(
  rest: Float32Array,
  indices: Uint16Array,
  along: Float32Array,
  pins: Float32Array | null,
): HairSurface {
  const edges: number[] = []
  const seen = new Set<number>()
  for (let t = 0; t < indices.length; t += 3) {
    for (let k = 0; k < 3; k++) {
      const a = Math.min(indices[t + k], indices[t + (k + 1) % 3])
      const b = Math.max(indices[t + k], indices[t + (k + 1) % 3])
      const key = a * 65536 + b
      if (a === b || seen.has(key)) continue
      seen.add(key)
      edges.push(a, b)
    }
  }
  return {
    base: rest.slice(),
    candidate: rest.slice(),
    mobility: along.map((v, i) => v * (1 - (pins?.[i] ?? 0))),
    indices,
    edges: new Uint16Array(edges),
    maxEdgeLengths: new Float32Array(edges.length / 2),
  }
}

/**
 * Weighted position projection: Müller et al., doi:10.1016/j.jvcir.2007.01.005.
 * Only secondary-motion compression/overstretch is corrected. The reference is THIS frame's
 * head-projected surface, not the neutral drawing. Roots with zero mobility stay put.
 */
export function constrainHairSurface(surface: HairSurface): void {
  const { base, candidate: p, mobility: w, indices, edges, maxEdgeLengths } = surface
  for (let e = 0; e < edges.length; e += 2) {
    const a = edges[e] * 2; const b = edges[e + 1] * 2
    maxEdgeLengths[e / 2] = Math.hypot(base[b] - base[a], base[b + 1] - base[a + 1]) * MAX_SECONDARY_EDGE_RATIO
  }
  // Length projection can re-compress neighbouring triangles. In strongly
  // tangled inputs finish with orientation-only relaxation: preserving topology
  // takes priority over the soft 2D stretch tolerance. Ordinary poses exit early.
  for (let pass = 0; pass < MAX_PROJECTION_PASSES * 2; pass++) {
    let corrected = false
    const edgeCount = pass < MAX_PROJECTION_PASSES ? edges.length : 0
    for (let n = 0; n < edgeCount; n += 2) {
      const e = pass % 2 ? edges.length - 2 - n : n
      const a = edges[e] * 2; const b = edges[e + 1] * 2
      const dx = p[b] - p[a]; const dy = p[b + 1] - p[a + 1]
      const squared = dx * dx + dy * dy
      const maximum = maxEdgeLengths[e / 2]
      const wa = w[a / 2]; const wb = w[b / 2]
      if (maximum < 1e-6 || squared <= maximum * maximum * 1.00002 || wa + wb <= 1e-12) continue
      const length = Math.sqrt(squared)
      const scale = (length - maximum) / (length * (wa + wb))
      p[a] += dx * scale * wa
      p[a + 1] += dy * scale * wa
      p[b] -= dx * scale * wb
      p[b + 1] -= dy * scale * wb
      corrected = true
    }
    // Fixed alternating order avoids a persistent left/right propagation bias.
    for (let n = 0; n < indices.length; n += 3) {
      const t = pass % 2 ? indices.length - 3 - n : n
      const a = indices[t] * 2
      const b = indices[t + 1] * 2
      const c = indices[t + 2] * 2
      const reference = area(base, a, b, c)
      if (reference <= 1e-6) continue
      const deficit = reference * MIN_SECONDARY_AREA_RATIO - area(p, a, b, c)
      if (deficit <= reference * 1e-5) continue
      const ax = p[b + 1] - p[c + 1]
      const ay = p[c] - p[b]
      const bx = p[c + 1] - p[a + 1]
      const by = p[a] - p[c]
      const cx = p[a + 1] - p[b + 1]
      const cy = p[b] - p[a]
      const wa = w[a / 2]
      const wb = w[b / 2]
      const wc = w[c / 2]
      const denominator = wa * (ax * ax + ay * ay) + wb * (bx * bx + by * by) + wc * (cx * cx + cy * cy)
      if (denominator <= 1e-12) continue
      const lambda = deficit / denominator
      p[a] += lambda * wa * ax
      p[a + 1] += lambda * wa * ay
      p[b] += lambda * wb * bx
      p[b + 1] += lambda * wb * by
      p[c] += lambda * wc * cx
      p[c + 1] += lambda * wc * cy
      corrected = true
    }
    if (!corrected) break
  }
}

function area(p: Float32Array, a: number, b: number, c: number): number {
  return (p[b] - p[a]) * (p[c + 1] - p[a + 1]) - (p[b + 1] - p[a + 1]) * (p[c] - p[a])
}
