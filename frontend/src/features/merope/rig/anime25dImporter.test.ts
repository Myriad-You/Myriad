import type { Layer, Psd } from 'ag-psd'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  anime25DBaseRole,
  gridMesh,
  isAnime25DDocument,
  normalizeAnime25DLayerName,
  prepareAnime25DRigPsd,
} from './anime25dImporter'

test('matches Anime2.5DRig normalization without merging numbered hair groups', () => {
  assert.equal(normalizeAnime25DLayerName('mouth'), 'mouth-open')
  assert.equal(normalizeAnime25DLayerName('eyelash_c'), 'eye-close')
  assert.equal(
    normalizeAnime25DLayerName('Front Hair_2 のコピー 3'),
    'front-hair-2',
  )
  assert.equal(anime25DBaseRole('front-hair-2'), 'front-hair')
})

test('maps native See-through PSD tags into stable FaceRig roles', () => {
  assert.equal(normalizeAnime25DLayerName('hairf'), 'front-hair')
  assert.equal(normalizeAnime25DLayerName('hairb'), 'back-hair')
  assert.equal(normalizeAnime25DLayerName('eyer'), 'eyelash-r')
  assert.equal(normalizeAnime25DLayerName('browl'), 'eyebrow-l')
  assert.equal(normalizeAnime25DLayerName('eyebg'), 'eyewhite')
  assert.equal(anime25DBaseRole('eyewhite-r'), 'eyewhite')
  assert.equal(anime25DBaseRole('handwear-l'), 'handwear')
})

test('explicit layer grids contain stable interior topology', () => {
  const mesh = gridMesh(
    { x: 0.1, y: 0.2, width: 0.5, height: 0.6 },
    'front-hair',
  )
  assert.ok(mesh.vertices.length > 40)
  assert.ok(mesh.indices.length > mesh.vertices.length)
  assert.equal(mesh.indices.length % 3, 0)
  assert.ok(mesh.indices.every((index) => index < mesh.vertices.length))
})

test('see-through PSD builds blink, mouth, strand, chest, and rigid side-arm fragments without limb IK', async () => {
  const psd = syntheticSeeThroughPsd()
  assert.equal(isAnime25DDocument(psd), true)
  const previousDocument = globalThis.document
  const previousImageData = globalThis.ImageData
  class FakeImageData {
    constructor(
      readonly data: Uint8ClampedArray,
      readonly width: number,
      readonly height: number,
    ) {}
  }
  class FakeCanvas {
    width = 0
    height = 0
    readonly context = {
      drawCount: 0,
      putImageData() {},
      drawImage: () => {
        this.context.drawCount += 1
      },
    }

    getContext() {
      return this.context
    }

    toBlob(callback: (value: Blob) => void) {
      callback(new Blob([new Uint8Array([1])], { type: 'image/png' }))
    }
  }
  const canvases: FakeCanvas[] = []
  Object.assign(globalThis, {
    ImageData: FakeImageData,
    document: {
      createElement: () => {
        const canvas = new FakeCanvas()
        canvases.push(canvas)
        return canvas
      },
    },
  })
  try {
    const prepared = await prepareAnime25DRigPsd(psd, 'master')
    const boneIds = prepared.source.bones.map((bone) => bone.id)
    const layers = prepared.source.layers
    const bone = (id: string) =>
      prepared.source.bones.find((candidate) => candidate.id === id)
    assert.equal(prepared.partCount >= 15, true)
    assert.equal(prepared.analysisReference.type, 'image/png')
    const neutralLayerCount = layers.filter(
      (layer) =>
        !(
          (layer.slot === 'eye-left' || layer.slot === 'eye-right') &&
          layer.variant === 'closed'
        ) &&
        !(layer.slot === 'mouth' && layer.variant === 'open'),
    ).length
    assert.equal(canvases[0]?.context.drawCount, layers.length)
    assert.equal(canvases[1]?.context.drawCount, neutralLayerCount)
    assert.equal(
      boneIds.some((id) => /upper-arm|forearm|wrist|thigh/.test(id)),
      false,
    )
    assert.ok(boneIds.includes('a25d-handwear'))
    assert.equal(bone('a25d-handwear-left')?.parent, 'a25d-handwear')
    assert.equal(bone('a25d-handwear-right')?.parent, 'a25d-handwear')
    assert.ok(boneIds.includes('a25d-chest'))
    assert.ok(boneIds.includes('a25d-eyelash-left'))
    assert.ok(boneIds.includes('a25d-eyelash-right'))
    assert.equal(bone('a25d-eyewhite-left')?.parent, 'face')
    assert.equal(bone('a25d-eyelash-left')?.parent, 'face')
    assert.equal(bone('a25d-irides-left')?.parent, 'left-eye')
    assert.ok(boneIds.some((id) => id.endsWith('hair-root')))
    assert.ok(boneIds.some((id) => id.endsWith('hair-tip')))
    assert.ok(
      layers.some(
        (layer) =>
          layer.id === 'a25d-eye-close-left' &&
          layer.slot === 'eye-left' &&
          layer.variant === 'closed',
      ),
      'separate closed-eye artwork is crossfaded',
    )
    assert.ok(
      layers.some(
        (layer) =>
          layer.id === 'a25d-mouth-close' &&
          layer.slot === 'mouth' &&
          layer.variant === 'closed',
      ),
      'separate closed-mouth artwork is crossfaded',
    )
    assert.ok(
      layers
        .filter((layer) => /hair|topwear|handwear/.test(layer.id))
        .every((layer) => (layer.mesh?.vertices.length || 0) > 8),
    )
    const z = (id: string) =>
      layers.find((layer) => layer.id === id)?.zIndex ?? Number.NaN
    assert.ok(z('a25d-back-hair') < z('a25d-face'))
    assert.ok(z('a25d-face') < z('a25d-front-hair-1'))
    assert.ok(z('a25d-handwear-left') < z('a25d-topwear'))
    assert.ok(z('a25d-handwear-right') < z('a25d-topwear'))
    const leftArmHandles = layers.find(
      (layer) => layer.id === 'a25d-handwear-left',
    )?.boneHandles
    assert.equal(leftArmHandles?.length, 1)
    assert.equal(leftArmHandles?.[0].boneId, 'a25d-handwear-left')
    assert.ok(z('a25d-eyewhite-left') < z('a25d-eyelash-left'))
    assert.equal(
      layers.some((layer) => layer.id.includes('legwear')),
      false,
    )
    assert.deepEqual(prepared.source.semantics?.chains, {
      torso: ['root', 'body', 'head'],
    })
    const playback = prepared.source.anime25dPlayback
    assert.ok(playback)
    assert.equal(playback.anchors.bodyPivot.y, playback.pixelCanvas.height)
    assert.equal(playback.anchors.bodyPivot.x, playback.anchors.neckPivot.x)
    const white = playback.layers.find(
      (item) => item.role === 'eyewhite' && item.side === 'L',
    )
    assert.ok(white)
    assert.notEqual(playback.anchors.eyeL?.closeY, white.y + white.h * 0.62)
    assert.ok(playback.layers.every((item, index) => item.z === index))
  } finally {
    Object.assign(globalThis, {
      document: previousDocument,
      ImageData: previousImageData,
    })
  }
})

