import type { Anime25DPlaybackLayer } from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { duplicateAccessoryLayers } from './accessoryDuplicate'

test('a fully baked copy keeps the surface and suppresses only the redundant overlay', () => {
  const base = {
    name: 'shirt',
    role: 'topwear',
    group: 'body',
    side: null,
    depth: 1,
    fade: null,
    phys: null,
    x: 0,
    y: 0,
    w: 8,
    h: 8,
  } as Anime25DPlaybackLayer
  const art = { ...base, name: 'pin', role: 'neckwear' }
  const image = {
    width: 8,
    height: 8,
    pixels: Uint8ClampedArray.from(
      Array.from({ length: 64 }, (_, i) => [80 + i * 2, 20, 30, 255]).flat(),
    ),
  }
  assert.deepEqual(
    Iterator.from(duplicateAccessoryLayers([base, art], () => image)).toArray(),
    [art],
  )
  const changed = { ...image, pixels: image.pixels.slice() }
  changed.pixels[0]++
  assert.equal(
    duplicateAccessoryLayers([base, art], (l) => (l === base ? changed : image))
      .size,
    0,
  )
  const uniform = {
    ...image,
    pixels: Uint8ClampedArray.from(
      Array.from({ length: 64 }, () => [80, 20, 30, 255]).flat(),
    ),
  }
  assert.equal(duplicateAccessoryLayers([base, art], () => uniform).size, 0)
})

test('only exact repeated accessory art is removed; shadows, occluders and offsets survive', () => {
  const a = {
    name: 'pin',
    role: 'headwear',
    group: 'head',
    side: null,
    depth: 1,
    fade: null,
    x: 0,
    y: 0,
    w: 2,
    h: 1,
  } as Anime25DPlaybackLayer
  const b = { ...a, name: 'pin-copy' }
  const image = {
    width: 2,
    height: 1,
    pixels: new Uint8ClampedArray([200, 10, 20, 255, 20, 40, 60, 128]),
  }
  assert.deepEqual(Iterator.from(duplicateAccessoryLayers([a, b], () => image)).toArray(), [b])
  assert.equal(
    duplicateAccessoryLayers([a, { ...b, x: 1 }], () => image).size,
    0,
  )
  assert.equal(
    duplicateAccessoryLayers([a, { ...a, role: 'front-hair' }, b], () => image)
      .size,
    0,
  )
  assert.equal(
    duplicateAccessoryLayers([a, b], (l) =>
      l === a
        ? image
        : {
            ...image,
            pixels: new Uint8ClampedArray([200, 10, 20, 255, 20, 40, 60, 127]),
          },
    ).size,
    0,
  )
  assert.equal(duplicateAccessoryLayers([a, b], () => null).size, 0)
})
