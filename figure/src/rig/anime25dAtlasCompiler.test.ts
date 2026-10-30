import assert from 'node:assert/strict'
import test from 'node:test'
import { layoutAnime25DAtlas } from './anime25dAtlasCompiler'

const copy = {
  anime25dLayerTooWide: 'layer {id} wider than {max}',
  anime25dAtlasOverflow: 'atlas overflow {max}',
}

function rectsOverlap(
  a: { x: number; y: number; w: number; h: number },
  b: { x: number; y: number; w: number; h: number },
  gutter: number,
): boolean {
  return (
    a.x < b.x + b.w + gutter &&
    a.x + a.w + gutter > b.x &&
    a.y < b.y + b.h + gutter &&
    a.y + a.h + gutter > b.y
  )
}

test('packs many same-height sprites into a square-ish atlas instead of one strip', () => {
  const layers = Array.from({ length: 12 }, (_, index) => ({
    id: `l${index}`,
    width: 640,
    height: 640,
  }))
  const packed = layoutAnime25DAtlas(layers, copy)
  assert.ok(packed.width <= 4096, `width ${packed.width}`)
  assert.ok(packed.height <= 4096, `height ${packed.height}`)
  assert.ok(
    packed.width < 12 * 640,
    `should wrap instead of a ${packed.width}px strip`,
  )
  assert.ok(packed.height > 640 + 16, 'uses more than one shelf')
  const aspect = packed.width / packed.height
  assert.ok(aspect > 0.4 && aspect < 2.5, `aspect ${aspect}`)

  const boxes = layers.map((layer, index) => ({
    x: packed.places[index].x,
    y: packed.places[index].y,
    w: layer.width,
    h: layer.height,
  }))
  for (let i = 0; i < boxes.length; i++) {
    assert.ok(boxes[i].x >= 8)
    assert.ok(boxes[i].y >= 8)
    assert.ok(boxes[i].x + boxes[i].w + 8 <= packed.width)
    assert.ok(boxes[i].y + boxes[i].h + 8 <= packed.height)
    for (let j = i + 1; j < boxes.length; j++) {
      assert.equal(
        rectsOverlap(boxes[i], boxes[j], 8),
        false,
        `sprites ${i} and ${j} overlap`,
      )
    }
  }
})

test('keeps output slots aligned with the input layer order', () => {
  const layers = [
    { id: 'short', width: 40, height: 20 },
    { id: 'tall', width: 30, height: 80 },
    { id: 'wide', width: 90, height: 25 },
  ]
  const packed = layoutAnime25DAtlas(layers, copy)
  assert.equal(packed.places.length, 3)
  assert.ok(packed.places[1].y !== packed.places[0].y || packed.places[1].x !== packed.places[0].x)
})

test('tucks small sprites into leftover space beside a large one', () => {
  const layers = [
    { id: 'body', width: 200, height: 200 },
    ...Array.from({ length: 8 }, (_, index) => ({
      id: `s${index}`,
      width: 80,
      height: 80,
    })),
  ]
  const packed = layoutAnime25DAtlas(layers, copy)
  const body = packed.places[0]
  const beside = packed.places
    .slice(1)
    .some(
      (place) =>
        place.y < body.y + 200 && place.x >= body.x + 200,
    )
  assert.equal(beside, true)
  assert.ok(packed.width * packed.height < (200 + 8 * 80) * (200 + 16))
})

test('rejects a sprite that cannot fit the 8192 cap', () => {
  assert.throws(
    () =>
      layoutAnime25DAtlas([{ id: 'huge', width: 8180, height: 10 }], copy),
    /huge/,
  )
})
