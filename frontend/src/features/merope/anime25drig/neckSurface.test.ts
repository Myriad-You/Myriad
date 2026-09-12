import type {
  Anime25DPlayback,
  Anime25DPlaybackAnchors,
  Anime25DPlaybackLayer,
} from './types'
import type { CroppedLayerPixels } from './webglRuntime'
import assert from 'node:assert/strict'
import test from 'node:test'
import { IDENTITY_DRIVER } from './driver'
import {
  compileAnime25DGpuLayers,
  disposeAnime25DGpuLayers,
} from './layerGpuBinding'
import { resolveAnime25DNeckSurface } from './neckSurface'
import { deriveAnime25DShellProfile } from './shellProfile'

const anchors: Anime25DPlaybackAnchors = {
  face: { x0: 300, x1: 550, y0: 250, y1: 520, cx: 425, cy: 385 },
  neckTop: 500,
  neckBottom: 600,
  neckPivot: { x: 420, y: 580 },
  bodyPivot: { x: 420, y: 1000 },
  faceScale: 1,
  mouth: { x0: 400, x1: 440, y0: 470, y1: 480, cx: 420, cy: 475 },
}
const skin = [245, 210, 200, 255]

function layer(
  role: string,
  x: number,
  y: number,
  w: number,
  h: number,
): Anime25DPlaybackLayer {
  return {
    name: role,
    role,
    x,
    y,
    w,
    h,
    depth: 1,
    group: 'body',
    phys: null,
    fade: null,
    side: null,
    strands: [],
    atlas: { x: 0, y: 0, w: 1, h: 1 },
  }
}

function pixels(
  w: number,
  h: number,
  colour: (x: number, y: number) => number[],
): CroppedLayerPixels {
  const data = new Uint8ClampedArray(w * h * 4)
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) data.set(colour(x, y), (y * w + x) * 4)
  }
  return { width: w, height: h, pixels: data }
}

function fixture() {
  const neck = layer('neck', 400, 500, 40, 100)
  const body = layer('topwear', 390, 560, 60, 100)
  const neckPixels = pixels(40, 100, (_x, y) =>
    y < 70
      ? [190, 150, 140, 255]
      : y <= 96
        ? skin
        : [120, 100, 90, y === 97 ? 195 : y === 98 ? 65 : 0],
  )
  const bodyPixels = pixels(60, 100, () => skin)
  const images = new Map([
    [neck, neckPixels],
    [body, bodyPixels],
  ])
  return {
    neck,
    body,
    neckPixels,
    bodyPixels,
    images,
    layers: [neck, body],
    read: (source: Anime25DPlaybackLayer) => images.get(source) ?? null,
  }
}

test('a short exact overlap seeds a wider shadow gradient without disabling the neck repair', () => {
  const f = fixture()
  for (let y = 70; y < 92; y++) {
    for (let x = 0; x < 40; x++) {
      f.neckPixels.pixels.set([227, 192, 182, 255], (y * 40 + x) * 4)
    }
}
  const plan = resolveAnime25DNeckSurface(f.layers, anchors, f.read)
  assert.ok(plan)
  assert.equal(plan.fadeStart, 0.705)
  assert.equal(plan.fadeEnd, 0.965)
  // Similar shading without the strongly matching seed is still insufficient.
  for (let y = 92; y <= 96; y++) {
    for (let x = 0; x < 40; x++) {
      f.neckPixels.pixels.set([227, 192, 182, 255], (y * 40 + x) * 4)
    }
}
  assert.equal(resolveAnime25DNeckSurface(f.layers, anchors, f.read), null)
})

test('open-neck overlap restores the shadow and fades before the dark cut matte', () => {
  const f = fixture()
  const before = f.neckPixels.pixels.slice()
  const plan = resolveAnime25DNeckSurface(f.layers, anchors, f.read)
  assert.ok(plan)
  assert.equal(plan.neck, f.neck)
  assert.equal(plan.body, f.body)
  assert.equal(plan.fadeStart, 0.705)
  assert.equal(plan.fadeEnd, 0.965)
  assert.ok(
    plan.fadeEnd < 0.97,
    'exclude contaminated antialiasing, not just fully transparent pixels',
  )
  assert.deepEqual(
    f.neckPixels.pixels,
    before,
    'source artwork is never modified',
  )
  assert.deepEqual(
    f.layers,
    [f.neck, f.body],
    'stored playback order is not mutated',
  )
})

