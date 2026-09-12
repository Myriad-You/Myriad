import type { Anime25DPlaybackLayer } from './types'
import type {
  UpstreamRuntimeExpression,
  UpstreamRuntimeFrame,
  UpstreamRuntimeLayer,
} from './upstream/types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { IDENTITY_DRIVER } from './driver'
import {
  bindAnime25DUpstreamFeature,
  deformAnime25DUpstreamFeaturePoint,
  resolveAnime25DUpstreamFeature,
} from './layerDeformation'
import { writeAnime25DLayerGlobalTransform } from './layerTransform'
import { deformUpstreamRuntimeLayer } from './upstream/runtime'

const FRAME: UpstreamRuntimeFrame = {
  anchors: {
    face: { cx: 128, cy: 126, x0: 54, x1: 202, y0: 28, y1: 244 },
    eyeL: {
      x0: 73,
      x1: 111,
      y0: 88,
      y1: 115,
      icx: 92,
      icy: 102,
      closeY: 103,
    },
    eyeR: {
      x0: 145,
      x1: 184,
      y0: 86,
      y1: 114,
      icx: 164,
      icy: 101,
      closeY: 102,
    },
    mouth: { x0: 101, x1: 157, y0: 157, y1: 184, cx: 129, cy: 170 },
    neckPivot: { cx: 128, cy: 220 },
    neckTop: 195,
    neckBottom: 263,
    bodyPivot: { cx: 128, cy: 330 },
    faceScale: 0.83,
    hairRootY: 37,
  },
  faceScale: 0.83,
  neckPivot: { cx: 128, cy: 220 },
  bodyPivot: { cx: 128, cy: 330 },
  faceCenter: { x: 128, y: 126 },
  chest: { cx: 128, cy: 252, rx: 78, ry: 62 },
  physicsEnabled: false,
  bustDisplacement: 0,
}

test('shared eye and eyebrow local deformation is byte-identical to upstream', () => {
  const expression = runtimeExpression({
    angleX: 0,
    angleY: 0,
    angleZ: 0,
    body: 0,
    breath: 0,
    breathHead: 0,
  })
  for (const fixture of featureLayers()) {
    const expected = runtimeLayer(fixture)
    deformUpstreamRuntimeLayer(expected, expression, FRAME)
    const actual = localFeatureVertices(fixture, expression)
    assert.deepEqual(actual, expected.cur, fixture.role)
  }
})

test('shared local stage composes with shader transforms like upstream deform', () => {
  for (let frame = 0; frame < 90; frame += 1) {
    const time = frame / 60
    const expression = runtimeExpression({
      angleX: Math.sin(time * 1.7) * 0.8,
      angleY: Math.cos(time * 1.3) * 0.7,
      angleZ: Math.sin(time * 0.9) * 0.6,
      body: Math.cos(time * 0.8) * 0.5,
      breath: 0.5 + 0.5 * Math.sin(time * 1.4),
      breathHead: 0.5 + 0.5 * Math.sin(time * 1.4 - 0.6),
    })
    for (const fixture of featureLayers()) {
      const expected = runtimeLayer(fixture)
      deformUpstreamRuntimeLayer(expected, expression, FRAME)
      const actual = composePlayerTransforms(fixture, expression)
      for (let index = 0; index < actual.length; index += 1) {
        assert.ok(
          Math.abs(actual[index] - expected.cur[index]) < 0.0001,
          `${fixture.role} frame ${frame} coordinate ${index}`,
        )
      }
    }
  }
})

test('resolver excludes Myriad replacement expressions from the upstream stage', () => {
  assert.equal(
    resolveAnime25DUpstreamFeature(
      { role: 'irides', fade: 'eyeOpen' },
      true,
    ),
    'eye-open-iris',
  )
  assert.equal(
    resolveAnime25DUpstreamFeature(
      { role: 'eye-close', fade: 'eyeClose' },
      true,
    ),
    'eye-close',
  )
  assert.equal(
    resolveAnime25DUpstreamFeature(
      { role: 'eye-cry', fade: 'eyeCry' },
      true,
    ),
    null,
  )
  assert.equal(
    resolveAnime25DUpstreamFeature(
      { role: 'mouth-open', fade: 'mouthOpen' },
      false,
    ),
    null,
  )
})

test('binding holds the live driver reference without per-frame reconstruction', () => {
  const source = featureLayers()[0]
  const expression = runtimeExpression({})
  const binding = bindAnime25DUpstreamFeature(
    source,
    FRAME.anchors.eyeL,
    FRAME.faceScale,
    expression,
  )!
  assert.equal(binding.kind, 'eye-open-iris')
  assert.equal(binding.expression, expression)
  expression.eyeX = 0.73
  assert.equal(binding.expression.eyeX, 0.73)
})