test('semantic content framing removes letterboxing and pads into the 3:4 stage', async () => {
  const generationFingerprint = 'a'.repeat(64)
  const source = syntheticSeeThroughPsd()
  const padded = {
    ...source,
    width: 384,
    children: source.children?.map((layer) => ({
      ...layer,
      left: (layer.left || 0) + 64,
    })),
  } as Psd
  const prepared = await prepareWithFakeCanvas(padded, generationFingerprint)
  assert.deepEqual(prepared.source.canvas, {
    width: 1,
    height: 1.3333333333333333,
  })
  assert.equal(prepared.source.characterAssetContractVersion, 3)
  assert.equal(
    prepared.source.sourceGenerationFingerprint,
    generationFingerprint,
  )
  assert.ok(
    prepared.source.layers
      .filter((layer) => !layer.id.includes('bottomwear'))
      .flatMap((layer) => layer.mesh?.vertices || [])
      .every(
        (vertex) =>
          vertex.x >= 0 &&
          vertex.x <= 1 &&
          vertex.y >= 0 &&
          vertex.y <= prepared.source.canvas.height,
      ),
  )
})

test('preflight rejects a PSD that cannot satisfy the rigid two-arm contract', async () => {
  const source = syntheticSeeThroughPsd()
  const withoutArms = {
    ...source,
    children: source.children?.filter((layer) => layer.name !== 'handwear'),
  } as Psd
  await assert.rejects(
    () => prepareWithFakeCanvas(withoutArms),
    /left\/right sleeve-forearm-hand fragments/,
  )
})

test('unknown layers follow rigger head/body split by centroid vs chin', async () => {
  const source = syntheticSeeThroughPsd()
  const withUnknown = {
    ...source,
    children: [
      ...(source.children || []),
      unknownBlob('ribbon', 100, 50, 140, 80),
      unknownBlob('sash', 90, 190, 160, 230),
    ],
  } as Psd
  const prepared = await prepareWithFakeCanvas(withUnknown)
  const playback = prepared.source.anime25dPlayback
  assert.ok(playback)
  assert.equal(
    playback.layers.find(
      (layer) => layer.role === 'unknown' && layer.name.includes('ribbon'),
    )?.group,
    'head',
  )
  assert.equal(
    playback.layers.find(
      (layer) => layer.role === 'unknown' && layer.name.includes('sash'),
    )?.group,
    'body',
  )
})