test('compiled collars bypass all open-neck pixel inference', () => {
  const f = fixture()
  for (const role of ['collar-front', 'collar-back']) {
    assert.equal(
      resolveAnime25DNeckSurface(
        [...f.layers, layer(role, 390, 530, 60, 70)],
        anchors,
        () => {
          throw new Error('collar topology must win before sampling')
        },
      ),
      null,
    )
  }
})

test('an unsplit pale high collar cannot qualify merely by resembling skin', () => {
  const f = fixture()
  f.body.y = 510
  assert.equal(resolveAnime25DNeckSurface(f.layers, anchors, f.read), null)
})

test('an open collar with a local pendant shadow uses a strongly seeded blend band', () => {
  const f = fixture()
  f.body.y = 532
  f.images.set(
    f.neck,
    pixels(40, 100, (x, y) => {
      if (y >= 80 && y < 90 && (x < 30 || (y >= 83 && y <= 85))) return skin
      return [170, 130, 120, 255]
    }),
  )
  const plan = resolveAnime25DNeckSurface(f.layers, anchors, f.read)
  assert.ok(plan)
  assert.equal(plan.fadeStart, 0.805)
  assert.equal(plan.fadeEnd, 0.895)

  // Broad but merely approximate agreement cannot seed a skin replacement.
  f.images.set(
    f.neck,
    pixels(40, 100, (x, y) =>
      y >= 80 && y < 90 && x < 30 ? skin : [170, 130, 120, 255],
    ),
  )
  assert.equal(resolveAnime25DNeckSurface(f.layers, anchors, f.read), null)
})

test('a tiny gap in an otherwise closed upper garment is not an open aperture', () => {
  const f = fixture()
  f.body.y = 510
  f.images.set(
    f.body,
    pixels(60, 100, (_x, y) => (y >= 10 && y < 13 ? [0, 0, 0, 0] : skin)),
  )
  assert.equal(resolveAnime25DNeckSurface(f.layers, anchors, f.read), null)
})

test('a coloured garment, thin matching stripe, or unsupported cut edge stays unchanged', () => {
  for (const kind of ['colour', 'stripe', 'hole', 'translucent']) {
    const f = fixture()
    f.images.set(
      f.body,
      pixels(60, 100, (x, y) => {
        if (kind === 'colour' || (kind === 'stripe' && y < 35))
          return [40, 80, 180, 255]
        if (kind === 'hole' && x === 30 && y === 38) return [0, 0, 0, 0]
        return kind === 'translucent' ? [...skin.slice(0, 3), 200] : skin
      }),
    )
    assert.equal(
      resolveAnime25DNeckSurface(f.layers, anchors, f.read),
      null,
      kind,
    )
  }
})

test('ambiguous neck/body pairs and unavailable pixels are never guessed', () => {
  const f = fixture()
  const other = { ...f.body, name: 'topwear-2' }
  f.images.set(other, f.bodyPixels)
  assert.equal(
    resolveAnime25DNeckSurface([...f.layers, other], anchors, f.read),
    null,
  )
  assert.equal(
    resolveAnime25DNeckSurface([...f.layers, { ...f.neck }], anchors, f.read),
    null,
  )
  assert.equal(resolveAnime25DNeckSurface([f.body], anchors, f.read), null)
  assert.equal(
    resolveAnime25DNeckSurface(f.layers, anchors, () => null),
    null,
  )
  f.images.delete(f.body)
  assert.equal(resolveAnime25DNeckSurface(f.layers, anchors, f.read), null)
})

test('UV join is independent of canvas scale and position', () => {
  const f = fixture()
  const expected = resolveAnime25DNeckSurface(f.layers, anchors, f.read)!
  for (const source of f.layers) {
    source.x = source.x * 2 + 31
    source.y = source.y * 2 + 19
    source.w *= 2
    source.h *= 2
  }
  const actual = resolveAnime25DNeckSurface(
    f.layers,
    {
      ...anchors,
      face: { ...anchors.face, y1: anchors.face.y1 * 2 + 19 },
      neckBottom: anchors.neckBottom * 2 + 19,
    },
    f.read,
  )
  assert.equal(actual?.fadeStart, expected.fadeStart)
  assert.equal(actual?.fadeEnd, expected.fadeEnd)
  assert.deepEqual(actual?.contour, expected.contour)
})

