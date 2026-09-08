import type {
  UpstreamGenericParts,
  UpstreamPsd,
  UpstreamPsdLayer,
  UpstreamRgbaImage,
} from './types'
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import test from 'node:test'
import { genericParts as upstreamGenericParts } from './genericParts'
import {
  ANIME25D_GENERIC_PART_SHA256,
  ANIME25D_IMPORT_FIXES_REVISION,
  ANIME25D_UPSTREAM_REVISION,
} from './revision'
import { rigger as port } from './rigger'

function image(
  width: number,
  height: number,
  paint: (x: number, y: number) => readonly [number, number, number, number],
): UpstreamRgbaImage {
  const data = new Uint8ClampedArray(width * height * 4)
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      data.set(paint(x, y), (y * width + x) * 4)
    }
  }
  return { width, height, data }
}

function solidLayer(
  name: string,
  left: number,
  top: number,
  width: number,
  height: number,
  rgba: readonly [number, number, number, number] = [50, 40, 60, 255],
): UpstreamPsdLayer {
  return {
    name,
    left,
    top,
    right: left + width,
    bottom: top + height,
    imageData: image(width, height, () => rgba),
  }
}

function pairedLayer(
  name: string,
  canvasWidth: number,
  canvasHeight: number,
  rects: ReadonlyArray<{
    x: number
    y: number
    width: number
    height: number
    rgba?: readonly [number, number, number, number]
  }>,
): UpstreamPsdLayer {
  return {
    name,
    left: 0,
    top: 0,
    right: canvasWidth,
    bottom: canvasHeight,
    imageData: image(canvasWidth, canvasHeight, (x, y) => {
      const rect = rects.find(
        (candidate) =>
          x >= candidate.x &&
          x < candidate.x + candidate.width &&
          y >= candidate.y &&
          y < candidate.y + candidate.height,
      )
      return rect ? rect.rgba || [50, 40, 60, 255] : [0, 0, 0, 0]
    }),
  }
}

function representativePsd(): UpstreamPsd {
  const width = 180
  const height = 240
  const paired = (
    name: string,
    y: number,
    partWidth: number,
    partHeight: number,
    rgba?: readonly [number, number, number, number],
  ) =>
    pairedLayer(name, width, height, [
      { x: 42, y, width: partWidth, height: partHeight, rgba },
      { x: 112, y, width: partWidth, height: partHeight, rgba },
    ])

  return {
    width,
    height,
    children: [
      solidLayer('back hair', 20, 5, 140, 205, [80, 70, 100, 255]),
      solidLayer('topwear', 24, 165, 132, 75, [100, 80, 120, 255]),
      solidLayer('neck', 72, 135, 36, 80, [240, 190, 180, 255]),
      solidLayer('face', 35, 18, 110, 132, [245, 200, 190, 255]),
      paired('eyewhite', 69, 26, 12, [245, 245, 250, 255]),
      paired('irides', 71, 10, 10, [70, 90, 150, 255]),
      paired('eyelash', 66, 28, 5, [30, 20, 35, 255]),
      solidLayer('mouth', 75, 123, 30, 13, [120, 45, 65, 255]),
      solidLayer('front hair_1', 27, 3, 126, 112, [90, 80, 120, 255]),
      solidLayer('unknown ornament', 82, 30, 16, 16, [180, 140, 60, 255]),
      solidLayer('unknown sash', 70, 190, 40, 20, [70, 50, 90, 255]),
    ],
  }
}

function genericParts(): UpstreamGenericParts {
  return {
    eyeL: image(8, 4, () => [20, 15, 25, 255]),
    eyeR: image(8, 4, () => [20, 15, 25, 255]),
    mouth: image(10, 3, () => [90, 30, 45, 255]),
  }
}

function clonePsd(psd: UpstreamPsd): UpstreamPsd {
  return {
    width: psd.width,
    height: psd.height,
    children: psd.children?.map((layer) => ({
      ...layer,
      imageData: layer.imageData
        ? {
            width: layer.imageData.width,
            height: layer.imageData.height,
            data: new Uint8ClampedArray(layer.imageData.data),
          }
        : undefined,
    })),
  }
}

test('records the upstream revision implemented by the standalone module', () => {
  assert.equal(
    ANIME25D_UPSTREAM_REVISION,
    'd48825867acd081de22b0e7b5585bb562288796d',
  )
  assert.equal(
    ANIME25D_IMPORT_FIXES_REVISION,
    '8deb51b7f93984191dfd5805becb349bbe58f90f',
  )
})

