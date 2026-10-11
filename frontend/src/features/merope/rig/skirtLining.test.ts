import type { RasterLayer } from './anime25dImportTypes'
import assert from 'node:assert/strict'
import test from 'node:test'
import { tuckLiningBehindLegs } from './skirtLining'

function solid(
  id: string,
  role: RasterLayer['role'],
  rgb: readonly [number, number, number],
  box: { left: number; top: number; width: number; height: number },
): RasterLayer {
  const data = new Uint8ClampedArray(box.width * box.height * 4)
  for (let i = 0; i < data.length; i += 4) data.set([...rgb, 255], i)
  return {
    id,
    role,
    sourceName: id,
    order: 0,
    side: 'left',
    group: 'body',
    ...box,
    data,
  }
}

/** Two legs 300 tall with a 10 px gap at x 40..50, and a skirt over their tops. */
function standingFigure(tab: { left: number; width: number; bottom: number }) {
  const legL = { ...solid('legwear-left', 'legwear', [250, 220, 210], { left: 10, top: 20, width: 30, height: 300 }), side: 'left' as const }
  const legR = { ...solid('legwear-right', 'legwear', [250, 220, 210], { left: 50, top: 20, width: 30, height: 300 }), side: 'right' as const }
  const skirt = { ...solid('bottomwear', 'bottomwear', [120, 80, 200], { left: 5, top: 0, width: 80, height: tab.bottom }), side: null }
  // The hem is at y 30; below it only the tab is drawn.
  for (let y = 30; y < tab.bottom; y += 1) {
    for (let x = 0; x < skirt.width; x += 1) {
      const canvasX = skirt.left + x
      if (canvasX < tab.left || canvasX >= tab.left + tab.width) skirt.data[(y * skirt.width + x) * 4 + 3] = 0
    }
  }
  return [legL, legR, skirt]
}

test('a tab of skirt hanging between the legs goes behind them', () => {
  const layers = tuckLiningBehindLegs(standingFigure({ left: 39, width: 12, bottom: 40 }))
  assert.deepEqual(layers.map((layer) => layer.id), ['bottomwear-lining', 'legwear-left', 'legwear-right', 'bottomwear'])
  const [lining, , , skirt] = layers
  // Trimmed to the tab (x 39..51, y 30..40) with the usual 2 px margin, inside the skirt's own box.
  assert.deepEqual([lining.left, lining.top, lining.width, lining.height], [37, 28, 16, 12])
  // The skirt keeps its hem, without the tab.
  assert.equal(skirt.data[(35 * skirt.width + (44 - skirt.left)) * 4 + 3], 0)
  assert.equal(skirt.data[(10 * skirt.width + (44 - skirt.left)) * 4 + 3], 255)
})

test('a skirt reaching across the legs, or a short fringe between them, stays in front', () => {
  const across = standingFigure({ left: 20, width: 50, bottom: 40 })
  assert.equal(tuckLiningBehindLegs(across), across)
  const fringe = standingFigure({ left: 41, width: 6, bottom: 32 })
  assert.equal(tuckLiningBehindLegs(fringe), fringe)
})
