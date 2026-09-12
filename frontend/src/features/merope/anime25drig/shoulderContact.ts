import type { Anime25DPlaybackLayer } from './types'
import type { CroppedLayerPixels } from './webglRuntime'

type Layer = Pick<Anime25DPlaybackLayer, 'x' | 'y' | 'w' | 'h' | 'side'>

// Bind-time evidence only: an upper inner arm contour adjoining matching skin.
// Missing pixels, detached sleeves and ambiguous torso layers remain unchanged.
export function shoulderContactWeights(
  arm: Layer, pixels: CroppedLayerPixels | null,
  torso: Layer, body: CroppedLayerPixels | null, rest: Float32Array,
): Float32Array | null {
  if (!pixels || !body || !arm.side) return null
  if (![arm.w, arm.h, torso.w, torso.h].every(v => Number.isFinite(v) && v > 0)) return null
  if ([pixels, body].some(r => r.width <= 0 || r.height <= 0 || r.pixels.length !== r.width * r.height * 4)) return null
  const sample = (layer: Layer, raster: CroppedLayerPixels, x: number, y: number) => {
    const ix = Math.floor((x - layer.x) / layer.w * raster.width)
    const iy = Math.floor((y - layer.y) / layer.h * raster.height)
    if (ix < 0 || iy < 0 || ix >= raster.width || iy >= raster.height) return null
    const i = (iy * raster.width + ix) * 4
    const p = raster.pixels
    return p[i + 3] >= 220 ? [p[i], p[i + 1], p[i + 2]] : null
  }
  const skin = (c: number[] | null): c is number[] => Boolean(c &&
    c[0] > 100 && c[0] > c[1] + 3 && c[1] > c[2] - 12 &&
    c[0] - c[2] > 8 && c[0] - c[1] < 85)
  const matchingSkin = (a: number[] | null, b: number[] | null) =>
    skin(a) && skin(b) && Math.max(...a.map((v, i) => Math.abs(v - b[i]))) < 35
  const innerRight = arm.x + arm.w / 2 < torso.x + torso.w / 2
  const step = Math.max(1, arm.h / 80)
  const reach = Math.max(2, arm.w * 0.08)
  const contacts: [number, number, number][] = []
  for (let y = arm.y; y < arm.y + arm.h * 0.65; y += step) {
    for (let n = 0; n < 40; n++) {
      const x = arm.x + arm.w * (innerRight ? 1 - n / 80 : n / 80)
      const c = sample(arm, pixels, x, y)
      if (!c) continue
      if (skin(c)) {
        for (let d = 0; d <= reach; d += Math.max(1, reach / 4)) {
          const b = sample(torso, body, x + (innerRight ? d : -d), y)
          if (matchingSkin(c, b)) {
            // The visible join may be the torso's cut edge INSIDE the arm,
            // not the arm's inner edge. Follow only their contiguous skin overlap.
            const direction = innerRight ? -1 : 1
            const stride = Math.max(1, arm.w / 160)
            let outerX = x
            for (let distance = stride; distance <= arm.w; distance += stride) {
              const candidateX = x + distance * direction
              if (!matchingSkin(sample(arm, pixels, candidateX, y), sample(torso, body, candidateX, y))) break
              outerX = candidateX
            }
            contacts.push([Math.min(x, outerX), Math.max(x, outerX), y])
            break
          }
        }
      }
      break
    }
  }
  if (contacts.length < 10 || contacts.at(-1)![2] - contacts[0][2] < arm.h * 0.18) return null
  const weights = new Float32Array(rest.length / 2)
  const pin = arm.w * 0.12
  const fade = arm.w * 0.7
  for (let i = 0; i < weights.length; i++) {
    let distance = Infinity
    for (const [left, right, y] of contacts) {
      const dx = Math.max(left - rest[i * 2], 0, rest[i * 2] - right)
      distance = Math.min(distance, Math.hypot(dx, rest[i * 2 + 1] - y))
    }
    const t = Math.max(0, Math.min(1, (distance - pin) / fade))
    weights[i] = 1 - t * t * (3 - 2 * t)
  }
  return weights
}
