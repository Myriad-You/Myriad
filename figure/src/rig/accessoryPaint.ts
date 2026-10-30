import type { Anime25DSourceReference, RasterLayer } from './anime25dImportTypes'
import { alphaAt } from './psdRepairMasks'

/** Small pieces worn on the head and neck, which a decomposer paints over whole. */
const PAINTED_ROLES: ReadonlySet<RasterLayer['role']> = new Set(['earwear', 'headwear', 'neckwear'])
/** Opaque enough to be the piece's own interior, not its antialiased rim. */
const INTERIOR_ALPHA = 245
/** What lies over it at rest no more than this, so the portrait shows the piece there. */
const UNCOVERED_ALPHA = 12

/**
 * A decomposer makes an accessory opaque over its whole outline and guesses
 * what it hides: a birdcage earring comes out a dark block where the portrait
 * shows the hair through its bars. Where such a piece is the topmost thing at
 * rest, the portrait shows exactly what it should look like there (itself, or
 * what is seen through it), so it takes the portrait's colours. Its outline
 * and its rim stay the decomposition's; whatever covers it keeps its own.
 */
export function paintAccessoriesFromReference(
  layers: readonly RasterLayer[],
  visibleAtRest: (layer: RasterLayer) => boolean,
  reference: Readonly<Anime25DSourceReference>,
): RasterLayer[] {
  return layers.map((layer, index) => {
    if (!PAINTED_ROLES.has(layer.role)) return layer
    const above = layers.slice(index + 1).filter(visibleAtRest)
    let data: Uint8ClampedArray | null = null
    for (let y = 0; y < layer.height; y += 1) {
      const canvasY = layer.top + y
      if (canvasY < 0 || canvasY >= reference.height) continue
      for (let x = 0; x < layer.width; x += 1) {
        const canvasX = layer.left + x
        if (canvasX < 0 || canvasX >= reference.width) continue
        const i = (y * layer.width + x) * 4
        if (layer.data[i + 3] < INTERIOR_ALPHA) continue
        const r = (canvasY * reference.width + canvasX) * 4
        if (reference.data[r + 3] < 250) continue
        if (above.some((cover) => alphaAt(cover, canvasX, canvasY) > UNCOVERED_ALPHA)) continue
        data ??= new Uint8ClampedArray(layer.data)
        data[i] = reference.data[r]
        data[i + 1] = reference.data[r + 1]
        data[i + 2] = reference.data[r + 2]
      }
    }
    return data ? { ...layer, data } : layer
  })
}
