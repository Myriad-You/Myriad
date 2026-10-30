import type { Anime25DPlaybackLayer } from './types'
import type { CroppedLayerPixels } from './webglRuntime'

const accessories = new Set(['neckwear', 'headwear', 'earwear', 'eyewear'])
/** Suppress proven redundant overlays, never erase/inpaint their underlying surface. */
export function duplicateAccessoryLayers(
  layers: readonly Anime25DPlaybackLayer[],
  read: (layer: Anime25DPlaybackLayer) => CroppedLayerPixels | null,
): Set<Anime25DPlaybackLayer> {
  const duplicates = new Set<Anime25DPlaybackLayer>()
  for (let i = 0; i < layers.length; i++) {
    const a = layers[i]
    if (!accessories.has(a.role)) continue
    for (let j = 0; j < i; j++) {
      const b = layers[j]
      // Removing the front copy across another drawing can change occlusion.
      if (
        layers
          .slice(j + 1, i)
          .some(
            (l) =>
              l.x < a.x + a.w &&
              l.x + l.w > a.x &&
              l.y < a.y + a.h &&
              l.y + l.h > a.y,
          )
      ) {
        continue
}
      if (
        !a.fade &&
        !a.phys &&
        ['neck', 'topwear', 'front-hair', 'back-hair'].includes(b.role) &&
        a.group === b.group &&
        embeddedCopy(a, b, read)
      ) {
        duplicates.add(a)
        break
      }
      if (
        duplicates.has(b) ||
        a.role !== b.role ||
        a.side !== b.side ||
        a.group !== b.group ||
        a.depth !== b.depth ||
        a.fade !== b.fade ||
        a.x !== b.x ||
        a.y !== b.y ||
        a.w !== b.w ||
        a.h !== b.h
      ) {
        continue
}
      const pa = read(a)
        const pb = read(b)
      if (
        !pa ||
        !pb ||
        pa.width !== pb.width ||
        pa.height !== pb.height ||
        pa.pixels.length !== pb.pixels.length
      ) {
        continue
}
      if (pa.pixels.every((v, k) => v === pb.pixels[k])) {
        duplicates.add(a)
        break
      }
    }
  }
  return duplicates
}

function embeddedCopy(
  a: Anime25DPlaybackLayer,
  b: Anime25DPlaybackLayer,
  read: (layer: Anime25DPlaybackLayer) => CroppedLayerPixels | null,
): boolean {
  if (a.x < b.x || a.y < b.y || a.x + a.w > b.x + b.w || a.y + a.h > b.y + b.h)
    return false
  const art = read(a)
    const base = read(b)
  if (!art || !base) return false
  let count = 0
    let low = 255
    let high = 0
  for (let y = 0; y < art.height; y++) {
    for (let x = 0; x < art.width; x++) {
      const p = (y * art.width + x) * 4
      if (art.pixels[p + 3] < 16) continue
      const wx = a.x + ((x + 0.5) / art.width) * a.w
        const wy = a.y + ((y + 0.5) / art.height) * a.h
      const bx = Math.floor(((wx - b.x) / b.w) * base.width)
        const by = Math.floor(((wy - b.y) / b.h) * base.height)
      if (bx < 0 || by < 0 || bx >= base.width || by >= base.height)
        return false
      const q = (by * base.width + bx) * 4
      if (base.pixels[q + 3] !== 255) return false
      for (let c = 0; c < 3; c++) {
        if (art.pixels[p + c] !== base.pixels[q + c]) return false
}
      low = Math.min(low, art.pixels[p])
      high = Math.max(high, art.pixels[p])
      count++
    }
}
  // Uniform colour coincidence (skin, cloth, shadow) is never enough evidence.
  return count >= 16 && high - low >= 30
}