test('preserves every decoded byte of the upstream generic parts', () => {
  const expectedDimensions = {
    eyeL: { width: 88, height: 42 },
    eyeR: { width: 94, height: 32 },
    mouth: { width: 72, height: 18 },
  }
  for (const key of ['eyeL', 'eyeR', 'mouth'] as const) {
    const part = upstreamGenericParts.get(key)
    assert.ok(part)
    assert.deepEqual(
      { width: part.width, height: part.height },
      expectedDimensions[key],
    )
    assert.equal(
      createHash('sha256').update(part.data).digest('hex'),
      ANIME25D_GENERIC_PART_SHA256[key],
    )
    assert.equal(upstreamGenericParts.get(key)?.data, part.data)
  }
  assert.equal(upstreamGenericParts.get('missing'), null)
})

test('freezes upstream name normalization and numbered-layer semantics', () => {
  assert.equal(port.normName(' ＭＯＵＴＨ のコピー 2 '), 'mouth_open')
  assert.equal(port.normName('eyelash_c'), 'eye_close')
  assert.equal(port.normName('mouth-c'), 'mouth-c')
  assert.equal(port.normName('レイヤー 1'), 'facedetail')
  assert.equal(port.baseName('front hair_12'), 'front hair')
  assert.equal(port.baseName('front hair-12'), 'front hair-12')
})

test('freezes connected-component thresholds and the all-dust exception', () => {
  const width = 20
  const alpha = new Uint8Array(width * 12)
  for (let y = 0; y < 5; y += 1) {
    for (let x = 0; x < 8; x += 1) alpha[y * width + x] = 17
  }
  for (let y = 7; y < 12; y += 1) {
    for (let x = 12; x < 19; x += 1) alpha[y * width + x] = 255
  }
  const cleaned = port._internals.cleanAlpha(alpha, width, 12, 40)
  for (let y = 0; y < 5; y += 1) {
    for (let x = 0; x < 8; x += 1) assert.equal(cleaned[y * width + x], 17)
  }
  for (let y = 7; y < 12; y += 1) {
    for (let x = 12; x < 19; x += 1) assert.equal(cleaned[y * width + x], 0)
  }

  const onlyDust = new Uint8Array(10)
  onlyDust.fill(255, 0, 9)
  const returned = port._internals.cleanAlpha(onlyDust, 10, 1, 40)
  assert.equal(returned, onlyDust)
  assert.ok(returned.every((value, index) => value === (index < 9 ? 255 : 0)))
})

test('freezes in-place PSD cleanup, trim padding, and mutation shape', () => {
  const source: UpstreamPsd = {
    width: 60,
    height: 40,
    children: [
      {
        ...pairedLayer('face', 60, 40, [
          { x: 20, y: 10, width: 8, height: 8 },
          { x: 1, y: 1, width: 1, height: 1 },
        ]),
        canvas: { stale: true },
      },
      { name: 'group without pixels' },
    ],
  }
  const stats = port.cleanPsdLayers(source)
  assert.deepEqual(stats, { noisy: 1, layers: 1 })
  assert.deepEqual(
    {
      left: source.children?.[0]?.left,
      top: source.children?.[0]?.top,
      right: source.children?.[0]?.right,
      bottom: source.children?.[0]?.bottom,
      width: source.children?.[0]?.imageData?.width,
      height: source.children?.[0]?.imageData?.height,
      canvas: source.children?.[0]?.canvas,
    },
    {
      left: 16,
      top: 6,
      right: 32,
      bottom: 22,
      width: 16,
      height: 16,
      canvas: undefined,
    },
  )
})

test('freezes complete rig order, anchors, warnings, strands, and synthesis', () => {
  const rig = port.buildRig(representativePsd(), { generic: genericParts() })
  assert.deepEqual(rig.canvas, { w: 180, h: 240 })
  assert.deepEqual(
    rig.layers.map((layer) => layer.name),
    [
      'back hair',
      'topwear',
      'neck',
      'face',
      'eyewhite_l',
      'eyewhite_r',
      'irides_l',
      'irides_r',
      'eyelash_l',
      'eyelash_r',
      'eye_close_l',
      'eye_close_r',
      'mouth_open',
      'mouth_close',
      'front hair_1',
      'unknown ornament',
      'unknown sash',
    ],
  )
  assert.ok(rig.layers.every((layer, index) => layer.z === index))
  assert.deepEqual(rig.synth, { eye: true, mouth: true })
  assert.equal(
    rig.layers.find((layer) => layer.name === 'front hair_1')?.strands?.length,
    2,
  )
  assert.equal(
    rig.layers.find((layer) => layer.name === 'unknown ornament')?.group,
    'head',
  )
  assert.equal(
    rig.layers.find((layer) => layer.name === 'unknown sash')?.group,
    'body',
  )
  assert.equal(rig.anchors.bodyPivot.cy, 240)
  assert.equal(rig.anchors.hairRootY, rig.anchors.face.y0 + 60)
  assert.deepEqual(rig.warnings, [
    '未知のレイヤー名 "unknown ornament" — head として扱います',
    '未知のレイヤー名 "unknown sash" — body として扱います',
    '不足する閉じ目を自動配置しました（「目」の差分バーで調整可）',
    'mouth_close が無いため汎用閉じ口を自動配置しました（「口」のバーで調整可）',
  ])
})

