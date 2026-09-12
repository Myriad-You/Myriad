import type {
  Anime25DPlaybackAnchors,
  Anime25DPlaybackLayer,
} from './types'
import type { UpstreamRig } from './upstream/types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { localToAtlasUv } from './atlasUv'
import { hairStrandDynamics } from './hairPhysics'
import { buildAnime25DLayerBinding } from './layerBinding'
import { bindUpstreamRuntimeRig } from './upstream/runtime'

const CANVAS_WIDTH = 768
const ANCHORS: Anime25DPlaybackAnchors = {
  face: { cx: 384, cy: 260, x0: 230, x1: 538, y0: 96, y1: 405 },
  eyeL: {
    x0: 272,
    x1: 335,
    y0: 181,
    y1: 220,
    icx: 304,
    icy: 201,
    closeY: 207,
  },
  eyeR: {
    x0: 433,
    x1: 496,
    y0: 181,
    y1: 220,
    icx: 464,
    icy: 201,
    closeY: 207,
  },
  mouth: { cx: 384, cy: 325, x0: 345, x1: 423, y0: 309, y1: 342 },
  neckPivot: { x: 384, y: 424 },
  neckTop: 395,
  neckBottom: 482,
  bodyPivot: { x: 384, y: 1_024 },
  faceScale: 308 / 333,
}

test('unextended player mesh and atlas UVs preserve upstream binding values', () => {
  const source = playbackLayer({
    name: 'face',
    role: 'face',
    x: 211.25,
    y: 82.75,
    w: 341.5,
    h: 412.25,
  })
  const upstream = upstreamBinding(source)
  const enhanced = buildAnime25DLayerBinding({
    source,
    canvasWidth: CANVAS_WIDTH,
    face: ANCHORS.face,
    layerZ: source.z!,
  })

  assert.deepEqual(enhanced.extensions, [])
  assert.deepEqual(enhanced.rest, upstream.base)
  assert.deepEqual(enhanced.indices, upstream.indices)
  assert.deepEqual(enhanced.atlasUvs, atlasUvs(upstream.uv, source))
})

test('eye curves have feature-scaled sampling while neutral UV coverage stays intact', () => {
  for (const role of ['eyewhite', 'eyelash', 'irides', 'eye-close', 'eye-close2', 'eyebrow']) {
    for (const scale of [0.5, 1, 2]) {
      const source = playbackLayer({ name: role, role, x: 270 * scale, y: 180 * scale, w: 80 * scale, h: 24 * scale })
      const binding = buildAnime25DLayerBinding({ source, canvasWidth: CANVAS_WIDTH * scale,
        face: { ...ANCHORS.face, x0: ANCHORS.face.x0 * scale, x1: ANCHORS.face.x1 * scale }, layerZ: 4 })
      assert.ok(binding.extensions.includes('eye-mesh-density'))
      assert.equal(binding.cols, 8)
      assert.equal(binding.rows, 4)
      assert.equal(binding.rest[0], source.x)
      assert.equal(binding.rest[1], source.y)
      assert.equal(binding.rest.at(-2), source.x + source.w)
      assert.equal(binding.rest.at(-1), source.y + source.h)
      for (let i = 0; i < binding.rest.length; i += 2) {
        const [u, v] = localToAtlasUv(source.atlas, (binding.rest[i] - source.x) / source.w, (binding.rest[i + 1] - source.y) / source.h)
        assert.ok(Math.abs(binding.atlasUvs[i] - u) < 1e-7)
        assert.ok(Math.abs(binding.atlasUvs[i + 1] - v) < 1e-7)
      }
    }
  }
})

