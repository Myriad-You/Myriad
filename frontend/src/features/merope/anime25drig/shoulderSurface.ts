import type { Anime25DPlaybackLayer } from './types'
import type { CroppedLayerPixels } from './webglRuntime'

/**
 * Fuse the two painted skin surfaces in rest space. Unlike an alpha cut,
 * the transition remains fully backed by skin when its torso mesh moves.
 */
export function fuseShoulderSurface(
  torso: Anime25DPlaybackLayer,
  body: CroppedLayerPixels,
  arms: readonly { layer: Anime25DPlaybackLayer; image: CroppedLayerPixels }[],
): CroppedLayerPixels | null {
  let output: Uint8ClampedArray | null = null
  const pixel = (image: CroppedLayerPixels, x: number, y: number) => {
    x = Math.floor(x)
    y = Math.floor(y)
    if (x < 0 || y < 0 || x >= image.width || y >= image.height) return null
    return image.pixels.subarray(
      (y * image.width + x) * 4,
      (y * image.width + x) * 4 + 4,
    )
  }
  const skin = (p: Uint8ClampedArray | null) =>
    p &&
    p[3] >= 250 &&
    p[0] > 100 &&
    p[0] > p[1] + 3 &&
    p[1] > p[2] - 12 &&
    p[0] - p[2] > 8 &&
    p[0] - p[1] < 85
  for (const { layer: arm, image } of arms) {
    const direction = arm.x + arm.w / 2 < torso.x + torso.w / 2 ? 1 : -1
    const band = Math.max(6, Math.round((body.width / torso.w) * arm.w * 0.055))
    const rows: {
      x: number
      y: number
      donors: Uint8ClampedArray[]
      strong: boolean
    }[] = []
    for (let y = 0; y < body.height; y++) {
      const wy = torso.y + ((y + 0.5) / body.height) * torso.h
      if (wy < arm.y || wy > arm.y + arm.h * 0.65) continue
      const start = Math.floor(
        (((direction === 1 ? arm.x : arm.x + arm.w) - torso.x) / torso.w) *
          body.width,
      )
      const end = Math.floor(
        (((direction === 1 ? arm.x + arm.w : arm.x) - torso.x) / torso.w) *
          body.width,
      )
      for (
        let x = start;
        direction === 1 ? x <= end : x >= end;
        x += direction
      ) {
        const edge = pixel(body, x, y)
        if (!edge || edge[3] < 16) continue
        const inside = pixel(body, x + direction * band, y)
        if (!skin(inside)) break
        const donors: Uint8ClampedArray[] = []
        let strong = true
        for (let d = 0; d <= band; d++) {
          const wx =
            torso.x + ((x + direction * d + 0.5) / body.width) * torso.w
          const donor = pixel(
            image,
            ((wx - arm.x) / arm.w) * image.width,
            ((wy - arm.y) / arm.h) * image.height,
          )
          const ordinarySkin = skin(donor)
          // Saturated warm highlights lose the red/green separation used by
          // the skin classifier. They may extend a proven seam, never seed it.
          const highlight =
            donor &&
            donor[3] >= 250 &&
            donor[2] >= 220 &&
            donor[0] >= donor[1] &&
            donor[1] >= donor[2] &&
            donor[0] - donor[2] >= 4
          if (
            (!ordinarySkin && !highlight) ||
            Math.max(
              ...[0, 1, 2].map((c) => Math.abs(donor![c] - inside![c])),
            ) > 35
          ) {
            break
}
          donors.push(donor!)
          if (!ordinarySkin) strong = false
        }
        if (donors.length === band + 1) rows.push({ x, y, donors, strong })
        break
      }
    }
    const seeds = rows.filter((row) => row.strong)
    if (seeds.length < Math.max(12, (arm.h / torso.h) * body.height * 0.15))
      continue
    // Only contiguous, short highlight runs connected to an ordinary skin
    // seed may be filled; a disconnected pale sleeve remains untouched.
    const accepted = new Set(seeds.map((row) => row.y))
    for (const directionY of [1, -1]) {
      let distance = Infinity
        let previous = -Infinity
      for (const row of directionY === 1 ? rows : rows.toReversed()) {
        if (Math.abs(row.y - previous) !== 1) distance = Infinity
        distance = row.strong ? 0 : distance + 1
        if (distance <= band * 3) accepted.add(row.y)
        previous = row.y
      }
    }
    for (const { x, y, donors } of Iterator.from(rows).filter((row) =>
      accepted.has(row.y),
    )) {
      for (let d = 0; d < band; d++) {
        const i = (y * body.width + x + direction * d) * 4
        const t = Math.max(0, (d - 2) / (band - 2))
        const mix = t * t * (3 - 2 * t)
        output ??= body.pixels.slice()
        for (let c = 0; c < 3; c++) {
          output[i + c] = Math.round(
            donors[d][c] * (1 - mix) + body.pixels[i + c] * mix,
          )
}
        // Keep silhouette and coverage byte-identical, including antialiasing.
      }
}
  }
  return output ? { ...body, pixels: output } : null
}