test('freezes missing-face fallbacks and exact Japanese diagnostics', () => {
  assert.throws(
    () => port.buildRig({ width: 20, height: 30, children: [] }),
    new Error(
      'レイヤーが見つかりません（グループは未対応・フラット構成にしてください）',
    ),
  )
  const rig = port.buildRig({
    width: 100,
    height: 200,
    children: [solidLayer('mystery', 10, 120, 20, 20)],
  })
  assert.deepEqual(rig.anchors.face, {
    cx: 50,
    cy: 60,
    x0: 35,
    x1: 65,
    y0: 20,
    y1: 100,
  })
  assert.deepEqual(rig.warnings, [
    'face レイヤーがありません — キャンバス中央を顔とみなします',
    '未知のレイヤー名 "mystery" — body として扱います',
    '目のアンカーが不完全です（eyewhite/irides を確認）',
    'mouth_open / mouth_close がありません',
  ])
})

test('freezes flat-image composition and widest-gap eye splitting', () => {
  const flat = port.flattenPsdToImg({
    width: 8,
    height: 3,
    children: [
      solidLayer('back', 1, 1, 6, 1, [200, 0, 0, 128]),
      solidLayer('front', 2, 1, 2, 1, [0, 100, 0, 128]),
    ],
  })
  assert.ok(flat)
  assert.deepEqual(
    { width: flat.width, height: flat.height },
    { width: 6, height: 1 },
  )
  assert.deepEqual(Array.from(flat.data.subarray(4, 8)), [66, 67, 0, 192])

  const eyes = pairedLayer('eyes', 18, 5, [
    { x: 1, y: 1, width: 4, height: 3 },
    { x: 13, y: 1, width: 4, height: 3 },
  ]).imageData
  assert.ok(eyes)
  const split = port.splitImgLR(eyes)
  assert.ok(split)
  assert.deepEqual(
    {
      lw: split.l.width,
      lh: split.l.height,
      rw: split.r.width,
      rh: split.r.height,
    },
    { lw: 4, lh: 3, rw: 4, rh: 3 },
  )
})

test('numbered semantic fragments share anchors without merging their artwork', () => {
  const source = representativePsd()
  const original = port.buildRig(clonePsd(source))
  for (const child of source.children!) {
    if (
      ['face', 'neck', 'eyewhite', 'irides', 'eyelash', 'mouth'].includes(
        child.name!,
      )
    ) {
      child.name = child.name === 'mouth' ? 'mouth_open_1' : `${child.name}_1`
    }
  }
  const numbered = port.buildRig(source)
  assert.deepEqual(numbered.anchors, original.anchors)
  assert.ok(numbered.layers.some((part) => part.name === 'eyewhite_1_l'))
  // A second same-side fragment contributes to the union, not last-write-wins.
  source.children!.push(solidLayer('eyewhite_2', 35, 69, 8, 12))
  const extended = port.buildRig(source)
  assert.equal(extended.anchors.eyeL!.x0, 35)
  assert.equal(extended.anchors.eyeL!.x1, original.anchors.eyeL!.x1)
  assert.deepEqual(extended.anchors.eyeR, original.anchors.eyeR)
  assert.ok(extended.layers.some((part) => part.name === 'eyewhite_2_l'))
})

test('authored close artwork on one side never suppresses synthesis on the other', () => {
  for (const [x, side, missing] of [
    [42, 'L', 'R'],
    [112, 'R', 'L'],
  ] as const) {
    const source = representativePsd()
    source.children!.push(solidLayer('eye_close_1', x, 76, 28, 5))
    const rig = port.buildRig(source, { generic: genericParts() })
    const closed = rig.layers.filter((layer) => layer.fade === 'eyeClose')
    assert.equal(closed.length, 2)
    assert.equal(closed.find((layer) => !layer.synthetic)!.side, side)
    assert.equal(closed.find((layer) => layer.synthetic)!.side, missing)
  }
})

