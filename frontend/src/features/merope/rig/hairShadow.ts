import type { RasterLayer } from './anime25dImportTypes'

/** Skin: opaque face or neck. */
const SKIN_ALPHA = 250
/** Bare skin: no hair (or anything else) over it at all. */
const BARE = 5
/** Partly under the hair: a tip or wisp the skin shows through. */
const WISP_LOW = 10
const WISP_HIGH = 230
/** The darkest tenth of the bare skin is its own shading, not the skin colour. */
const SHADE_SHARE = 0.1
/** Darker than the skin by this much (lightness) is a shadow on it. */
const SHADOW_DEPTH = 6
const MIN_SAMPLES = 200
/** A shadow beside the hair reaches this share of the skin's width out from it, at most. */
const CAST_REACH = 0.05
/** The skin's own outline: this many pixels in from its edge stay on the skin. */
const OUTLINE = 6

/**
 * The skin under and beside the front hair is painted with the shadow the
 * hair casts on it. At rest hair and shadow are one; once a keyed lock turns
 * off it (or the head turns and the hair swings), the shadow stays on the
 * skin as a stain the shape of the hair. The skin goes back to its plain
 * colour (a plane fitted on the bare skin, its shading left out) and the
 * lock takes the shadow into itself — under a wisp the wisp gets more opaque
 * and darker, beside the hair the lock grows a translucent dark (the colour
 * stays on the skin) — by what the shadow was, so the picture at rest is
 * the same and the shadow goes where the hair goes. Where anything else that
 * shows at rest is drawn between the skin and the hair (an eye, a brow) the
 * skin is left be.
 */
export function liftHairShadowOffSkin(layers: RasterLayer[], visibleAtRest: (layer: RasterLayer) => boolean): RasterLayer[] {
  const out = layers.slice()
  for (let index = 0; index < out.length; index += 1) {
    const skin = out[index]
    if (skin.role !== 'face' && skin.role !== 'neck') continue
    const above = out.slice(index + 1).filter(visibleAtRest)
    const hair = above.filter((layer) => layer.role === 'front-hair')
    const lifted = hair.length > 0 ? liftUnder(skin, hair, above.filter((layer) => layer.role !== 'front-hair')) : null
    if (!lifted) continue
    out[index] = lifted.skin
    for (let upper = index + 1; upper < out.length; upper += 1) out[upper] = lifted.hair.get(out[upper].id) ?? out[upper]
  }
  return out
}

function lightness(data: Uint8ClampedArray, at: number): number {
  return data[at] * 0.299 + data[at + 1] * 0.587 + data[at + 2] * 0.114
}

/** Calls `visit` with each pixel of `layer` over `skin`'s box, in the skin's frame. */
function overSkin(skin: RasterLayer, layer: RasterLayer, visit: (p: number, alpha: number) => void) {
  const x0 = Math.max(0, layer.left - skin.left)
  const y0 = Math.max(0, layer.top - skin.top)
  const x1 = Math.min(skin.width, layer.left + layer.width - skin.left)
  const y1 = Math.min(skin.height, layer.top + layer.height - skin.top)
  for (let y = y0; y < y1; y += 1) {
    for (let x = x0; x < x1; x += 1) {
      visit(y * skin.width + x, layer.data[((y + skin.top - layer.top) * layer.width + x + skin.left - layer.left) * 4 + 3])
    }
  }
}