function featureLayers(): Anime25DPlaybackLayer[] {
  return [
    layer('irides', 'eyeOpen', 'L', 80, 91, 24, 20, 1.2),
    layer('eyelash', 'eyeOpen', 'R', 142, 84, 45, 34, 1.3),
    layer('eye-close', 'eyeClose', 'L', 72, 98, 42, 15, 1.31),
    layer('eyebrow', null, 'R', 143, 68, 44, 13, 1.34),
  ]
}

function layer(
  role: string,
  fade: Anime25DPlaybackLayer['fade'],
  side: Anime25DPlaybackLayer['side'],
  x: number,
  y: number,
  w: number,
  h: number,
  depth: number,
): Anime25DPlaybackLayer {
  return {
    name: `${role}-${side}`,
    role,
    z: 1,
    depth,
    group: 'head',
    phys: null,
    fade,
    side,
    x,
    y,
    w,
    h,
    atlas: { x: 0, y: 0, w: 1, h: 1 },
    strands: [],
  }
}

function runtimeLayer(source: Anime25DPlaybackLayer): UpstreamRuntimeLayer {
  const base = vertices(source)
  return {
    name: source.name,
    bn: source.role.replaceAll('-', '_'),
    group: source.group,
    side: source.side,
    fade:
      source.fade === 'eyeOpen' || source.fade === 'eyeClose'
        ? source.fade
        : null,
    x: source.x,
    y: source.y,
    w: source.w,
    h: source.h,
    depth: source.depth,
    base,
    cur: new Float32Array(base),
    strands: null,
  }
}

function localFeatureVertices(
  source: Anime25DPlaybackLayer,
  expression: UpstreamRuntimeExpression,
): Float32Array {
  const output = vertices(source)
  const eye = source.side === 'L' ? FRAME.anchors.eyeL : FRAME.anchors.eyeR
  const binding = bindAnime25DUpstreamFeature(
    source,
    eye,
    FRAME.faceScale,
    expression,
  )!
  const point = { x: 0, y: 0 }
  for (let index = 0; index < output.length; index += 2) {
    point.x = output[index]
    point.y = output[index + 1]
    deformAnime25DUpstreamFeaturePoint(point, binding)
    output[index] = point.x
    output[index + 1] = point.y
  }
  return output
}

function composePlayerTransforms(
  source: Anime25DPlaybackLayer,
  expression: UpstreamRuntimeExpression,
): Float32Array {
  const output = localFeatureVertices(source, expression)
  const angleZ = expression.angleZ * 0.07
  const matrix = writeAnime25DLayerGlobalTransform(
    {
      headFollow: 1,
      headRotationCosine: Math.cos(angleZ),
      headRotationSine: Math.sin(angleZ),
      neckPivotX: FRAME.neckPivot.cx,
      neckPivotY: FRAME.neckPivot.cy,
      faceScale: FRAME.faceScale,
      angleX: expression.angleX,
      angleY: expression.angleY,
      depthOffset: source.depth - 1,
      faceCenterY: FRAME.faceCenter.y,
      specialOffsetY: 0,
      breathOffset: expression.breathHead * 1.6,
    },
    new Float32Array(9),
  )
  const bodyAngle = expression.body * 0.028
  const bodyCosine = Math.cos(bodyAngle)
  const bodySine = Math.sin(bodyAngle)
  for (let index = 0; index < output.length; index += 2) {
    const layerX =
      matrix[0] * output[index] +
      matrix[3] * output[index + 1] +
      matrix[6]
    const layerY =
      matrix[1] * output[index] +
      matrix[4] * output[index + 1] +
      matrix[7]
    const relativeX = layerX - FRAME.bodyPivot.cx
    const relativeY = layerY - FRAME.bodyPivot.cy
    output[index] =
      FRAME.bodyPivot.cx + relativeX * bodyCosine - relativeY * bodySine
    output[index + 1] =
      FRAME.bodyPivot.cy + relativeX * bodySine + relativeY * bodyCosine
  }
  return output
}

function vertices(source: Anime25DPlaybackLayer): Float32Array {
  return new Float32Array([
    source.x,
    source.y,
    source.x + source.w,
    source.y,
    source.x,
    source.y + source.h,
    source.x + source.w,
    source.y + source.h,
  ])
}

function runtimeExpression(
  overrides: Partial<UpstreamRuntimeExpression>,
): UpstreamRuntimeExpression {
  return {
    ...IDENTITY_DRIVER,
    eyeOpenL: 0.37,
    eyeOpenR: 0.71,
    eyeX: -0.43,
    eyeY: 0.58,
    brow: -0.32,
    browAngL: 0.44,
    browAngR: -0.29,
    browAngSym: 0.18,
    eyeCY: 0.36,
    eyeCAng: -0.52,
    eyeScaleL: 1.24,
    eyeScaleR: 0.83,
    irisScale: 0.89,
    breath: 0,
    breathHead: 0,
    ...overrides,
  }
}
