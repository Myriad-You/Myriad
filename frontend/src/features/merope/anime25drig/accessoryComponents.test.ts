import type { Anime25DPlaybackLayer } from './types'
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'
import { removeDuplicatedNeckComponents } from './accessoryComponents'
import { IDENTITY_DRIVER } from './driver'
import {
  compileAnime25DGpuLayers,
  disposeAnime25DGpuLayers,
} from './layerGpuBinding'
import { deriveAnime25DShellProfile } from './shellProfile'

function fixture() {
  const source = {
    role: 'headwear',
    x: 0,
    y: 0,
    w: 32,
    h: 48,
  } as Anime25DPlaybackLayer
  const layer = { ...source, role: 'neckwear' }
  const art = {
    width: 32,
    height: 48,
    pixels: new Uint8ClampedArray(32 * 48 * 4),
  }
  const image = { ...art, pixels: art.pixels.slice() }
  for (const top of [0, 32]) {
    for (let y = top; y < top + 8; y++) {
      for (let x = 4; x < 12; x++) {
        const i = (y * 32 + x) * 4
        const v = 100 + ((x * 31 + y * 17) % 130)
        art.pixels.set([v, v - 10, v - 20, 255], i)
        if (top === 32) image.pixels.set(art.pixels.subarray(i, i + 4), i)
      }
}
}
  return { source, layer, art, image }
}

test('removes only a co-located disconnected neck duplicate including its alpha fringe', () => {
  const { source, layer, art, image } = fixture()
  art.pixels.set([120, 110, 100, 12], (32 * 32 + 3) * 4)
  const original = art.pixels.slice()
    const target = image.pixels.slice()
  const patch = removeDuplicatedNeckComponents(
    source,
    art,
    [{ layer, image }],
    24,
  )
  assert.ok(patch)
  for (let i = 0; i < art.pixels.length; i++) {
    assert.equal(
      patch.pixels[i],
      i % 4 === 3 && i >= 32 * 32 * 4 ? 0 : original[i],
    )
  }
  assert.deepEqual(art.pixels, original)
  assert.deepEqual(image.pixels, target)
})

test('retains different detail, flat colours, insufficient coverage and distant matching ornaments', () => {
  for (const kind of [
    'detail',
    'flat',
    'coverage',
    'offset',
    'single',
    'above-neck',
    'animated',
  ]) {
    const { source, layer, art, image } = fixture()
    if (kind === 'detail') {
      for (let y = 32; y < 40; y++) {
        for (let x = 4; x < 12; x++) {
          const i = (y * 32 + x) * 4
          for (let c = 0; c < 3; c++)
            image.pixels[i + c] = 330 - art.pixels[i + c]
        }
}
}
    if (kind === 'flat') {
      for (let i = 0; i < art.pixels.length; i += 4) {
        art.pixels.set([200, 200, 200], i)
        image.pixels.set([200, 200, 200], i)
      }
}
    if (kind === 'coverage') image.pixels.fill(0, 36 * 32 * 4)
    if (kind === 'offset') layer.x += 8
    if (kind === 'single') art.pixels.fill(0, 0, 24 * 32 * 4)
    if (kind === 'animated') layer.fade = 'eyeClose'
    assert.equal(
      removeDuplicatedNeckComponents(
        source,
        art,
        [{ layer, image }],
        kind === 'above-neck' ? 40 : 24,
      ),
      null,
      kind,
    )
  }
})

test('one-texel segmentation displacement and small colour differences remain recognizable', () => {
  const { source, layer, art, image } = fixture()
  const shifted = {
    ...image,
    pixels: new Uint8ClampedArray(image.pixels.length),
  }
  for (let y = 32; y < 40; y++) {
    for (let x = 4; x < 12; x++) {
      const i = (y * 32 + x) * 4
        const j = i + 32 * 4
      shifted.pixels.set(
        [
          image.pixels[i] + 5,
          image.pixels[i + 1] + 5,
          image.pixels[i + 2] + 5,
          255,
        ],
        j,
      )
    }
}
  assert.ok(
    removeDuplicatedNeckComponents(
      source,
      art,
      [{ layer, image: shifted }],
      24,
    ),
  )
})

