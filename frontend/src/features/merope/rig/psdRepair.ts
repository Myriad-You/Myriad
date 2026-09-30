import type { Anime25DSourceReference, RasterLayer } from './anime25dImportTypes'
import type { Anime25DPsdReconciliation } from './psdReconciliation'
import type { RecoveredPiece } from './psdRepairLayers'
import { analyzeAnime25DPsd, reportAnime25DPsdAnalysis } from './psdReconciliation'
import { hasExpressionVariants, mergeByGroup, mountFor, overlayLayer, revealableBelow, withPixels } from './psdRepairLayers'
import { addAntialiasedRim, alphaAt, closeMask, components, fillSmallHoles, meanCoveredDistance } from './psdRepairMasks'

export interface Anime25DPsdRepair {
  layers: RasterLayer[]
  reconciliation: Anime25DPsdReconciliation | null
}

/**
 * A reveal grows past the verdict threshold to the covering art's real edge,
 * which keeps its own line art; this caps runaway growth in flat colour.
 */
const REVEAL_MAX_GROWTH = 4
const REVEAL_SOFT_EDGE = 2
const REVEAL_SOFT_EDGE_ALPHA = 160
/**
 * A covered layer that only matches pixel by pixel is not the same art: pale
 * skin under pale hair does. Faithful reveals measured 7-23; false ones 31-37.
 */
const REVEAL_MAX_MEAN_DISTANCE = 26
/** Bridges an accessory's thin parts, e.g. the bars of a birdcage earring. */
const RECOVER_CLOSING = 3
const RECOVER_MAX_HOLE_SHARE = 0.004
/** Larger recoveries would pin garment-sized static art onto moving layers. */
const RECOVER_MAX_SHARE = 0.02

/**
 * Repairs what reconciliation can prove from the source illustration:
 * - `buried`: erase the covering pixels so the matching layer below shows,
 *   when that layer matches as a whole rather than by coincidence.
 * - `missing` / `mismatch`, and weak `buried`: lift the illustration's own
 *   pixels into a rigid `objects` layer per body group, drawn above whatever
 *   it overrides. Garment-sized areas are only reported.
 * Layers with expression variants are never edited or covered, so blinking and
 * speech stay live; such regions are only reported. Hidden surfaces cannot be
 * recovered because the illustration never shows them.
 */
