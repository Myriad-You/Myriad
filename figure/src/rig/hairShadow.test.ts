import type { RasterLayer } from './anime25dImportTypes'
import assert from 'node:assert/strict'
import test from 'node:test'
import { liftHairShadowOffSkin as liftWith } from './hairShadow'

const SKIN = [240, 214, 204] as const
const SHADOW = [180, 140, 130] as const
const HAIR = [90, 60, 40] as const

/** Every layer shows at rest but one named for crying. */
const liftHairShadowOffSkin = (layers: RasterLayer[]) => liftWith(layers, (shown) => shown.id !== 'eye-cry-l')

function layer(id: string, role: RasterLayer['role'], box: { left: number; top: number; width: number; height: number }): RasterLayer {
  return { id, role, sourceName: id, order: 0, side: null, group: 'head', ...box, data: new Uint8ClampedArray(box.width * box.height * 4) }
}

function paint(target: RasterLayer, x0: number, y0: number, x1: number, y1: number, rgba: readonly number[]) {
  for (let y = y0; y < y1; y += 1) {
    for (let x = x0; x < x1; x += 1) target.data.set(rgba, ((y - target.top) * target.width + x - target.left) * 4)
  }
}

function pixel(target: RasterLayer, x: number, y: number): number[] {
  const lx = x - target.left
  const ly = y - target.top
  if (lx < 0 || ly < 0 || lx >= target.width || ly >= target.height) return [0, 0, 0, 0]
  return Array.from(target.data.subarray((ly * target.width + lx) * 4, (ly * target.width + lx) * 4 + 4))
}

function over(top: number[], under: readonly number[]): number[] {
  const a = top[3] / 255
  return [0, 1, 2].map((c) => Math.round(top[c] * a + under[c] * (1 - a)))
}

/** A face with a wisp at half strength over x 40..60, its shadow painted on the skin under it. */
function wispOverFace() {
  const face = layer('face', 'face', { left: 0, top: 0, width: 100, height: 100 })
  paint(face, 0, 0, 100, 100, [...SKIN, 255])
  paint(face, 40, 30, 60, 70, [...SHADOW, 255])
  const wisp = layer('front-hair-1', 'front-hair', { left: 30, top: 0, width: 40, height: 80 })
  paint(wisp, 30, 0, 70, 30, [...HAIR, 255])
  paint(wisp, 40, 30, 60, 70, [...HAIR, 128])
  return [face, wisp]
}

test('the shadow under a wisp goes into the wisp; the picture at rest is the same', () => {
  const before = wispOverFace()
  const [face, wisp] = liftHairShadowOffSkin(wispOverFace())
  for (const [x, y] of [[50, 50], [45, 35], [20, 50], [50, 10]]) {
    const was = over(pixel(before[1], x, y), pixel(before[0], x, y))
    const now = over(pixel(wisp, x, y), pixel(face, x, y))
    for (const c of [0, 1, 2]) assert.ok(Math.abs(was[c] - now[c]) <= 1, `${x},${y}: ${was} → ${now}`)
  }
  // The wisp gone (turned off it), the skin under it is plain.
  assert.deepEqual(pixel(face, 50, 50).slice(0, 3), [...SKIN])
  assert.ok(pixel(wisp, 50, 50)[3] > 128, 'the wisp is more opaque, carrying the shadow')
})

test('bare skin and skin under solid hair are left as they are', () => {
  const [face, wisp] = liftHairShadowOffSkin(wispOverFace())
  assert.deepEqual(pixel(face, 20, 50), [...SKIN, 255])
  assert.deepEqual(pixel(wisp, 50, 10), [...HAIR, 255])
})

test('a face without hair over it is not touched', () => {
  const face = layer('face', 'face', { left: 0, top: 0, width: 100, height: 100 })
  paint(face, 0, 0, 100, 100, [...SKIN, 255])
  const layers = [face]
  assert.equal(liftHairShadowOffSkin(layers)[0], face)
})

/** The skin, only darker. */
const DIM = SKIN.map((value) => Math.round(value * 0.8))
/** Darker and redder: a blush. */
const BLUSH = [236, 180, 176] as const

/** A face with a solid lock over x 40..60, its shadow cast on the bare skin beside it to x 64. */
function shadowBesideLock(shadow: readonly number[] = DIM) {
  const face = layer('face', 'face', { left: 0, top: 0, width: 100, height: 100 })
  paint(face, 0, 0, 100, 100, [...SKIN, 255])
  paint(face, 40, 20, 64, 70, [...shadow, 255])
  const lock = layer('front-hair-1', 'front-hair', { left: 40, top: 0, width: 20, height: 70 })
  paint(lock, 40, 0, 60, 70, [...HAIR, 255])
  return [face, lock]
}

test('the shadow beside a lock goes into the lock; the picture at rest is the same', () => {
  const before = shadowBesideLock()
  const [face, lock] = liftHairShadowOffSkin(shadowBesideLock())
  for (const [x, y] of [[62, 40], [63, 60], [50, 40], [80, 40]]) {
    const was = over(pixel(before[1], x, y), pixel(before[0], x, y))
    const now = over(pixel(lock, x, y), pixel(face, x, y))
    for (const c of [0, 1, 2]) assert.ok(Math.abs(was[c] - now[c]) <= 1, `${x},${y}: ${was} → ${now}`)
  }
  pixel(face, 62, 40).slice(0, 3).forEach((value, c) => assert.ok(Math.abs(value - SKIN[c]) <= 1, 'the skin beside the lock is plain'))
  assert.ok(lock.left + lock.width >= 64, 'the lock grew to carry its shadow')
  assert.deepEqual(pixel(lock, 62, 40).slice(0, 3), [0, 0, 0], 'a plain dark, no colour')
})

test('beside the hair a blush keeps its colour on the skin; only its dark goes', () => {
  const before = shadowBesideLock(BLUSH)
  const [face, lock] = liftHairShadowOffSkin(shadowBesideLock(BLUSH))
  const was = over(pixel(before[1], 62, 40), pixel(before[0], 62, 40))
  const now = over(pixel(lock, 62, 40), pixel(face, 62, 40))
  for (const c of [0, 1, 2]) assert.ok(Math.abs(was[c] - now[c]) <= 1, `${was} → ${now}`)
  const [r, g] = pixel(face, 62, 40)
  assert.ok(r - g > SKIN[0] - SKIN[1] + 20, 'still redder than the skin')
})

test('skin with an eye drawn over it under a wisp is left be', () => {
  const [face, wisp] = wispOverFace()
  const eye = layer('eyewhite-l', 'eyewhite', { left: 40, top: 30, width: 20, height: 40 })
  paint(eye, 40, 30, 60, 70, [255, 255, 255, 255])
  const [faceAfter, eyeAfter, wispAfter] = liftHairShadowOffSkin([face, eye, wisp])
  assert.deepEqual(pixel(faceAfter, 50, 50), [...SHADOW, 255])
  assert.equal(eyeAfter, eye)
  assert.deepEqual(pixel(wispAfter, 50, 50), [...HAIR, 128])
})

test('a variant hidden at rest does not keep the shadow on the skin', () => {
  const [face, wisp] = wispOverFace()
  const tears = layer('eye-cry-l', 'eye-cry', { left: 40, top: 30, width: 20, height: 40 })
  paint(tears, 40, 30, 60, 70, [120, 180, 255, 200])
  const [faceAfter] = liftHairShadowOffSkin([face, tears, wisp])
  assert.deepEqual(pixel(faceAfter, 50, 50).slice(0, 3), [...SKIN])
})