function liftUnder(skin: RasterLayer, hair: RasterLayer[], between: RasterLayer[]): { skin: RasterLayer; hair: Map<string, RasterLayer> } | null {
  const { width, height } = skin
  const size = width * height
  // Per skin pixel: the one hair layer over it (-1 none, -2 more than one) and its alpha.
  const over = new Int16Array(size).fill(-1)
  const overAlpha = new Uint8Array(size)
  for (const [h, layer] of hair.entries()) {
    overSkin(skin, layer, (p, a) => {
      if (a < BARE) return
      over[p] = over[p] === -1 ? h : -2
      overAlpha[p] = Math.max(overAlpha[p], a)
    })
  }
  const hidden = new Uint8Array(size)
  for (const layer of between) {
    overSkin(skin, layer, (p, a) => {
      if (a >= BARE) hidden[p] = 1
    })
  }
  const solid = (p: number) => skin.data[p * 4 + 3] >= SKIN_ALPHA && !hidden[p]
  const bare: number[] = []
  const bareLight: number[] = []
  for (let p = 0; p < size; p += 1) {
    if (!solid(p) || over[p] !== -1) continue
    bare.push(p)
    bareLight.push(lightness(skin.data, p * 4))
  }
  if (bare.length < MIN_SAMPLES) return null
  const shade = [...bareLight].sort((a, b) => a - b)[Math.floor(bareLight.length * SHADE_SHARE)]
  const samples = bare.filter((_, i) => bareLight[i] >= shade)
  const field = [0, 1, 2].map((channel) => fitPlane(samples, skin.data, channel, width, height))
  const plainAt = (p: number) => {
    const x = (p % width) / width - 0.5
    const y = Math.floor(p / width) / height - 0.5
    return field.map((plane) => Math.max(plane.low, Math.min(plane.high, plane.a + plane.b * x + plane.c * y)))
  }
  const shadowed = (p: number, plain: number[]) => lightness(skin.data, p * 4) <= plain[0] * 0.299 + plain[1] * 0.587 + plain[2] * 0.114 - SHADOW_DEPTH
  // Per skin pixel, the lock whose shadow it carries.
  const owner = new Int16Array(size).fill(-1)
  for (let p = 0; p < size; p += 1) {
    if (over[p] >= 0 && solid(p) && overAlpha[p] >= WISP_LOW && overAlpha[p] <= WISP_HIGH && shadowed(p, plainAt(p))) owner[p] = over[p]
  }
  // Beside the hair: shadow joined to a lock through shadow, on the skin clear of its outline.
  const inner = insideSkin(skin)
  const reach = Math.max(4, Math.round(width * CAST_REACH))
  const steps = new Int32Array(size).fill(-1)
  let front: number[] = []
  for (let p = 0; p < size; p += 1) {
    if (over[p] >= 0) {
      steps[p] = 0
      front.push(p)
    }
  }
  for (let step = 1; step <= reach && front.length > 0; step += 1) {
    const next: number[] = []
    for (const p of front) {
      const x = p % width
      for (const q of [x > 0 ? p - 1 : -1, x < width - 1 ? p + 1 : -1, p - width, p + width]) {
        if (q < 0 || q >= size || steps[q] !== -1 || over[q] !== -1 || !inner[q] || hidden[q] || !shadowed(q, plainAt(q))) continue
        steps[q] = step
        owner[q] = over[p] >= 0 ? over[p] : owner[p]
        next.push(q)
      }
    }
    front = next
  }
  const moved: number[][] = hair.map(() => [])
  for (let p = 0; p < size; p += 1) {
    if (owner[p] >= 0) moved[owner[p]].push(p)
  }
  if (moved.every((pixels) => pixels.length === 0)) return null
  const skinData = new Uint8ClampedArray(skin.data)
  const lifted = new Map<string, RasterLayer>()
  for (const [h, pixels] of moved.entries()) {
    if (pixels.length === 0) continue
    const lock = grown(hair[h], skin, pixels)
    for (const p of pixels) {
      const x = p % width
      const y = (p - x) / width
      const plain = plainAt(p)
      const at = ((y + skin.top - lock.top) * lock.width + x + skin.left - lock.left) * 4
      const a = lock.data[at + 3] / 255
      // What shows at rest.
      const shown = [0, 1, 2].map((c) => a * lock.data[at + c] + (1 - a) * skin.data[p * 4 + c])
      if (a > 0) {
        // Under a wisp: the wisp that shows the same over the plain skin.
        let alpha = a
        for (const c of [0, 1, 2]) {
          if (plain[c] > 0) alpha = Math.max(alpha, 1 - shown[c] / plain[c])
        }
        alpha = Math.min(1, alpha)
        for (const c of [0, 1, 2]) {
          lock.data[at + c] = Math.round((shown[c] - (1 - alpha) * plain[c]) / alpha)
          skinData[p * 4 + c] = Math.round(plain[c])
        }
        lock.data[at + 3] = Math.round(alpha * 255)
      } else {
        // Beside the hair only the dark goes, as plain dark: its colour (a blush, the
        // warmth of the shadow) stays on the skin at the skin's lightness, so what
        // moves with the hair darkens whatever it lands on and tints nothing.
        const plainLight = plain[0] * 0.299 + plain[1] * 0.587 + plain[2] * 0.114
        // As much as leaves the skin a colour (no channel past white); the rest stays on it.
        const alpha = Math.max(0, Math.min(1 - lightness(skin.data, p * 4) / plainLight, 1 - Math.max(...shown) / 255))
        for (const c of [0, 1, 2]) skinData[p * 4 + c] = Math.round(shown[c] / (1 - alpha))
        lock.data.set([0, 0, 0, Math.round(alpha * 255)], at)
      }
    }
    lifted.set(lock.id, lock)
  }
  return { skin: { ...skin, data: skinData }, hair: lifted }
}