test('missing close-eye and close-mouth layers get Anime2.5DRig generic diffs', async () => {
  const source = syntheticSeeThroughPsd()
  const withoutClosedArtwork = {
    ...source,
    children: source.children?.filter(
      (layer) => layer.name !== 'eyelash_c' && layer.name !== 'mouth_c',
    ),
  } as Psd
  const prepared = await prepareWithFakeCanvas(withoutClosedArtwork)
  assert.ok(
    prepared.source.layers.some(
      (layer) => layer.slot === 'eye-left' && layer.variant === 'closed',
    ),
  )
  assert.ok(
    prepared.source.layers.some(
      (layer) => layer.slot === 'mouth' && layer.variant === 'closed',
    ),
  )
})

async function prepareWithFakeCanvas(
  psd: Psd,
  sourceGenerationFingerprint?: string,
) {
  const previousDocument = globalThis.document
  const previousImageData = globalThis.ImageData
  class FakeImageData {
    constructor(
      readonly data: Uint8ClampedArray,
      readonly width: number,
      readonly height: number,
    ) {}
  }
  class FakeCanvas {
    width = 0
    height = 0
    readonly context = { putImageData() {}, drawImage() {} }
    getContext() {
      return this.context
    }

    toBlob(callback: (value: Blob) => void) {
      callback(new Blob([new Uint8Array([1])], { type: 'image/png' }))
    }
  }
  Object.assign(globalThis, {
    ImageData: FakeImageData,
    document: { createElement: () => new FakeCanvas() },
  })
  try {
    return await prepareAnime25DRigPsd(
      psd,
      'master',
      undefined,
      sourceGenerationFingerprint,
    )
  } finally {
    Object.assign(globalThis, {
      document: previousDocument,
      ImageData: previousImageData,
    })
  }
}

function unknownBlob(
  name: string,
  left: number,
  top: number,
  right: number,
  bottom: number,
): Layer {
  const width = 256
  const height = 256
  const data = new Uint8ClampedArray(width * height * 4)
  for (let y = top; y < bottom; y += 1) {
    for (let x = left; x < right; x += 1) {
      const index = (y * width + x) * 4
      data[index] = 48
      data[index + 1] = 32
      data[index + 2] = 24
      data[index + 3] = 255
    }
  }
  return { name, left: 0, top: 0, imageData: { width, height, data } }
}

function syntheticSeeThroughPsd(): Psd {
  const width = 256
  const height = 256
  const layer = (
    name: string,
    rectangles: Array<[number, number, number, number]>,
  ): Layer => {
    const data = new Uint8ClampedArray(width * height * 4)
    for (const [left, top, right, bottom] of rectangles) {
      for (let y = top; y < bottom; y += 1) {
        for (let x = left; x < right; x += 1) {
          const index = (y * width + x) * 4
          data[index] = 48
          data[index + 1] = 32
          data[index + 2] = 24
          data[index + 3] = 255
        }
      }
    }
    return { name, left: 0, top: 0, imageData: { width, height, data } }
  }
  return {
    width,
    height,
    children: [
      layer('back hair', [[52, 18, 204, 220]]),
      layer('handwear', [
        [12, 132, 72, 244],
        [184, 132, 244, 244],
      ]),
      layer('bottomwear', [[54, 184, 202, 254]]),
      layer('legwear', [[70, 210, 186, 256]]),
      layer('topwear', [[48, 116, 208, 212]]),
      layer('ears', [
        [48, 62, 72, 118],
        [184, 62, 208, 118],
      ]),
      layer('face', [[66, 34, 190, 142]]),
      layer('nose', [[122, 84, 134, 104]]),
      layer('mouth', [[108, 112, 148, 130]]),
      layer('eyewhite', [
        [82, 70, 112, 88],
        [144, 70, 174, 88],
      ]),
      layer('eyelash', [
        [80, 66, 114, 72],
        [142, 66, 176, 72],
      ]),
      layer('eyelash_c', [
        [80, 76, 114, 80],
        [142, 76, 176, 80],
      ]),
      layer('irides', [
        [94, 72, 104, 84],
        [152, 72, 162, 84],
      ]),
      layer('eyebrow', [
        [82, 54, 112, 60],
        [144, 54, 174, 60],
      ]),
      layer('front hair_1', [
        [62, 18, 98, 104],
        [102, 12, 138, 116],
        [142, 18, 184, 102],
      ]),
      layer('mouth_c', [[108, 120, 148, 124]]),
    ],
  } as Psd
}
