import type { Anime25DPart, Anime25DPlaybackLayer } from './types'
import type { CroppedLayerPixels } from './webglRuntime'
import assert from 'node:assert/strict'
import test from 'node:test'
import { cutPiece, partOfLayer, splitHangingParts, splitLayerName } from './partSplit'

const PINK = [240, 170, 190]
const GOLD = [210, 160, 40]

/** A w×h layer, pink and opaque, with a gold block at [gx0, gy0, gx1, gy1). */
function art(w: number, h: number, gold: [number, number, number, number] | null, clear: [number, number, number, number] | null = null): CroppedLayerPixels {
  const pixels = new Uint8ClampedArray(w * h * 4)
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4
      const inGold = gold && x >= gold[0] && x < gold[2] && y >= gold[1] && y < gold[3]
      const inClear = clear && x >= clear[0] && x < clear[2] && y >= clear[1] && y < clear[3]
      const colour = inGold ? GOLD : PINK
      pixels.set([...colour, inClear && !inGold ? 0 : 255], i)
    }
  }
  return { pixels, width: w, height: h }
}

const pixel = (image: CroppedLayerPixels, x: number, y: number) => Array.from(image.pixels.subarray((y * image.width + x) * 4, (y * image.width + x) * 4 + 4))

test('a pendant is cut out of the layer it shares and the hole filled from around it', () => {
  // A pink band down to y 18; the gold pendant hangs from it, over nothing below.
  const cut = cutPiece(art(40, 40, [16, 14, 24, 34], [0, 18, 40, 40]), [10, 10, 30, 36], 14)!
  assert.ok(cut)
  assert.equal(cut.x, 15, 'as big as it is, with its soft edge')
  assert.equal(cut.piece.width, 10)
  assert.deepEqual(pixel(cut.piece, 20 - cut.x, 28 - cut.y), [...GOLD, 255], 'the pendant is in its own layer')
  const filled = pixel(cut.host, 20, 16)
  assert.ok(filled.slice(0, 3).every((v, c) => Math.abs(v - PINK[c]) < 2) && filled[3] === 255, `where it lay on the band, the band is filled: ${filled}`)
  assert.equal(pixel(cut.host, 20, 28)[3], 0, 'below it, nothing')
})

test('what lies on its cloth is not a piece that hangs', () => {
  assert.equal(cutPiece(art(40, 40, [16, 14, 24, 26]), [10, 10, 30, 30], 14), null, 'a gold patch on pink cloth stays')
})

test('what cannot be told apart stays as it was', () => {
  assert.equal(cutPiece(art(40, 40, null), [10, 10, 30, 30]), null, 'nothing differs')
  assert.equal(cutPiece(art(40, 40, [0, 0, 40, 40]), [10, 10, 30, 30]), null, 'all of it differs')
  assert.equal(cutPiece(art(40, 40, [0, 18, 40, 22]), [10, 10, 30, 30]), null, 'a stripe running on past the box is the host\'s')
})

test('a pendant hangs from a band: what is above where it hangs stays', () => {
  // A purple band across the whole layer at y 4–12, a gold pendant hanging from it.
  const image = art(40, 40, [17, 12, 23, 30], [0, 12, 40, 40])
  for (let y = 4; y < 12; y++) {
    for (let x = 0; x < 40; x++) image.pixels.set([150, 90, 200, 255], (y * 40 + x) * 4)
  }
  assert.equal(cutPiece(image, [12, 6, 28, 18]), null, 'nothing hangs from the top of a box that is all band')
  const cut = cutPiece(image, [12, 6, 28, 30], 12)!
  assert.ok(cut, 'from where it hangs it is a piece')
  assert.equal(pixel(cut.host, 5, 10)[0], 150, 'the band stays in the host')
  assert.equal(pixel(cut.piece, 20 - cut.x, 20 - cut.y)[0], GOLD[0])
})