export function repairAnime25DPsd(
  layers: readonly RasterLayer[],
  visibleAtRest: (layer: RasterLayer) => boolean,
  reference: Readonly<Anime25DSourceReference>,
  maxNewLayers: number,
): Anime25DPsdRepair {
  const visible = layers.filter(visibleAtRest)
  const before = analyzeAnime25DPsd(visible, reference)
  if (!before || before.status !== 'reconciled') {
    return {
      layers: [...layers],
      reconciliation: before && reportAnime25DPsdAnalysis(before, reference),
    }
  }
  const { bounds } = before
  const repaired = new Uint8Array(reference.width * reference.height)
  const copies = new Map<RasterLayer, RasterLayer>()
  const writable = (index: number) => {
    const original = visible[index]
    let copy = copies.get(original)
    if (!copy) {
      copy = { ...original, data: new Uint8ClampedArray(original.data) }
      copies.set(original, copy)
    }
    return copy
  }
  const at = (target: number) => ({
    x: bounds.x0 + (target % bounds.width),
    y: bounds.y0 + Math.floor(target / bounds.width),
  })

  const count = bounds.width * bounds.height
  // Specks below the size floor only help bridge a real region's fragments.
  const support = new Uint8Array(count)
  for (let target = 0; target < count; target += 1) {
    const code = before.flagged[target]
    if (code === 2 || code === 4) support[target] = 1
  }
  const seeds = new Uint8Array(count)
  for (const region of before.regions) {
    if (region.kind !== 'missing' && region.kind !== 'mismatch') continue
    for (const target of region.members) seeds[target] = 1
  }

  let revealed = 0
  for (const region of before.regions) {
    if (region.kind !== 'buried') continue
    const covering = (target: number) => {
      const { x, y } = at(target)
      const indices: number[] = []
      for (
        let index = before.covered[target] + 1;
        index < visible.length;
        index += 1
      ) {
        if (alphaAt(visible[index], x, y) > 0) indices.push(index)
      }
      return indices
    }
    if (
      region.members.some((target) =>
        covering(target).some((index) => hasExpressionVariants(visible[index])),
      )
    ) {
      continue
    }
    if (
      meanCoveredDistance(before, region.members, visible, reference) >
      REVEAL_MAX_MEAN_DISTANCE
    ) {
      for (const target of region.members) {
        seeds[target] = 1
        support[target] = 1
      }
      continue
    }
    const targets = new Map<number, number>()
    for (const target of region.members) {
      targets.set(target, before.covered[target])
    }
    // Grow while the covered art keeps beating the composite, so the reveal
    // ends at the covering drawing's own outline, not at a threshold contour.
    const queue = [...region.members]
    const limit = region.members.length * REVEAL_MAX_GROWTH
    while (queue.length > 0 && targets.size < limit) {
      const target = queue.pop()!
      const x = target % bounds.width
      const y = Math.floor(target / bounds.width)
      for (const next of [
        x > 0 ? target - 1 : -1,
        x < bounds.width - 1 ? target + 1 : -1,
        y > 0 ? target - bounds.width : -1,
        y < bounds.height - 1 ? target + bounds.width : -1,
      ]) {
        if (next < 0 || targets.has(next)) continue
        // A covering strand may span several layers underneath it.
        const matched = revealableBelow(
          before,
          visible,
          next,
          at(next),
          reference,
        )
        if (matched >= 0) {
          targets.set(next, matched)
          queue.push(next)
        }
      }
    }
    const erased = new Map<number, Array<{ x: number; y: number }>>()
    for (const [target, below] of targets) {
      const { x, y } = at(target)
      for (let index = below + 1; index < visible.length; index += 1) {
        const layer = visible[index]
        if (alphaAt(layer, x, y) === 0 || hasExpressionVariants(layer)) {
          continue
        }
        const copy = writable(index)
        copy.data[((y - copy.top) * copy.width + (x - copy.left)) * 4 + 3] = 0
        repaired[y * reference.width + x] = 1
        const points = erased.get(index) ?? []
        points.push({ x, y })
        erased.set(index, points)
      }
    }
    // The removed drawing's anti-aliased rim would linger as a ghost outline.
    for (const [index, points] of erased) {
      const copy = writable(index)
      for (const { x, y } of points) {
        for (let dy = -REVEAL_SOFT_EDGE; dy <= REVEAL_SOFT_EDGE; dy += 1) {
          for (let dx = -REVEAL_SOFT_EDGE; dx <= REVEAL_SOFT_EDGE; dx += 1) {
            const alpha = alphaAt(copy, x + dx, y + dy)
            if (alpha === 0 || alpha >= REVEAL_SOFT_EDGE_ALPHA) continue
            copy.data[
              ((y + dy - copy.top) * copy.width + (x + dx - copy.left)) * 4 + 3
            ] = 0
            repaired[(y + dy) * reference.width + x + dx] = 1
          }
        }
      }
    }
    revealed += 1
  }

  const pieces: RecoveredPiece[] = []
  {
    const mask = closeMask(
      support,
      bounds.width,
      bounds.height,
      RECOVER_CLOSING,
    )
    fillSmallHoles(
      mask,
      bounds.width,
      bounds.height,
      Math.max(64, Math.round(before.compared * RECOVER_MAX_HOLE_SHARE)),
    )
    const maxArea = Math.round(before.compared * RECOVER_MAX_SHARE)
    for (const members of components(mask, bounds.width, bounds.height)) {
      if (!members.some((target) => seeds[target])) continue
      const shown = members.filter((target) => {
        const { x, y } = at(target)
        return reference.data[(y * reference.width + x) * 4 + 3] >= 250
      })
      if (shown.length < before.minArea || shown.length > maxArea) continue
      if (
        shown.some((target) => {
          const { x, y } = at(target)
          return visible.some(
            (layer) =>
              hasExpressionVariants(layer) && alphaAt(layer, x, y) >= 128,
          )
        })
      ) {
        continue
      }
      const pixels = new Map<
        number,
        readonly [number, number, number, number]
      >()
      let above = -1
      for (const target of shown) {
        const { x, y } = at(target)
        const offset = (y * reference.width + x) * 4
        pixels.set(y * reference.width + x, [
          reference.data[offset],
          reference.data[offset + 1],
          reference.data[offset + 2],
          255,
        ])
        above = Math.max(above, before.top[target])
      }
      addAntialiasedRim(before, shown, reference, pixels)
      pieces.push({ pixels, above, ...mountFor(before, shown, visible) })
    }
  }
  // A piece of an existing accessory, e.g. the tassel under a hair ornament,
  // joins that drawing so both stay one rigid body. It only can when nothing
  // drawn above the accessory covers the piece at rest.
  let absorbed = 0
  const standalone: RecoveredPiece[] = []
  for (const piece of pieces) {
    if (piece.owner < 0 || piece.above > piece.owner) {
      standalone.push(piece)
      continue
    }
    const original = visible[piece.owner]
    copies.set(
      original,
      withPixels(
        copies.get(original) ?? original,
        piece.pixels,
        reference.width,
      ),
    )
    for (const key of piece.pixels.keys()) repaired[key] = 1
    absorbed += 1
  }
  // One layer per piece lets each follow what it hangs from; over budget,
  // pieces of one body part share a plain rigid layer instead.
  const recoveredLayers =
    standalone.length <= maxNewLayers
      ? standalone
      : mergeByGroup(standalone, maxNewLayers)
  for (const piece of recoveredLayers) {
    for (const key of piece.pixels.keys()) repaired[key] = 1
  }
  const recovered =
    absorbed +
    standalone.filter((piece) =>
      recoveredLayers.some(
        (layer) => layer === piece || layer.group === piece.group,
      ),
    ).length

  let output = layers.map((layer) => copies.get(layer) ?? layer)
  for (const piece of recoveredLayers) {
    const layer = overlayLayer(piece, reference.width, output)
    if (!layer) continue
    const anchor =
      piece.above >= 0
        ? visible[piece.above]
        : piece.neighbour >= 0
          ? visible[piece.neighbour]
          : visible.findLast((candidate) => candidate.group === piece.group)
    const index = anchor
      ? output.indexOf(copies.get(anchor) ?? anchor)
      : output.length - 1
    output = output.toSpliced(index + 1, 0, layer)
  }

  const after = analyzeAnime25DPsd(output.filter(visibleAtRest), reference)
  return {
    layers: output,
    reconciliation: after
      ? reportAnime25DPsdAnalysis(
          after,
          reference,
          { revealed, recovered },
          repaired,
        )
      : null,
  }
}