test('narrow hair has unique, separated strands instead of forced duplicate springs', () => {
  for (const width of [1, 2, 3, 8, 16, 64, 128]) {
    const strands = port._internals.detectStrands(
      new Uint8Array(width * 100).fill(255),
      width,
      100,
      30,
      6,
    )
    assert.ok(strands.length > 0 && strands.length <= 6)
    for (let index = 1; index < strands.length; index += 1) {
      assert.ok(strands[index].x - strands[index - 1].x >= 30)
    }
  }
})

test('synthetic close edges ignore RGB stored in fully transparent pixels', () => {
  const source = representativePsd()
  source.children = source.children!.filter(
    (layer) => !['eyelash', 'eyebrow'].includes(layer.name!),
  )
  const edge = image(2, 1, (x) => (x === 0 ? [255, 0, 0, 128] : [0, 0, 255, 0]))
  const rig = port.buildRig(source, { generic: { eyeL: edge } })
  const eye = rig.layers.find((layer) => layer.synthetic && layer.side === 'L')!
  assert.ok(eye)
  assert.deepEqual([...eye.img.data.subarray(0, 4)], [255, 0, 0, 128])
  for (let offset = 0; offset < eye.img.data.length; offset += 4) {
    if (eye.img.data[offset + 3] === 0) continue
    assert.equal(eye.img.data[offset], 255)
    assert.equal(eye.img.data[offset + 2], 0)
  }
})

test('empty semantic layers and prototype-like names cannot create invalid anchors or slots', () => {
  const source = representativePsd()
  source
    .children!.find((layer) => layer.name === 'face')!
    .imageData!.data.fill(0)
  source.children!.push(solidLayer('face_2', 35, 18, 110, 132))
  for (const name of ['__proto__', 'constructor', 'toString']) {
    source.children!.push(solidLayer(name, 60, 30, 10, 10))
  }
  const rig = port.buildRig(source)
  assert.equal(rig.anchors.face.cx, 89.5)
  assert.ok(rig.layers.every((layer) => Number.isFinite(layer.depth)))
  assert.ok(rig.warnings.some((warning) => warning.includes('空のレイヤー')))
  assert.throws(
    () =>
      port.buildRig({
        width: 2,
        height: 2,
        children: [solidLayer('face', 0, 0, 2, 2, [0, 0, 0, 0])],
      }),
    /画素/,
  )
})

test('PSD fixtures can be cloned without sharing image buffers', () => {
  const source = representativePsd()
  const cloned = clonePsd(source)
  assert.deepEqual(cloned, source)
  assert.notEqual(
    cloned.children?.[0]?.imageData?.data,
    source.children?.[0]?.imageData?.data,
  )
})

test('representative rig stays deterministic with the September import fixes', () => {
  const source = representativePsd()
  const representative = port.buildRig(clonePsd(source), {
    generic: genericParts(),
  })
  assert.equal(
    createHash('sha256').update(JSON.stringify(representative)).digest('hex'),
    'c4be40ea0587d32487d8091d3d9bafd3dea9036fcdf44cdde8a9c71009dab8e2',
  )
  assert.deepEqual(
    representative,
    port.buildRig(clonePsd(source), { generic: genericParts() }),
  )
})

