import type { Layer, Psd } from 'ag-psd'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  anime25DBaseRole,
  isAnime25DDocument,
  normalizeAnime25DLayerName,
  prepareAnime25DRigPsd,
} from './anime25dImporter'
import { gridMesh } from './anime25dSkeletonCompiler'
import { CHARACTER_ASSET_CONTRACT_VERSION } from './contract'

test('matches Anime2.5DRig normalization without merging numbered hair groups', () => {
  // PSD 图层名常带首尾空白和「のコピー N」后缀
  assert.equal(
    normalizeAnime25DLayerName(' Front Hair_1 のコピー 2 '),
    'front-hair-1',
  )
  assert.equal(normalizeAnime25DLayerName('mouth'), 'mouth-open')
  assert.equal(normalizeAnime25DLayerName('eyelash_c'), 'eye-close')
  assert.equal(normalizeAnime25DLayerName('eye_dizzy'), 'eye-dizzy')
  assert.equal(normalizeAnime25DLayerName('eye_squeeze'), 'eye-squeeze')
  assert.equal(normalizeAnime25DLayerName('eye_cry'), 'eye-cry')
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
    // Manga accents and the wandering silly irides never enter the neutral
    // reference the design model reads back, even though they ship in the atlas.
    const accentIds = [
      'a25d-maniac-eye-shadow-left',
      'a25d-maniac-eye-shadow-right',
      'a25d-maniac-mouth-shadow',
      'a25d-iris-silly-left',
      'a25d-iris-silly-right',
      'a25d-anger-mark',
      'a25d-speechless-sweat',
      'a25d-lovestruck-heart-left',
      'a25d-lovestruck-heart-right',
      'a25d-lovestruck-face-effect',
      'a25d-lovestruck-drool',
    ]
    const neutralLayerCount = layers.filter(
      (layer) =>
        !(
          (layer.slot === 'eye-left' || layer.slot === 'eye-right') &&
          layer.variant !== 'open'
        ) &&
        !(layer.slot === 'mouth' && layer.variant !== 'closed') &&
        !accentIds.includes(layer.id),
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
          layer.id === 'a25d-eye-dizzy-left' &&
          layer.slot === 'eye-left' &&
          layer.variant === 'dizzy',
      ),
      'independent dizzy-eye artwork is compiled',
    )
    assert.ok(
      layers.some(
        (layer) =>
          layer.id === 'a25d-eye-dizzy-right' &&
          layer.slot === 'eye-right' &&
          layer.variant === 'dizzy',
      ),
      'each eye receives its own dizzy variant',
    )
    assert.ok(
      layers.some(
        (layer) =>
          layer.id === 'a25d-eye-squeeze-left' &&
          layer.slot === 'eye-left' &&
          layer.variant === 'squeeze',
      ),
      'independent screen-left > artwork is compiled',
    )
    assert.ok(
      layers.some(
        (layer) =>
          layer.id === 'a25d-eye-squeeze-right' &&
          layer.slot === 'eye-right' &&
          layer.variant === 'squeeze',
      ),
      'independent screen-right < artwork is compiled',
    )
    assert.ok(
      layers.some(
        (layer) =>
          layer.id === 'a25d-eye-cry-left' &&
          layer.slot === 'eye-left' &&
          layer.variant === 'cry',
      ),
      'screen-left crying artwork is compiled from its own eye anchor',
    )
    assert.ok(
      layers.some(
        (layer) =>
          layer.id === 'a25d-eye-cry-right' &&
          layer.slot === 'eye-right' &&
          layer.variant === 'cry',
      ),
      'screen-right crying artwork is compiled from its own eye anchor',
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
      layers.some(
        (layer) =>
          layer.id === 'a25d-mouth-cry' &&
          layer.slot === 'mouth' &&
          layer.variant === 'cry',
      ),
      'independent crying-mouth artwork is compiled',
    )
    assert.ok(
      layers.some(
        (layer) =>
          layer.id === 'a25d-mouth-maniac' &&
          layer.slot === 'mouth' &&
          layer.variant === 'maniac',
      ),
      'face-scaled maniac laugh artwork is compiled independently',
    )
    assert.ok(
      layers.some((layer) => layer.id === 'a25d-anger-mark'),
      'anger accent is synthesized without replacing the face artwork',
    )
    assert.ok(
      layers.some((layer) => layer.id === 'a25d-speechless-sweat'),
      'speechless sweat accent is synthesized independently',
    )
    assert.ok(
      layers.some((layer) => layer.id === 'a25d-lovestruck-heart-left') &&
        layers.some((layer) => layer.id === 'a25d-lovestruck-heart-right'),
      'each eye receives a separately anchored heart pupil',
    )
    assert.ok(
      layers.some((layer) => layer.id === 'a25d-lovestruck-face-effect'),
      'blush, hatch marks, and sweat share one face-local effect layer',
    )
    assert.ok(
      layers.some((layer) => layer.id === 'a25d-lovestruck-drool'),
      'drool remains separate so it can follow the animated mouth corner',
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
    assert.equal(
      playback.layers.filter((item) => item.fade === 'eyeDizzy').length,
      2,
    )
    assert.equal(
      playback.layers.filter((item) => item.fade === 'eyeSqueeze').length,
      2,
    )
    assert.equal(
      playback.layers.filter((item) => item.fade === 'eyeCry').length,
      2,
    )
    assert.equal(
      playback.layers.filter((item) => item.fade === 'mouthCry').length,
      1,
    )
    assert.equal(
      playback.layers.filter((item) => item.fade === 'mouthManiac').length,
      1,
    )
    assert.equal(playback.mouthProfile.source, 'alpha-contour')
    assert.equal(playback.mouthProfile.silhouettes.length, 6)
    assert.equal(playback.mouthProfile.bridges.length, 15)
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
  assert.equal(
    prepared.source.characterAssetContractVersion,
    CHARACTER_ASSET_CONTRACT_VERSION,
  )
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
    /rigid-left-arm-fragment, rigid-right-arm-fragment/,
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

test('plain See-through mouth becomes closed art while speaking and cry variants are generated', async () => {
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
      (layer) =>
        layer.id === 'a25d-mouth-maniac' &&
        layer.slot === 'mouth' &&
        layer.variant === 'maniac',
    ),
  )
  for (const variant of ['wide', 'round', 'narrow'] as const) {
    assert.ok(
      prepared.source.layers.some(
        (layer) =>
          layer.id === `a25d-mouth-${variant}` &&
          layer.slot === 'mouth' &&
          layer.variant === variant,
      ),
    )
  }
  assert.ok(
    prepared.source.layers.some(
      (layer) =>
        layer.id === 'a25d-mouth-close' &&
        layer.slot === 'mouth' &&
        layer.variant === 'closed',
    ),
  )
  assert.ok(
    prepared.source.layers.some(
      (layer) =>
        layer.id === 'a25d-mouth-open' &&
        layer.slot === 'mouth' &&
        layer.variant === 'open',
    ),
  )
  assert.ok(
    prepared.source.layers.some(
      (layer) =>
        layer.id === 'a25d-mouth-cry' &&
        layer.slot === 'mouth' &&
        layer.variant === 'cry',
    ),
  )
  assert.equal(
    prepared.source.layers.filter((layer) => layer.variant === 'dizzy').length,
    2,
  )
  assert.equal(
    prepared.source.layers.filter((layer) => layer.variant === 'squeeze')
      .length,
    2,
  )
  assert.equal(
    prepared.source.layers.filter(
      (layer) => layer.variant === 'cry' && layer.slot !== 'mouth',
    ).length,
    2,
  )
  assert.equal(
    prepared.source.layers.filter(
      (layer) => layer.variant === 'cry' && layer.slot === 'mouth',
    ).length,
    1,
  )
})

