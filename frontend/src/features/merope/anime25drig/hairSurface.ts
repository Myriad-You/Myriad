/** Local area barrier after existing hair motion; not a second physics clock. */
export interface HairSurface {
  base: Float32Array
  candidate: Float32Array
  mobility: Float32Array
  indices: Uint16Array
}

export function bindHairSurface(
  rest: Float32Array,
  indices: Uint16Array,
  along: Float32Array,
  pins: Float32Array | null,
): HairSurface {
  return {
    base: rest.slice(),
    candidate: rest.slice(),
    mobility: along.map((v, i) => v * (1 - (pins?.[i] ?? 0))),
    indices,
  }
}

/**
 * Weighted position projection: Müller et al., doi:10.1016/j.jvcir.2007.01.005.
 * Only secondary-motion compression is corrected. The reference is THIS frame's
 * head-projected surface, not the neutral drawing. Roots with zero mobility stay put.
 */
export function constrainHairSurface(surface: HairSurface): void {
  const { base, candidate: p, mobility: w, indices } = surface
  for (let pass = 0; pass < 8; pass++) {
    let corrected = false
    // Fixed alternating order avoids a persistent left/right propagation bias.
    for (let n = 0; n < indices.length; n += 3) {
      const t = pass % 2 ? indices.length - 3 - n : n
      const a = indices[t] * 2
      const b = indices[t + 1] * 2
      const c = indices[t + 2] * 2
      const reference = area(base, a, b, c)
      if (reference <= 1e-6) continue
      const deficit = reference * 0.2 - area(p, a, b, c)
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