/** The skin's opaque pixels at least OUTLINE pixels in from its edge. */
function insideSkin(skin: RasterLayer): Uint8Array {
  const { width, height } = skin
  let inner = new Uint8Array(width * height)
  for (let p = 0; p < width * height; p += 1) inner[p] = skin.data[p * 4 + 3] >= SKIN_ALPHA ? 1 : 0
  for (let pass = 0; pass < OUTLINE; pass += 1) {
    const next = new Uint8Array(width * height)
    for (let y = 1; y < height - 1; y += 1) {
      for (let x = 1; x < width - 1; x += 1) {
        const p = y * width + x
        next[p] = inner[p] & inner[p - 1] & inner[p + 1] & inner[p - width] & inner[p + width]
      }
    }
    inner = next
  }
  return inner
}

/** A copy of `lock` whose box also takes in `pixels` (of the skin's frame). */
function grown(lock: RasterLayer, skin: RasterLayer, pixels: readonly number[]): RasterLayer {
  let left = lock.left
  let top = lock.top
  let right = lock.left + lock.width
  let bottom = lock.top + lock.height
  for (const p of pixels) {
    const x = (p % skin.width) + skin.left
    const y = Math.floor(p / skin.width) + skin.top
    left = Math.min(left, x)
    top = Math.min(top, y)
    right = Math.max(right, x + 1)
    bottom = Math.max(bottom, y + 1)
  }
  const width = right - left
  const data = new Uint8ClampedArray(width * (bottom - top) * 4)
  for (let y = 0; y < lock.height; y += 1) {
    const start = y * lock.width * 4
    data.set(lock.data.subarray(start, start + lock.width * 4), ((y + lock.top - top) * width + lock.left - left) * 4)
  }
  return { ...lock, left, top, width, height: bottom - top, data }
}

/** value ≈ a + b·x + c·y over the samples (x, y from -0.5 to 0.5), clamped to what they span. */
function fitPlane(samples: readonly number[], pixels: Uint8ClampedArray, channel: number, width: number, height: number) {
  let n = 0
  let sx = 0
  let sy = 0
  let sxx = 0
  let syy = 0
  let sxy = 0
  let sv = 0
  let sxv = 0
  let syv = 0
  let low = 255
  let high = 0
  for (const p of samples) {
    const x = (p % width) / width - 0.5
    const y = Math.floor(p / width) / height - 0.5
    const v = pixels[p * 4 + channel]
    n += 1
    sx += x
    sy += y
    sxx += x * x
    syy += y * y
    sxy += x * y
    sv += v
    sxv += x * v
    syv += y * v
    low = Math.min(low, v)
    high = Math.max(high, v)
  }
  // Centred normal equations, solved by Cramer's rule.
  const mx = sx / n
  const my = sy / n
  const mv = sv / n
  const cxx = sxx / n - mx * mx
  const cyy = syy / n - my * my
  const cxy = sxy / n - mx * my
  const cxv = sxv / n - mx * mv
  const cyv = syv / n - my * mv
  const det = cxx * cyy - cxy * cxy
  const b = Math.abs(det) < 1e-9 ? 0 : (cxv * cyy - cyv * cxy) / det
  const c = Math.abs(det) < 1e-9 ? 0 : (cyv * cxx - cxv * cxy) / det
  return { a: mv - b * mx - c * my, b, c, low, high }
}
