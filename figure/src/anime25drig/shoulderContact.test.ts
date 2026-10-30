import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'
import { shoulderContactWeights } from './shoulderContact'

test('pins adjoining skin but leaves detached or fabric sleeves unchanged', () => {
  const arm = { x: 0, y: 0, w: 100, h: 200, side: 'L' as const }
  const torso = { x: 98, y: 0, w: 200, h: 200, side: null }
  const raster = (r: number, g: number, b: number) => ({ width: 200, height: 200,
    pixels: new Uint8ClampedArray(Array.from({ length: 40000 }, () => [r, g, b, 255]).flat()) })
  const skin = raster(245, 200, 185)
  const rest = new Float32Array([99, 50, 0, 190])
  const weights = shoulderContactWeights(arm, skin, torso, skin, rest)
  assert.ok(weights)
  assert.equal(weights[0], 1)
  assert.equal(weights[1], 0)
  assert.equal(shoulderContactWeights(arm, skin, { ...torso, x: 150 }, skin, rest), null)
  assert.equal(shoulderContactWeights(arm, raster(70, 70, 110), torso, skin, rest), null)
  assert.equal(shoulderContactWeights(arm, null, torso, skin, rest), null)
})

test('the visible torso edge inside an overlapping arm is pinned, not only the arm cut edge', () => {
  const arm = { x: 0, y: 0, w: 100, h: 200, side: 'L' as const }
  const torso = { x: 40, y: 0, w: 200, h: 200, side: null }
  const skin = { width: 100, height: 200, pixels: new Uint8ClampedArray(100 * 200 * 4) }
  for (let i = 0; i < skin.pixels.length; i += 4) skin.pixels.set([245, 200, 185, 255], i)
  const rest = new Float32Array([40, 50, 60, 50, 99, 50, 0, 200])
  const weights = shoulderContactWeights(arm, skin, torso, skin, rest)!
  assert.ok(weights)
  assert.deepEqual(Iterator.from(weights).toArray(), [1, 1, 1, 0])
  // Mirroring preserves the same overlap constraint on the other shoulder.
  const mirrored = shoulderContactWeights(
    { ...arm, x: 140, side: 'R' }, skin,
    { ...torso, x: 0 }, skin,
    new Float32Array([200, 50, 180, 50, 141, 50, 240, 200]),
  )!
  assert.deepEqual(mirrored, weights)
})

test('overlap expansion stops at material changes and transparent gaps', () => {
  const arm = { x: 0, y: 0, w: 100, h: 200, side: 'L' as const }
  const torso = { ...arm, x: 20, w: 200, side: null }
  const skin = { width: 100, height: 200, pixels: new Uint8ClampedArray(100 * 200 * 4) }
  for (let i = 0; i < skin.pixels.length; i += 4) skin.pixels.set([245, 200, 185, 255], i)
  for (const color of [[245, 200, 185, 0], [60, 60, 100, 255]]) {
    const pixels = { ...skin, pixels: skin.pixels.slice() }
    for (let y = 0; y < 200; y++) { for (let x = 65; x <= 70; x++) pixels.pixels.set(color, (y * 100 + x) * 4)
}
    const weights = shoulderContactWeights(arm, pixels, torso, skin, new Float32Array([30, 50, 95, 50]))!
    assert.ok(weights)
    assert.ok(weights[0] < 0.7, 'must not jump a gap to pin another skin island')
    assert.equal(weights[1], 1)
  }
})

test('real atlas exposes both shoulder contacts', { skip: !process.env.MEROPE_SHOULDER_ASSET }, async () => {
  const sharp = (await import('sharp')).default
  const root = process.env.MEROPE_SHOULDER_ASSET!
  const manifest = JSON.parse(await readFile(`${root}/manifest.json`, 'utf8'))
  const layers = manifest.anime25dPlayback.layers
  const atlas = `${root}/atlas.png`
  const meta = await sharp(atlas).metadata()
  const crop = async (layer: any) => {
    const { data, info } = await sharp(atlas).extract({ left: Math.round(layer.atlas.x * meta.width!),
      top: Math.round(layer.atlas.y * meta.height!), width: Math.round(layer.atlas.w * meta.width!),
      height: Math.round(layer.atlas.h * meta.height!) }).ensureAlpha().raw().toBuffer({ resolveWithObject: true })
    return { pixels: new Uint8ClampedArray(data), width: info.width, height: info.height }
  }
  const torso = layers.find((l: any) => l.role === 'topwear')
  const body = await crop(torso)
  for (const arm of layers.filter((l: any) => l.role === 'handwear')) {
    const rest = new Float32Array(Array.from({ length: 100 }, (_, i) =>
      [arm.x + arm.w * (i % 10) / 9, arm.y + arm.h * Math.floor(i / 10) / 9]).flat())
    const weights = shoulderContactWeights(arm, await crop(arm), torso, body, rest)
    assert.ok(weights, arm.name)
    assert.ok(Math.max(...weights) > 0.99, arm.name)
    assert.ok(Math.min(...weights) < 0.01, arm.name)
  }
})
