import type { Anime25DPlaybackLayer } from './types'
import type { CroppedLayerPixels } from './webglRuntime'
import assert from 'node:assert/strict'
import test from 'node:test'
import { canLiftNeckwearOverSkin } from './neckwearOcclusion'

function fixture() {
  const neck = {
    role: 'neck',
    x: 10,
    y: 10,
    w: 20,
    h: 30,
  } as Anime25DPlaybackLayer
  const body = { ...neck, role: 'topwear', x: 0, y: 25, w: 40, h: 40 }
  const accessory = { ...neck, role: 'neckwear', x: 0, y: 0, w: 40, h: 50 }
  const images = new Map<Anime25DPlaybackLayer, CroppedLayerPixels>()
  const put = (
    layer: Anime25DPlaybackLayer,
    opaque: (x: number, y: number) => boolean,
  ) => {
    const pixels = new Uint8ClampedArray(layer.w * layer.h * 4)
    for (let y = 0; y < layer.h; y++) {
      for (let x = 0; x < layer.w; x++) {
        pixels[(y * layer.w + x) * 4 + 3] = opaque(x + layer.x, y + layer.y)
          ? 255
          : 0
      }
    }
    images.set(layer, { width: layer.w, height: layer.h, pixels })
  }
  put(neck, () => true)
  put(body, () => true)
  return {
    neck,
    body,
    accessory,
    put,
    read: (l: Anime25DPlaybackLayer) => images.get(l) ?? null,
  }
}

test('transparent padding and detached art cannot establish skin contact', () => {
  const f = fixture()
  for (const visible of [() => false, (x: number) => x < 5]) {
    f.put(f.accessory, visible)
    assert.equal(
      canLiftNeckwearOverSkin(f.accessory, f.neck, f.body, [], f.read),
      false,
    )
  }
  f.put(f.accessory, (x, y) => x >= 18 && x <= 22 && y >= 28 && y < 35)
  assert.equal(
    canLiftNeckwearOverSkin(f.accessory, f.neck, f.body, [], f.read),
    true,
  )
})

test('front/back interleaving and unavailable obstacle pixels preserve authored order', () => {
  const f = fixture()
  f.put(f.accessory, (x, y) => x >= 18 && x <= 22 && y >= 28 && y < 35)
  const crossing = { ...f.body, role: 'front-hair' }
  assert.equal(
    canLiftNeckwearOverSkin(f.accessory, f.neck, f.body, [crossing], f.read),
    false,
  )
  f.put(crossing, (x) => x === 20)
  assert.equal(
    canLiftNeckwearOverSkin(f.accessory, f.neck, f.body, [crossing], f.read),
    false,
  )
  f.put(crossing, (x) => x < 5)
  assert.equal(
    canLiftNeckwearOverSkin(f.accessory, f.neck, f.body, [crossing], f.read),
    true,
  )
})