test('face mesh resolves nonuniform depth landmarks without changing atlas coverage or other layers', () => {
  const source = playbackLayer({ name: 'face', role: 'face', x: 200, y: 80, w: 360, h: 400 })
  const faceShell = {
    head: { centerX: 384, centerY: 240, radiusX: 190, radiusY: 230, radiusZ: 140 },
    faceProfile: { enabled: true, startY: 80, endY: 460, points: [{ v: 0.06, z: 0.1 }, { v: 0.31, z: 0.02 }, { v: 0.65, z: 0.3 }, { v: 0.78, z: 0.06 }, { v: 0.97, z: 0.14 }] },
  }
  const input = { source, faceShell, canvasWidth: CANVAS_WIDTH, face: ANCHORS.face, layerZ: 1 }
  const mesh = buildAnime25DLayerBinding(input)
  assert.deepEqual(mesh.extensions, ['face-profile-grid'])
  assert.ok(mesh.rest.includes(faceShell.head.centerX))
  for (const point of faceShell.faceProfile.points) {
    assert.ok(mesh.rest.includes(Math.fround(80 + point.v * 380)))
  }
  assert.equal(mesh.rest[0], source.x)
  assert.equal(mesh.rest[1], source.y)
  assert.equal(mesh.rest.at(-2), source.x + source.w)
  assert.equal(mesh.rest.at(-1), source.y + source.h)
  for (let i = 0; i < mesh.rest.length; i += 2) {
    const [u, v] = localToAtlasUv(source.atlas, (mesh.rest[i] - source.x) / source.w, (mesh.rest[i + 1] - source.y) / source.h)
    assert.ok(Math.abs(mesh.atlasUvs[i] - u) < 1e-7)
    assert.ok(Math.abs(mesh.atlasUvs[i + 1] - v) < 1e-7)
  }
  assert.ok(mesh.rest.length / 2 < 1500, 'landmark refinement stays local and bounded for this fixture')
  for (const role of ['neck', 'collar-front', 'front-hair', 'topwear']) {
    const layer = { ...source, role }
    const withProfile = buildAnime25DLayerBinding({ ...input, source: layer })
    const withoutProfile = buildAnime25DLayerBinding({ ...input, source: layer, faceShell: undefined })
    assert.deepEqual(withProfile, withoutProfile, role)
  }
})

test('hair keeps upstream topology, progress, bang blocks, and spring phase', () => {
  const source = playbackLayer({
    name: 'front-hair-2',
    role: 'front-hair',
    phys: 'hair',
    x: 147.5,
    y: 38.25,
    w: 472.5,
    h: 522.75,
    z: 9,
    strands: [
      { x: 222.5, rootY: 65.25, tipY: 481.75 },
      { x: 365.75, rootY: 58.5, tipY: 506.25 },
      { x: 548.25, rootY: 72.75, tipY: 535.5 },
    ],
  })
  const upstream = upstreamBinding(source)
  const enhanced = buildAnime25DLayerBinding({
    source,
    canvasWidth: CANVAS_WIDTH,
    face: ANCHORS.face,
    layerZ: source.z!,
  })

  assert.deepEqual(enhanced.extensions, [
    'hair-length-dynamics',
    'front-hair-upper-parallax',
  ])
  assert.deepEqual(enhanced.rest, upstream.base)
  assert.deepEqual(enhanced.indices, upstream.indices)
  assert.deepEqual(enhanced.alongStrand, upstream.su)
  assert.deepEqual(enhanced.bangWeights, upstream.bw)
  assert.deepEqual(
    enhanced.springs?.map(({ phase }) => phase),
    upstream.spr?.map(({ phase }) => phase),
  )

  const expectedWeights = new Float32Array(upstream.sw!)
  const referenceHeight = ANCHORS.face.y1 - ANCHORS.face.y0
  for (let vertex = 0; vertex < upstream.base.length / 2; vertex += 1) {
    for (let strand = 0; strand < source.strands.length; strand += 1) {
      const dynamics = hairStrandDynamics(
        source.strands[strand].rootY,
        source.strands[strand].tipY,
        referenceHeight,
      )
      const index = vertex * source.strands.length + strand
      expectedWeights[index] *= dynamics.amplitudeScale
    }
  }
  assert.deepEqual(enhanced.strandWeights, expectedWeights)
})

