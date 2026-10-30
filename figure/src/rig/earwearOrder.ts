import type { RasterLayer } from './anime25dImportTypes'
import { alphaAt } from './psdRepairMasks'

/** A lock over this share of an ear ornament at rest is drawn over it on purpose. */
const COVERED_SHARE = 0.01

/**
 * An ear ornament the picture shows over the hair at rest goes over the
 * front hair too: when the locks turn or swing they pass behind it, as the
 * turned pictures draw them. Nothing changes at rest, since no lock covers it
 * there; one a lock does cover at rest keeps its place.
 */
export function raiseEarwearOverFrontHair(layers: readonly RasterLayer[]): RasterLayer[] {
  let output = [...layers]
  const locks = output.filter((layer) => layer.role === 'front-hair')
  if (locks.length === 0) return output
  for (const earwear of layers.filter((layer) => layer.role === 'earwear' && layer.group === 'head')) {
    const topLock = Math.max(...locks.map((lock) => output.indexOf(lock)))
    const index = output.indexOf(earwear)
    if (index > topLock || coveredByLocks(earwear, locks)) continue
    output = output.toSpliced(index, 1)
    output = output.toSpliced(topLock, 0, earwear)
  }
  return output
}

function coveredByLocks(earwear: RasterLayer, locks: readonly RasterLayer[]): boolean {
  let shown = 0
  let covered = 0
  for (let y = 0; y < earwear.height; y++) {
    for (let x = 0; x < earwear.width; x++) {
      if (earwear.data[(y * earwear.width + x) * 4 + 3] < 128) continue
      shown++
      const canvasX = earwear.left + x
      const canvasY = earwear.top + y
      if (locks.some((lock) => alphaAt(lock, canvasX, canvasY) >= 128)) covered++
    }
  }
  return shown > 0 && covered > shown * COVERED_SHARE
}