test('an outline drawn through a piece does not carry it off', () => {
  // A dark outline from top to bottom, x 19–21, through the gold charm.
  const image = art(40, 40, [16, 14, 24, 32], [0, 16, 40, 40])
  for (let y = 0; y < 40; y++) {
    for (let x = 19; x < 21; x++) image.pixels.set([40, 30, 60, 255], (y * 40 + x) * 4)
  }
  const cut = cutPiece(image, [10, 10, 30, 34], 14)!
  assert.ok(cut, 'the charm is still a piece')
  assert.deepEqual(pixel(cut.piece, 17 - cut.x, 26 - cut.y), [...GOLD, 255])
  assert.deepEqual(pixel(cut.host, 20, 5), [40, 30, 60, 255], 'the outline above it stays in the host')
})

test('a box the inventory put off to one side still finds its piece', () => {
  const layer = { name: 'legwear', role: 'legwear', x: 0, y: 0, w: 60, h: 60, atlas: { x: 0, y: 0, w: 1, h: 1 } } as Anime25DPlaybackLayer
  const pixels = art(60, 60, [36, 20, 44, 34], [0, 22, 60, 60])
  const part: Anime25DPart = {
    name: 'charm', layer: 'legwear', separate: false, box: [24, 18, 36, 36], anchor: [30, 20],
    bone: 'thighR', motion: 'dangle', material: 'metal', stiffness: 0.6, weight: 0.5,
  }
  const cut = splitHangingParts([layer], { version: 1, source: 'ai-vision', parts: [part] }, { width: 1000, height: 1000 }, () => pixels)
  assert.equal(cut.length, 1)
  assert.deepEqual(pixel(cut[0].pixels, 40 - (cut[0].source.x), 25 - (cut[0].source.y)), [...GOLD, 255])
})

test('a piece hanging past its host leaves nothing behind there', () => {
  // The host is clear below y = 28; the charm hangs from 20 to 36.
  const cut = cutPiece(art(40, 40, [16, 20, 24, 36], [0, 24, 40, 40]), [10, 14, 30, 38], 20)!
  assert.ok(cut)
  assert.equal(pixel(cut.host, 20, 34)[3], 0, 'below the host, clear')
  assert.equal(pixel(cut.host, 20, 21)[3], 255, 'within it, filled')
})

test('the pieces of an inventory are cut out of their hosts, named for their part', () => {
  const layer = (name: string, role: string): Anime25DPlaybackLayer => ({
    name, role, x: 100, y: 100, w: 60, h: 40, atlas: { x: 0, y: 0, w: 0.1, h: 0.1 },
  }) as Anime25DPlaybackLayer
  const choker = layer('neckwear', 'neckwear')
  const face = layer('face', 'face')
  const part = (patch: Partial<Anime25DPart>): Anime25DPart => ({
    name: 'pendant', layer: 'neckwear', separate: false, box: [114, 112, 126, 132], anchor: [120, 112],
    bone: 'neck', motion: 'dangle', material: 'metal', stiffness: 0.6, weight: 0.5, ...patch,
  })
  const profile = { version: 1 as const, source: 'ai-vision' as const, parts: [
    part({}),
    part({ name: 'unnamed', layer: null, box: [143, 130, 151, 139], anchor: [147, 130] }),
    part({ name: 'cloth', motion: 'sway' }),
    part({ name: 'earring', bone: 'head' }),
  ] }
  const pixels = art(60, 40, [16, 14, 24, 30], [0, 16, 60, 40])
  // A second charm low on the right, which the inventory did not place in a layer.
  for (let y = 32; y < 38; y++) {
    for (let x = 44; x < 50; x++) pixels.pixels.set([...GOLD, 255], (y * 60 + x) * 4)
  }
  const cut = splitHangingParts([face, choker], profile, { width: 1000, height: 1000 }, (l) => (l === choker ? pixels : null))
  assert.equal(cut.length, 2, 'the named and the found host; cloth stays, and an "earring" on the choker is not cut')
  assert.equal(cut[0].source.name, splitLayerName(0, profile.parts[0]))
  assert.equal(cut[0].source.role, 'objects')
  assert.equal(cut[0].host, choker)
  assert.equal(cut[1].host, choker, 'the topmost layer opaque at its middle')
  assert.equal(partOfLayer(profile, cut[0].source.name), profile.parts[0])
  assert.equal(partOfLayer(profile, 'neckwear'), profile.parts[0])
})