test('intentional mesh replacements are explicit rather than parity failures', () => {
  const mouth = buildAnime25DLayerBinding({
    source: playbackLayer({
      name: 'mouth-open',
      role: 'mouth-open',
      fade: 'mouthOpen',
      x: 350,
      y: 312,
      w: 68,
      h: 28,
    }),
    canvasWidth: CANVAS_WIDTH,
    face: ANCHORS.face,
    layerZ: 4,
  })
  const collar = buildAnime25DLayerBinding({
    source: playbackLayer({
      name: 'collar-front',
      role: 'collar-front',
      group: 'body',
      x: 270,
      y: 405,
      w: 228,
      h: 160,
    }),
    canvasWidth: CANVAS_WIDTH,
    face: ANCHORS.face,
    layerZ: 5,
    extraGridX: [310, 384, 458],
    extraGridY: [431, 470],
  })

  assert.deepEqual(mouth.extensions, ['mouth-mesh-density'])
  assert.ok(mouth.cols >= 6)
  assert.ok(mouth.rows >= 4)
  assert.deepEqual(collar.extensions, [
    'collar-mesh-density',
    'collar-contact-grid',
  ])
  assert.ok(collar.rest.includes(384))
  assert.ok(collar.rest.includes(470))
})

function playbackLayer(
  overrides: Partial<Anime25DPlaybackLayer>,
): Anime25DPlaybackLayer {
  return {
    name: 'layer',
    role: 'unknown',
    z: 3,
    depth: 1,
    group: 'head',
    phys: null,
    fade: null,
    side: null,
    x: 100,
    y: 100,
    w: 120,
    h: 160,
    atlas: { x: 0.125, y: 0.25, w: 0.375, h: 0.5 },
    strands: [],
    ...overrides,
  }
}

function upstreamBinding(source: Anime25DPlaybackLayer) {
  const pixel = {
    width: 1,
    height: 1,
    data: new Uint8ClampedArray([255, 255, 255, 255]),
  }
  const rig: UpstreamRig = {
    canvas: { w: CANVAS_WIDTH, h: 1_024 },
    anchors: {
      face: ANCHORS.face,
      eyeL: ANCHORS.eyeL,
      eyeR: ANCHORS.eyeR,
      mouth: ANCHORS.mouth,
      neckPivot: { cx: ANCHORS.neckPivot.x, cy: ANCHORS.neckPivot.y },
      neckTop: ANCHORS.neckTop,
      neckBottom: ANCHORS.neckBottom,
      bodyPivot: { cx: ANCHORS.bodyPivot.x, cy: ANCHORS.bodyPivot.y },
      faceScale: ANCHORS.faceScale,
      hairRootY: Math.min(
        ANCHORS.face.y0,
        ...source.strands.map(({ rootY }) => rootY),
      ),
    },
    layers: [
      {
        name:
          source.role === 'front-hair'
            ? 'front hair'
            : source.role === 'back-hair'
              ? 'back hair'
              : source.role.replaceAll('-', '_'),
        x: source.x,
        y: source.y,
        w: source.w,
        h: source.h,
        z: source.z!,
        depth: source.depth,
        group: source.group,
        phys: source.phys,
        fade:
          source.fade === 'eyeOpen' ||
          source.fade === 'eyeClose' ||
          source.fade === 'mouthOpen' ||
          source.fade === 'mouthClose'
            ? source.fade
            : null,
        side: source.side,
        strands: source.strands,
        img: pixel,
      },
    ],
    warnings: [],
    synth: { eye: false, mouth: false },
  }
  return bindUpstreamRuntimeRig(rig).layers[0]
}

function atlasUvs(
  localUvs: Float32Array,
  source: Anime25DPlaybackLayer,
): Float32Array {
  const output = new Float32Array(localUvs.length)
  for (let index = 0; index < localUvs.length; index += 2) {
    const [u, v] = localToAtlasUv(
      source.atlas,
      localUvs[index],
      localUvs[index + 1],
    )
    output[index] = u
    output[index + 1] = v
  }
  return output
}