test('authored eye_dizzy artwork takes precedence over generated symbols', async () => {
  const source = syntheticSeeThroughPsd()
  const withAuthoredDizzyEyes = {
    ...source,
    children: [
      ...(source.children || []),
      unknownBlob('eye_dizzy_l', 80, 70, 114, 76),
      unknownBlob('eye_dizzy_r', 142, 70, 176, 76),
    ],
  } as Psd
  const prepared = await prepareWithFakeCanvas(withAuthoredDizzyEyes)
  const dizzyEyes = prepared.source.layers.filter(
    (layer) => layer.variant === 'dizzy',
  )
  assert.equal(dizzyEyes.length, 2)
  for (const eye of dizzyEyes) {
    const xs = eye.mesh.vertices.map((vertex) => vertex.x)
    const ys = eye.mesh.vertices.map((vertex) => vertex.y)
    const width = Math.max(...xs) - Math.min(...xs)
    const height = Math.max(...ys) - Math.min(...ys)
    assert.ok(width / height > 3, 'authored wide mark must not be replaced')
  }
})

test('authored eye_squeeze artwork takes precedence over generated chevrons', async () => {
  const source = syntheticSeeThroughPsd()
  const withAuthoredSqueezeEyes = {
    ...source,
    children: [
      ...(source.children || []),
      unknownBlob('eye_squeeze_l', 80, 68, 114, 82),
      unknownBlob('eye_squeeze_r', 142, 68, 176, 82),
    ],
  } as Psd
  const prepared = await prepareWithFakeCanvas(withAuthoredSqueezeEyes)
  const squeezeEyes = prepared.source.layers.filter(
    (layer) => layer.variant === 'squeeze',
  )
  assert.equal(squeezeEyes.length, 2)
  for (const eye of squeezeEyes) {
    const xs = eye.mesh.vertices.map((vertex) => vertex.x)
    const ys = eye.mesh.vertices.map((vertex) => vertex.y)
    const width = Math.max(...xs) - Math.min(...xs)
    const height = Math.max(...ys) - Math.min(...ys)
    assert.ok(width / height > 2, 'authored wide mark must not be replaced')
  }
})

test('authored eye_cry artwork takes precedence over generated crying eyes', async () => {
  const source = syntheticSeeThroughPsd()
  const withAuthoredCryEyes = {
    ...source,
    children: [
      ...(source.children || []),
      unknownBlob('eye_cry_l', 78, 66, 116, 118),
      unknownBlob('eye_cry_r', 140, 66, 178, 121),
    ],
  } as Psd
  const prepared = await prepareWithFakeCanvas(withAuthoredCryEyes)
  const cryingEyes = prepared.source.layers.filter(
    (layer) => layer.variant === 'cry' && layer.slot !== 'mouth',
  )
  assert.equal(cryingEyes.length, 2)
  for (const eye of cryingEyes) {
    const xs = eye.mesh.vertices.map((vertex) => vertex.x)
    const ys = eye.mesh.vertices.map((vertex) => vertex.y)
    const width = Math.max(...xs) - Math.min(...xs)
    const height = Math.max(...ys) - Math.min(...ys)
    assert.ok(height > width, 'authored tall tear artwork must not be replaced')
  }
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