test('image primitives preserve component and strand invariants across seeded masks', () => {
  let state = 39657510
  const randomByte = () => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0
    return state >>> 24
  }

  for (let sample = 0; sample < 24; sample += 1) {
    const width = 11 + (sample % 9)
    const height = 9 + (sample % 7)
    const alpha = new Uint8Array(width * height)
    for (let index = 0; index < alpha.length; index += 1) {
      const value = randomByte()
      alpha[index] = value < 96 ? 0 : value
    }
    for (const threshold of [0, 8, 16, 127, 255]) {
      const components = port._internals.labelComponents(
        alpha,
        width,
        height,
        threshold,
      )
      assert.equal(components.lab.length, alpha.length)
      assert.equal(components.sizes.length, components.count + 1)
      assert.equal(components.sumX.length, components.count + 1)
      assert.equal(
        components.sizes.reduce((total, size) => total + size, 0),
        components.lab.filter((label) => label > 0).length,
      )
      assert.ok(
        components.lab.every(
          (label) => label >= 0 && label <= components.count,
        ),
      )
      for (let index = 1; index <= components.count; index += 1) {
        assert.ok(components.sizes[index] > 0)
        assert.ok(components.sumX[index] >= 0)
        assert.ok(
          components.sumX[index] <= components.sizes[index] * (width - 1),
        )
      }
    }
    const clean = new Uint8Array(alpha)
    assert.equal(port._internals.cleanAlpha(clean, width, height, 40), clean)
    const strands = port._internals.detectStrands(alpha, width, height, 3, 6)
    assert.ok(strands.length <= 6)
    assert.ok(
      strands.every(
        (strand) =>
          Number.isFinite(strand.x) &&
          strand.rootY >= 0 &&
          strand.rootY <= strand.tipY &&
          strand.tipY < height,
      ),
    )
  }

  const contour = new Float32Array(97)
  for (let index = 0; index < contour.length; index += 1) {
    contour[index] = randomByte() / 3
  }
  const peaks = port._internals.findPeaks(contour, 7, 10)
  assert.deepEqual(peaks, port._internals.findPeaks(contour, 7, 10))
  assert.ok(
    peaks.every(
      (peak) =>
        peak.x >= 0 && peak.x < contour.length && Number.isFinite(peak.prom),
    ),
  )
  for (let index = 1; index < peaks.length; index += 1) {
    assert.ok(peaks[index - 1].prom >= peaks[index].prom)
    assert.ok(
      peaks
        .slice(0, index)
        .every((existing) => Math.abs(existing.x - peaks[index].x) >= 7),
    )
  }
})

test('build edge cases match the corrected import fingerprint', () => {
  const explicitDiffs: UpstreamPsd = {
    width: 120,
    height: 180,
    children: [
      solidLayer('face', 20, 10, 80, 100),
      pairedLayer('eyewhite', 120, 180, [
        { x: 28, y: 45, width: 22, height: 10 },
        { x: 72, y: 45, width: 22, height: 10 },
      ]),
      pairedLayer('irides', 120, 180, [
        { x: 34, y: 46, width: 8, height: 8 },
        { x: 78, y: 46, width: 8, height: 8 },
      ]),
      pairedLayer('eye_close', 120, 180, [
        { x: 28, y: 50, width: 22, height: 4 },
        { x: 72, y: 50, width: 22, height: 4 },
      ]),
      solidLayer('mouth_open', 48, 103, 24, 10),
      solidLayer('mouth_close', 48, 107, 24, 4),
    ],
  }
  const negativeCoordinates: UpstreamPsd = {
    width: 80,
    height: 100,
    children: [
      solidLayer('face', -12.75, -7.25, 70, 80),
      solidLayer('front hair', -30, -20, 120, 95),
      solidLayer('off canvas', 200, 200, 10, 10),
    ],
  }
  const duplicateNames: UpstreamPsd = {
    width: 120,
    height: 180,
    children: [
      solidLayer('face', 10, 10, 70, 90),
      solidLayer('face', 30, 20, 70, 100),
      solidLayer('mouth_2', 47, 116, 26, 8),
      solidLayer('empty unknown', 10, 10, 2, 2, [0, 0, 0, 0]),
    ],
  }

  const digest = createHash('sha256')
  for (const psd of [explicitDiffs, negativeCoordinates, duplicateNames]) {
    const actual = port.buildRig(clonePsd(psd), { generic: genericParts() })
    digest.update(JSON.stringify(actual))
    assert.ok(actual.layers.length > 0)
    assert.ok(actual.layers.every((layer, index) => layer.z === index))
  }
  assert.equal(
    digest.digest('hex'),
    'd33170604d8d5b5fe3fd02407af0d3cce592c16e404e72affac68524e0186605',
  )

  const noGap = image(8, 3, () => [20, 30, 40, 255])
  assert.equal(port.splitImgLR(noGap), null)
  const names = [
    ['', '', ''],
    [' mouth-01 ', 'mouth_open', ' mouth-01 '],
    ['Ｍｏｕｔｈ＿３', 'mouth_open', 'Ｍｏｕｔｈ＿３'],
    ['face のコピー', 'face', 'face のコピー'],
    ['face のコピー 19', 'face', 'face のコピー 19'],
    ['face copy 2', 'face copy 2', 'face copy 2'],
  ] as const
  for (const [value, normalized, base] of names) {
    assert.equal(port.normName(value), normalized)
    assert.equal(port.baseName(value), base)
  }
})