test(
  'real split asset loses only the stray neck flower, preserving the hair ornament and neckwear',
  { skip: !process.env.MEROPE_ACCESSORY_ASSET },
  async () => {
    const sharp = (await import('sharp')).default
    const root = process.env.MEROPE_ACCESSORY_ASSET!
    const m = JSON.parse(await readFile(`${root}/manifest.json`, 'utf8'))
    const playback = m.anime25dPlayback
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
    const source = playback.layers.find(
      (l: Anime25DPlaybackLayer) => l.role === 'headwear',
    )
    const layer = playback.layers.find(
      (l: Anime25DPlaybackLayer) => l.role === 'neckwear',
    )
    const art = await crop(source)
      const image = await crop(layer)
    const original = art.pixels.slice()
      const neck = image.pixels.slice()
    const patch = removeDuplicatedNeckComponents(
      source,
      art,
      [{ layer, image }],
      playback.anchors.neckTop,
    )
    assert.ok(patch)
    let removed = 0
      let retained = 0
    for (let i = 0; i < original.length; i++) {
      if (i % 4 !== 3) {
        assert.equal(patch.pixels[i], original[i])
        continue
      }
      if (original[i] !== patch.pixels[i]) {
        removed++
        assert.equal(patch.pixels[i], 0)
        const wy =
          source.y +
          ((Math.floor(i / 4 / art.width) + 0.5) / art.height) * source.h
        assert.ok(wy >= playback.anchors.neckTop)
      } else if (original[i]) { retained++
}
    }
    assert.ok(
      removed > 1000 && retained > removed,
      JSON.stringify({ removed, retained }),
    )
    assert.deepEqual(art.pixels, original)
    assert.deepEqual(image.pixels, neck)
    assert.equal(
      removeDuplicatedNeckComponents(
        source,
        patch,
        [{ layer, image }],
        playback.anchors.neckTop,
      ),
      null,
    )
    // Exercise the production compilation path, not only the pixel helper:
    // the texture patch and attachment footprint must both use the cleaned art.
    const raw = await sharp(`${root}/atlas.png`).ensureAlpha().raw().toBuffer()
    const previousDocument = globalThis.document
    Object.assign(globalThis, {
      document: {
        createElement: () => {
          let left = 0
            let top = 0
          return {
            getContext: () => ({
              drawImage(_atlas: unknown, x: number, y: number) {
                left = x
                top = y
              },
              getImageData(_x: number, _y: number, w: number, h: number) {
                const data = new Uint8ClampedArray(w * h * 4)
                for (let y = 0; y < h; y++) {
                  const start = ((top + y) * meta.width! + left) * 4
                  data.set(raw.subarray(start, start + w * 4), y * w * 4)
                }
                return { data }
              },
            }),
          }
        },
      },
    })
    const gl = {
      ARRAY_BUFFER: 1,
      ELEMENT_ARRAY_BUFFER: 2,
      DYNAMIC_DRAW: 3,
      STATIC_DRAW: 4,
      FLOAT: 5,
      createVertexArray: () => ({}),
      createBuffer: () => ({}),
      getAttribLocation: () => 0,
      bindVertexArray() {},
      bindBuffer() {},
      bufferData() {},
      enableVertexAttribArray() {},
      vertexAttribPointer() {},
      deleteBuffer() {},
      deleteVertexArray() {},
    } as unknown as WebGL2RenderingContext
    let compiled: ReturnType<typeof compileAnime25DGpuLayers> | undefined
    try {
      compiled = compileAnime25DGpuLayers(
        gl,
        {} as WebGLProgram,
        playback,
        deriveAnime25DShellProfile(playback),
        { ...IDENTITY_DRIVER },
        null,
        { width: meta.width, height: meta.height } as HTMLImageElement,
      )
      const upload = compiled.atlasPatches?.find(
        (p) =>
          p.x === Math.round(source.atlas.x * meta.width!) &&
          p.y === Math.round(source.atlas.y * meta.height!),
      )
      assert.deepEqual(upload?.pixels, patch.pixels)
      for (const p of compiled.atlasPatches ?? []) {
        if (p === upload) continue
        for (let y = 0; y < p.height; y++) {
          for (let x = 0; x < p.width; x++) {
            assert.equal(
              p.pixels[(y * p.width + x) * 4 + 3],
              raw[((p.y + y) * meta.width! + p.x + x) * 4 + 3],
              'body fusion must never cut alpha holes',
            )
}
}
      }
      const neckLayer = compiled.layers.find((l) => l.source.role === 'neck')!
      const bodyLayer = compiled.layers.find(
        (l) => l.source.role === 'topwear',
      )!
      assert.ok(
        neckLayer.neckSurfaceFade,
        'this real asset must enable the open-neck repair',
      )
      assert.ok(
        compiled.layers.indexOf(neckLayer) > compiled.layers.indexOf(bodyLayer),
      )
      const ornament = compiled.layers.find((l) => l.source === source)
      assert.ok(ornament?.attachment)
      assert.ok(
        ornament.attachment.y < playback.anchors.neckTop,
        'the deleted flower must no longer pull the hair attachment toward the neck',
      )
    } finally {
      if (compiled) disposeAnime25DGpuLayers(gl, compiled)
      Object.assign(globalThis, { document: previousDocument })
    }
  },
)
