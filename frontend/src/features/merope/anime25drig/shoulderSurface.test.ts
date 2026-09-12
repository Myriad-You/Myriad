import type { Anime25DPlaybackLayer } from './types'
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'
import { fuseShoulderSurface } from './shoulderSurface'

test('skin fusion removes the cut colour but preserves every alpha byte and garment', () => {
  const torso = { x: 40, y: 0, w: 160, h: 100 } as Anime25DPlaybackLayer
  const arm = { x: 0, y: 0, w: 100, h: 100 } as Anime25DPlaybackLayer
  const make = (w: number, h: number, rgba: number[]) => ({
    width: w,
    height: h,
    pixels: Uint8ClampedArray.from(
      Array.from({ length: w * h }, () => rgba).flat(),
    ),
  })
  const body = make(160, 100, [250, 215, 205, 255])
  for (let y = 0; y < 100; y++) {
    for (let x = 0; x < 2; x++)
      body.pixels.set([150, 140, 140, x ? 255 : 180], (y * 160 + x) * 4)
}
  const image = make(100, 100, [250, 215, 205, 255])
  const original = body.pixels.slice()
  const patch = fuseShoulderSurface(torso, body, [{ layer: arm, image }])!
  assert.ok(patch)
  assert.deepEqual(Iterator.from(patch.pixels.slice(0, 4)).toArray(), [250, 215, 205, 180])
  for (let i = 3; i < original.length; i += 4)
    assert.equal(patch.pixels[i], original[i])
  assert.deepEqual(
    patch.pixels.slice(70 * 160 * 4),
    original.slice(70 * 160 * 4),
  )
  assert.deepEqual(body.pixels, original)
  for (const rgba of [
    [30, 40, 80, 255],
    [250, 215, 205, 100],
    [180, 135, 115, 255],
  ]) {
    assert.equal(
      fuseShoulderSurface(torso, body, [
        { layer: arm, image: make(100, 100, rgba) },
      ]),
      null,
    )
}
})

test('warm highlights extend a connected skin seam but cannot establish one', () => {
  const torso = { x: 40, y: 0, w: 160, h: 100 } as Anime25DPlaybackLayer
  const arm = { x: 0, y: 0, w: 100, h: 100 } as Anime25DPlaybackLayer
  const make = (w: number, color: number[]) => ({
    width: w,
    height: 100,
    pixels: Uint8ClampedArray.from(
      Array.from({ length: w * 100 }, () => color).flat(),
    ),
  })
  const body = make(160, [255, 245, 240, 255])
  for (let y = 0; y < 100; y++)
    body.pixels.set([150, 140, 140, 255], y * 160 * 4)
  const donor = make(100, [255, 245, 235, 255])
  for (let y = 0; y < 10; y++) {
    for (let x = 0; x < 100; x++)
      donor.pixels.set([255, 253, 247, 255], (y * 100 + x) * 4)
}
  const patch = fuseShoulderSurface(torso, body, [
    { layer: arm, image: donor },
  ])!
  assert.deepEqual(Iterator.from(patch.pixels.slice(0, 4)).toArray(), [255, 253, 247, 255])
  assert.equal(
    fuseShoulderSurface(torso, body, [
      { layer: arm, image: make(100, [255, 253, 247, 255]) },
    ]),
    null,
  )
  donor.pixels.fill(0, 10 * 100 * 4, 11 * 100 * 4)
  const disconnected = fuseShoulderSurface(torso, body, [
    { layer: arm, image: donor },
  ])!
  assert.deepEqual(
    disconnected.pixels.slice(0, 10 * 160 * 4),
    body.pixels.slice(0, 10 * 160 * 4),
  )
})

test(
  'real shoulder fusion is bilateral, alpha-preserving and restricted to supported overlap',
  { skip: !process.env.MEROPE_SHOULDER_ASSET },
  async () => {
    const sharp = (await import('sharp')).default
    const root = process.env.MEROPE_SHOULDER_ASSET!
    const m = JSON.parse(await readFile(`${root}/manifest.json`, 'utf8'))
    const layers = m.anime25dPlayback.layers as Anime25DPlaybackLayer[]
    const meta = await sharp(`${root}/atlas.png`).metadata()
    const crop = async (l: Anime25DPlaybackLayer) => {
      const { data, info } = await sharp(`${root}/atlas.png`)
        .extract({
          left: Math.round(l.atlas.x * meta.width!),
          top: Math.round(l.atlas.y * meta.height!),
          width: Math.round(l.atlas.w * meta.width!),
          height: Math.round(l.atlas.h * meta.height!),
        })
        .ensureAlpha()
        .raw()
        .toBuffer({ resolveWithObject: true })
      return {
        width: info.width,
        height: info.height,
        pixels: new Uint8ClampedArray(data),
      }
    }
    const torso = layers.find((l) => l.role === 'topwear')!
      const body = await crop(torso)
    const arms = await Promise.all(
      layers
        .filter((l) => l.role === 'handwear')
        .map(async (layer) => ({ layer, image: await crop(layer) })),
    )
    const original = body.pixels.slice()
      const patch = fuseShoulderSurface(torso, body, arms)!
    assert.ok(patch)
    const count = [0, 0]
    for (let y = 0; y < body.height; y++) {
      for (let x = 0; x < body.width; x++) {
        const i = (y * body.width + x) * 4
        assert.equal(patch.pixels[i + 3], original[i + 3])
        if (patch.pixels.slice(i, i + 3).every((v, c) => v === original[i + c]))
          continue
        count[x < body.width / 2 ? 0 : 1]++
        const wx = torso.x + ((x + 0.5) / body.width) * torso.w
          const wy = torso.y + ((y + 0.5) / body.height) * torso.h
        assert.ok(
          arms.some(({ layer: a, image: p }) => {
            const ax = Math.floor(((wx - a.x) / a.w) * p.width)
              const ay = Math.floor(((wy - a.y) / a.h) * p.height)
            return (
              ax >= 0 &&
              ay >= 0 &&
              ax < p.width &&
              ay < p.height &&
              p.pixels[(ay * p.width + ax) * 4 + 3] >= 250
            )
          }),
        )
      }
}
    assert.ok(
      count.every((n) => n > 400),
      JSON.stringify(count),
    )
    assert.deepEqual(body.pixels, original)
  },
)