test('transparent padding preserves the world-space join and contour', () => {
  const f = fixture()
  const expected = resolveAnime25DNeckSurface(f.layers, anchors, f.read)!
  const padded = { ...f.neck, x: 390, y: 480, w: 60, h: 140 }
  const image = pixels(60, 140, (x, y) => {
    if (x < 10 || x >= 50 || y < 20 || y >= 120) return [0, 0, 0, 0]
    return [
      ...f.neckPixels.pixels.subarray(
        ((y - 20) * 40 + x - 10) * 4,
        ((y - 20) * 40 + x - 10) * 4 + 4,
      ),
    ]
  })
  f.images.set(padded, image)
  const actual = resolveAnime25DNeckSurface([padded, f.body], anchors, f.read)!
  assert.ok(actual)
  assert.equal(
    padded.y + actual.fadeStart * padded.h,
    f.neck.y + expected.fadeStart * f.neck.h,
  )
  assert.equal(
    padded.y + actual.fadeEnd * padded.h,
    f.neck.y + expected.fadeEnd * f.neck.h,
  )
  for (let i = 0; i < 32; i++) {
    assert.ok(
      Math.abs(
        padded.y +
          actual.contour.bands[i] * padded.h -
          f.neck.y -
          expected.contour.bands[i] * f.neck.h,
      ) < 0.00002,
    )
  }
})

test('resampling and mild colour noise do not switch open skin into garment topology', () => {
  const f = fixture()
  const expected = resolveAnime25DNeckSurface(f.layers, anchors, f.read)!
  for (const scale of [1, 2, 3]) {
    const read = (source: Anime25DPlaybackLayer) => {
      const image = f.read(source)!
      return pixels(image.width * scale, image.height * scale, (x, y) => {
        const offset =
          (Math.floor(y / scale) * image.width + Math.floor(x / scale)) * 4
        return Iterator.from(image.pixels.subarray(offset, offset + 4))
          .map((v, i) => (i < 3 ? v + (((x + y) % 3) - 1) : v))
          .toArray()
      })
    }
    const actual = resolveAnime25DNeckSurface(f.layers, anchors, read)!
    assert.ok(actual)
    assert.ok(Math.abs(actual.fadeStart - expected.fadeStart) <= 0.006)
    assert.ok(Math.abs(actual.fadeEnd - expected.fadeEnd) <= 0.006)
  }
})

for (const jewelryOrder of ['before-neck', 'before-body', 'after-body']) {
  test(`GPU binding recovers neck and jewelry occlusion from ${jewelryOrder}`, () => {
    const f = fixture()
    const jewelry = layer('neckwear', 410, 595, 20, 30)
    f.neck.atlas = { x: 0, y: 0, w: 0.4, h: 1 }
    f.body.atlas = { x: 0.4, y: 0, w: 0.6, h: 1 }
    jewelry.atlas = { x: 0, y: 0, w: 0.2, h: 0.3 }
    const importedLayers =
      jewelryOrder === 'before-neck'
        ? [jewelry, f.neck, f.body]
        : jewelryOrder === 'before-body'
          ? [f.neck, jewelry, f.body]
          : [f.neck, f.body, jewelry]
    const playback = {
      layers: Iterator.from(importedLayers).toArray(),
      anchors,
      pixelCanvas: { width: 1024, height: 1365 },
    } as Anime25DPlayback
    const previousDocument = globalThis.document
    let reads = 0
    Object.assign(globalThis, {
      document: {
        createElement: () => {
          let sourceX = 0
          return {
            getContext: () => ({
              drawImage(_image: unknown, x: number) {
                sourceX = x
              },
              getImageData(_x: number, _y: number, w: number, h: number) {
                reads++
                return {
                  data:
                    w === 40
                      ? f.neckPixels.pixels
                      : sourceX === 40
                        ? f.bodyPixels.pixels
                        : pixels(w, h, () => skin).pixels,
                }
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
        { width: 100, height: 100 } as HTMLImageElement,
      )
      assert.equal(compiled.collarClip, null)
      assert.deepEqual(
        compiled.layers.map((l) => l.source.role),
        ['topwear', 'neck', 'neckwear'],
      )
      assert.equal(compiled.layers[1].neckSurfaceFade?.start, 0.705)
      assert.equal(compiled.layers[1].neckSurfaceFade?.end, 0.965)
      assert.equal(
        compiled.layers[1].neckSurfaceFade?.contour?.bands.length,
        32,
      )
      assert.equal(compiled.layers[0].neckSurfaceFade, undefined)
      assert.equal(compiled.layers[2].neckSurfaceFade, undefined)
      assert.equal(compiled.layers[2].attachment?.hostName, 'topwear')
      assert.equal(
        reads,
        3,
        'seam and attachment share a transient pixel cache',
      )
      assert.deepEqual(playback.layers, importedLayers)
    } finally {
      if (compiled) disposeAnime25DGpuLayers(gl, compiled)
      Object.assign(globalThis, { document: previousDocument })
    }
  })
}
