import type {
  UpstreamRig,
  UpstreamRuntimeBoundLayer,
  UpstreamRuntimeExpression,
  UpstreamRuntimeFrame,
  UpstreamRuntimeLayer,
  UpstreamRuntimeParameters,
  UpstreamRuntimeTickAutomation,
} from './types'
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import test from 'node:test'
import {
  bindUpstreamRuntimeRig,
  createUpstreamRuntimeTickState,
  deformUpstreamRuntimeLayer,
  planUpstreamRuntimeDraw,
  stepUpstreamRuntimeTick,
  UPSTREAM_RUNTIME_DEFAULTS,
  upstreamRuntimeFadeAlpha,
  upstreamRuntimeSmooth,
} from './runtime'

const MIGRATION_FINGERPRINTS = {
  binding320:
    '9531aed257420ac2eae05cb93c7985f58f1fd191ff94605f04c5d83ab04dada5',
  binding1152:
    '2e8a337368869550c30661ebc157642fe34005b62b1afccb64994d97ac957eca',
  deformation:
    'ffd36dc4690197e15bbd5208c9c1e095dd7232a518817f2d2746997e3d4f9237',
  tick: '4815f3c8d9b061d1f637ba2406bdd219ee5ea4484612116d2c9105f76ecf6a39',
  camera: '1ddfd017ea54da9e6e5cced95e99a736576e28e73aebaac2ffc1f85f84e5d55b',
} as const

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
  physicsEnabled: true,
  bustDisplacement: -1.7,
}

const BASE_EXPRESSION: UpstreamRuntimeExpression = {
  ...UPSTREAM_RUNTIME_DEFAULTS,
  breath: 0,
  breathHead: 0,
}

test('tick state owns independent parameter and expression objects', () => {
  const initial = { ...UPSTREAM_RUNTIME_DEFAULTS, angleX: 0.75 }
  const state = createUpstreamRuntimeTickState(125, initial)

  assert.deepEqual(state.current, initial)
  assert.notEqual(state.current, initial)
  assert.notEqual(state.expression, state.current)
  assert.equal(state.lastTimeMs, 125)
  assert.equal(state.nextBlinkAtMs, 1_925)

  state.current.angleX = -0.5
  assert.equal(initial.angleX, 0.75)
  assert.equal(UPSTREAM_RUNTIME_DEFAULTS.angleX, 0)
})

test('mesh binding preserves its fixed layouts and hair weight invariants', () => {
  for (const [canvasWidth, fingerprint] of [
    [320, MIGRATION_FINGERPRINTS.binding320],
    [1_152, MIGRATION_FINGERPRINTS.binding1152],
  ] as const) {
    const rig = bindingRig(canvasWidth)
    const binding = bindUpstreamRuntimeRig(rig)

    assert.equal(binding.anchors, rig.anchors)
    assert.deepEqual(binding.faceCenter, {
      x: rig.anchors.face.cx,
      y: rig.anchors.face.cy,
    })
    assert.deepEqual(binding.chest, {
      cx: rig.anchors.neckPivot.cx,
      cy:
        rig.anchors.neckBottom +
        (rig.anchors.face.y1 - rig.anchors.face.y0) * 0.6,
      rx: (rig.anchors.face.x1 - rig.anchors.face.x0) * 0.6,
      ry: (rig.anchors.face.y1 - rig.anchors.face.y0) * 0.45,
    })

    for (const bound of binding.layers) {
      assert.notEqual(bound.cur, bound.base)
      assert.deepEqual(bound.cur, bound.base)
      assert.equal(bound.base.length, bound.uv.length)
      assert.equal(bound.indices.length, bound.nIdx)
      assert.ok(bound.indices.every((index) => index < bound.base.length / 2))
    }

    for (const hair of binding.layers.filter(
      (layer) => layer.strands?.length,
    )) {
      const strandCount = hair.strands!.length
      const vertexCount = hair.base.length / 2
      assert.equal(hair.sw?.length, vertexCount * strandCount)
      assert.equal(hair.su?.length, vertexCount)
      assert.equal(hair.spr?.length, strandCount)
      assert.ok(hair.su?.every((progress) => progress >= 0 && progress <= 1))
      for (let vertex = 0; vertex < vertexCount; vertex += 1) {
        let total = 0
        for (let strand = 0; strand < strandCount; strand += 1) {
          total += hair.sw![vertex * strandCount + strand]
        }
        assert.ok(Math.abs(total - 1) < 1e-5)
      }
      hair.spr?.forEach((spring, index) => {
        assert.equal(spring.phase, index * 1.37 + hair.z)
      })
    }

    const frontHair = binding.layers.find((layer) => layer.bn === 'front hair')
    assert.ok(frontHair?.bw)
    for (let vertex = 0; vertex < frontHair.base.length / 2; vertex += 1) {
      const total =
        frontHair.bw[vertex * 3] +
        frontHair.bw[vertex * 3 + 1] +
        frontHair.bw[vertex * 3 + 2]
      assert.ok(Math.abs(total - 1) < 1e-5)
    }

    assert.equal(sha256(plainBindingSnapshot(binding)), fingerprint)
  }
})

test('draw planning keeps hidden eye whites as stencils and skips other hidden layers', () => {
  const expression: UpstreamRuntimeExpression = {
    ...BASE_EXPRESSION,
    eyeOpenL: 0,
    eyeOpenR: 1,
    mouthOpen: 0,
  }
  const commands = planUpstreamRuntimeDraw(drawLayers(), expression)

  assert.deepEqual(
    commands.map(({ name, stencil }) => [name, stencil]),
    [
      ['eyewhite_l', 'write'],
      ['face', 'none'],
      ['eyewhite_r', 'write'],
      ['irides_r', 'test'],
      ['mouth_close', 'none'],
    ],
  )
  assert.equal(commands[0].alpha, 0)
  assert.equal(commands[0].alphaCut, 0.25)
  assert.ok(!commands.some((command) => command.name === 'irides_l'))
  assert.ok(!commands.some((command) => command.name === 'mouth_open'))
})

test('fade functions are bounded, complementary, and side-specific', () => {
  assert.equal(upstreamRuntimeSmooth(-1), 0)
  assert.equal(upstreamRuntimeSmooth(0), 0)
  assert.equal(upstreamRuntimeSmooth(0.5), 0.5)
  assert.equal(upstreamRuntimeSmooth(1), 1)
  assert.equal(upstreamRuntimeSmooth(2), 1)

  for (let frame = 0; frame <= 120; frame += 1) {
    const expression = runtimeExpression(frame)
    assert.equal(
      upstreamRuntimeFadeAlpha({ fade: null, side: null }, expression),
      1,
    )
    for (const side of ['L', 'R'] as const) {
      const open = upstreamRuntimeFadeAlpha(
        { fade: 'eyeOpen', side },
        expression,
      )
      const close = upstreamRuntimeFadeAlpha(
        { fade: 'eyeClose', side },
        expression,
      )
      assert.ok(open >= 0 && open <= 1)
      assert.ok(close >= 0 && close <= 1)
      assert.ok(Math.abs(open + close - 1) < Number.EPSILON * 4)
    }
    const mouthOpen = upstreamRuntimeFadeAlpha(
      { fade: 'mouthOpen', side: null },
      expression,
    )
    const mouthClose = upstreamRuntimeFadeAlpha(
      { fade: 'mouthClose', side: null },
      expression,
    )
    assert.ok(mouthOpen >= 0 && mouthOpen <= 1)
    assert.ok(mouthClose >= 0 && mouthClose <= 1)
    assert.ok(Math.abs(mouthOpen + mouthClose - 1) < Number.EPSILON * 4)
  }

  const asymmetric = { ...BASE_EXPRESSION, eyeOpenL: 0, eyeOpenR: 1 }
  assert.equal(
    upstreamRuntimeFadeAlpha({ fade: 'eyeOpen', side: 'L' }, asymmetric),
    0,
  )
  assert.equal(
    upstreamRuntimeFadeAlpha({ fade: 'eyeOpen', side: 'R' }, asymmetric),
    1,
  )
})

test('vertex deformation matches the fixed 121-frame regression sequence', () => {
  const fixtures = runtimeLayers()
  const digest = createHash('sha256')

  for (let frame = 0; frame <= 120; frame += 1) {
    const expression = runtimeExpression(frame)
    for (const fixture of fixtures) {
      const actual = cloneRuntimeLayer(fixture)
      const baseBefore = new Float32Array(actual.base)
      applySpringFrame(actual, frame)
      deformUpstreamRuntimeLayer(actual, expression, FRAME)
      digest.update(JSON.stringify(Array.from(actual.cur)))
      assert.deepEqual(actual.base, baseBefore)
      assert.ok(actual.cur.every(Number.isFinite))
    }
  }

  assert.equal(digest.digest('hex'), MIGRATION_FINGERPRINTS.deformation)
})

test('disabled physics ignores strand spring displacement', () => {
  const expression = runtimeExpression(57)
  const withMotion = hairLayer()
  const withoutMotion = cloneRuntimeLayer(withMotion)
  applySpringFrame(withMotion, 57)
  for (const spring of withoutMotion.spr ?? []) {
    spring.stiff.dx = 0
    spring.soft.dx = 0
  }
  const frame = { ...FRAME, physicsEnabled: false }

  deformUpstreamRuntimeLayer(withMotion, expression, frame)
  deformUpstreamRuntimeLayer(withoutMotion, expression, frame)

  assert.deepEqual(withMotion.cur, withoutMotion.cur)
})

test('tick loop keeps blink, smoothing, breath, bounce, and springs stable', () => {
  const automation: UpstreamRuntimeTickAutomation = { idle: true, blink: true }
  const target: UpstreamRuntimeParameters = { ...UPSTREAM_RUNTIME_DEFAULTS }
  const layers = [hairLayer()]
  const random = sequenceRandom([0.35, 0.1, 0.72, 0.64])
  const state = createUpstreamRuntimeTickState(0)
  const digest = createHash('sha256')
  let sawBlink = false

  for (let frame = 1; frame <= 300; frame += 1) {
    target.angleX = Math.sin(frame * 0.021) * 0.63
    target.angleY = Math.cos(frame * 0.017) * 0.51
    target.angleZ = Math.sin(frame * 0.013 + 0.4) * 0.48
    target.body = Math.cos(frame * 0.011) * 0.42
    target.mouthOpen = 0.5 + Math.sin(frame * 0.08) * 0.45
    target.physAmp = 1.4 + Math.sin(frame * 0.019) * 0.8
    target.soft = 1.3 + Math.cos(frame * 0.016) * 0.7
    target.fhAmp = 1.2 + Math.sin(frame * 0.023) * 0.6
    target.fhSoft = 0.7 + Math.cos(frame * 0.018) * 0.3
    const nowMs = (frame * 1_000) / 60
    const expression = stepUpstreamRuntimeTick(state, {
      nowMs,
      target,
      automation,
      cameraLive: false,
      layers,
      frame: FRAME,
      random,
    })
    digest.update(
      JSON.stringify(
        stabilizeSnapshot(plainTickSnapshot(state, expression, layers)),
      ),
    )
    sawBlink ||= expression.eyeOpenL < 0.99 || expression.eyeOpenR < 0.99
    assert.equal(expression, state.expression)
    assert.ok(expression.breath >= 0 && expression.breath <= 1)
    assert.ok(expression.breathHead >= 0 && expression.breathHead <= 1)
    assert.ok(Object.values(state.current).every(Number.isFinite))
    assert.ok(Object.values(state.bounce).every(Number.isFinite))
    assert.ok(
      layers.every((item) =>
        item.spr?.every(
          (spring) =>
            Object.values(spring.stiff).every(Number.isFinite) &&
            Object.values(spring.soft).every(Number.isFinite),
        ),
      ),
    )
  }

  assert.ok(sawBlink)
  assert.equal(state.lastTimeMs, 5_000)
  assert.equal(digest.digest('hex'), MIGRATION_FINGERPRINTS.tick)
})

test('camera tracking damps physics without mutating its target', () => {
  const automation: UpstreamRuntimeTickAutomation = {
    idle: false,
    blink: false,
  }
  const target: UpstreamRuntimeParameters = { ...UPSTREAM_RUNTIME_DEFAULTS }
  const originalTarget = { ...target }
  const layers = [hairLayer()]
  const state = createUpstreamRuntimeTickState(0)
  const digest = createHash('sha256')

  for (let frame = 1; frame <= 90; frame += 1) {
    const nowMs = (frame * 1_000) / 60
    const expression = stepUpstreamRuntimeTick(state, {
      nowMs,
      target,
      automation,
      cameraLive: true,
      layers,
      frame: FRAME,
      random: () => 0.5,
    })
    digest.update(
      JSON.stringify(
        stabilizeSnapshot(plainTickSnapshot(state, expression, layers)),
      ),
    )
    assert.deepEqual(target, originalTarget)
    assert.ok(expression.physAmp <= state.current.physAmp)
    assert.ok(expression.soft <= state.current.soft)
    assert.ok(expression.fhAmp <= state.current.fhAmp)
    assert.ok(expression.fhSoft <= state.current.fhSoft)
  }

  assert.ok(state.cameraPhysicsScale > 0.5)
  assert.ok(state.cameraPhysicsScale < 0.51)
  assert.equal(digest.digest('hex'), MIGRATION_FINGERPRINTS.camera)
})

test('tick loop caps a delayed frame at fifty milliseconds', () => {
  const state = createUpstreamRuntimeTickState(0)
  const target = { ...UPSTREAM_RUNTIME_DEFAULTS, mouthOpen: 1 }
  const expression = stepUpstreamRuntimeTick(state, {
    nowMs: 1_000,
    target,
    automation: { idle: false, blink: false },
    cameraLive: false,
    layers: [],
    frame: FRAME,
    random: () => 0.5,
  })

  assert.equal(expression.mouthOpen, 0.7000000000000001)
  assert.equal(state.lastTimeMs, 1_000)
  assert.equal(target.mouthOpen, 1)
})

function sha256(value: unknown): string {
  return createHash('sha256').update(JSON.stringify(value)).digest('hex')
}

function bindingRig(canvasWidth: number): UpstreamRig {
  const pixel = {
    width: 1,
    height: 1,
    data: new Uint8ClampedArray([255, 255, 255, 255]),
  }
  return {
    canvas: { w: canvasWidth, h: 512 },
    anchors: FRAME.anchors,
    warnings: [],
    synth: { eye: false, mouth: false },
    layers: [
      {
        name: 'face',
        x: 51.25,
        y: 24.75,
        w: 18,
        h: 21,
        z: 7,
        depth: 1,
        group: 'head',
        phys: null,
        fade: null,
        side: null,
        strands: null,
        img: pixel,
      },
      {
        name: 'front hair_12_l',
        x: 39.2,
        y: 15.4,
        w: 203.6,
        h: 267.3,
        z: 11,
        depth: 1.55,
        group: 'head',
        phys: 'hair',
        fade: null,
        side: 'L',
        strands: [
          { x: 66.5, rootY: 28.5, tipY: 250.4 },
          { x: 115.25, rootY: 25.75, tipY: 263.6 },
          { x: 194.75, rootY: 31.2, tipY: 271.8 },
        ],
        synthetic: true,
        img: pixel,
      },
      {
        name: 'back hair',
        x: 18.1,
        y: 4.6,
        w: 241.8,
        h: 322.2,
        z: 0,
        depth: 0.72,
        group: 'head',
        phys: 'hair',
        fade: null,
        side: null,
        strands: [{ x: 129.4, rootY: 18.2, tipY: 318.9 }],
        img: pixel,
      },
    ],
  }
}

function plainBindingSnapshot(
  binding: ReturnType<typeof bindUpstreamRuntimeRig>,
): unknown {
  return structuredClone({
    canvas: binding.canvas,
    anchors: binding.anchors,
    faceScale: binding.faceScale,
    neckPivot: binding.neckPivot,
    bodyPivot: binding.bodyPivot,
    faceCenter: binding.faceCenter,
    chest: binding.chest,
    layers: binding.layers.map((layer: UpstreamRuntimeBoundLayer) => ({
      name: layer.name,
      bn: layer.bn,
      group: layer.group,
      side: layer.side,
      fade: layer.fade,
      x: layer.x,
      y: layer.y,
      w: layer.w,
      h: layer.h,
      z: layer.z,
      depth: layer.depth,
      phys: layer.phys,
      synthetic: layer.synthetic,
      strands: layer.strands,
      base: Array.from(layer.base),
      cur: Array.from(layer.cur),
      uv: Array.from(layer.uv),
      indices: Array.from(layer.indices),
      nIdx: layer.nIdx,
      sw: layer.sw && Array.from(layer.sw),
      su: layer.su && Array.from(layer.su),
      spr: layer.spr,
      bw: layer.bw && Array.from(layer.bw),
    })),
  })
}

function drawLayers(): UpstreamRuntimeLayer[] {
  return [
    layer(
      'eyewhite_l',
      'eyewhite',
      'head',
      'eyeOpen',
      'L',
      72,
      86,
      42,
      29,
      1.1,
    ),
    layer('irides_l', 'irides', 'head', 'eyeOpen', 'L', 83, 91, 20, 20, 1.2),
    layer(
      'mouth_open',
      'mouth_open',
      'head',
      'mouthOpen',
      null,
      98,
      153,
      62,
      35,
      1.4,
    ),
    layer('face', 'face', 'head', null, null, 52, 25, 152, 224, 1),
    layer(
      'eyewhite_r',
      'eyewhite',
      'head',
      'eyeOpen',
      'R',
      142,
      84,
      45,
      31,
      1.1,
    ),
    layer('irides_r', 'irides', 'head', 'eyeOpen', 'R', 153, 90, 21, 21, 1.2),
    layer(
      'mouth_close',
      'mouth_close',
      'head',
      'mouthClose',
      null,
      99,
      163,
      60,
      14,
      1.41,
    ),
  ]
}

function runtimeExpression(frame: number): UpstreamRuntimeExpression {
  const time = frame / 60
  return {
    ...BASE_EXPRESSION,
    angleX: Math.sin(time * 1.7) * 0.82,
    angleY: Math.cos(time * 1.3) * 0.71,
    angleZ: Math.sin(time * 0.9 + 0.3) * 0.64,
    eyeOpenL: 0.5 + Math.sin(time * 4.1) * 0.58,
    eyeOpenR: 0.5 + Math.cos(time * 3.7) * 0.58,
    eyeX: Math.sin(time * 2.3) * 0.91,
    eyeY: Math.cos(time * 2.9) * 0.78,
    brow: Math.sin(time * 1.1) * 0.74,
    mouthOpen: 0.5 + Math.sin(time * 3.3) * 0.61,
    mouthForm: Math.cos(time * 2.1) * 0.88,
    mouthCY: Math.sin(time * 1.2) * 0.76,
    body: Math.cos(time * 0.8) * 0.69,
    browAngL: Math.sin(time * 1.4) * 0.63,
    browAngR: Math.cos(time * 1.6) * 0.59,
    browAngSym: Math.sin(time * 1.8) * 0.66,
    bangL: Math.sin(time * 2.2) * 0.77,
    bangC: Math.cos(time * 2.4) * 0.72,
    bangR: Math.sin(time * 2.6) * -0.68,
    armY: Math.cos(time * 1.5) * 0.81,
    armPos: Math.sin(time * 1.9) * 0.73,
    bust: 1.4 + Math.sin(time) * 1.1,
    bustY: Math.cos(time * 0.7) * 1.9,
    irisScale: 0.85 + Math.sin(time * 2.7) * 0.31,
    mouthEase: 0.5 + Math.sin(time * 0.6) * 0.45,
    eyeEase: 0.5 + Math.cos(time * 0.5) * 0.45,
    fhAmp: 1.5 + Math.sin(time * 0.9) * 1.2,
    fhSoft: 0.8 + Math.cos(time * 0.8) * 0.7,
    physAmp: 1.4 + Math.cos(time * 0.75) * 1.2,
    soft: 1.6 + Math.sin(time * 0.65) * 1.3,
    eyeCY: Math.sin(time * 1.7) * 0.79,
    eyeCAng: Math.cos(time * 1.9) * 0.84,
    mouthCAng: Math.sin(time * 1.25) * 0.81,
    eyeScaleL: 1 + Math.sin(time * 1.45) * 0.37,
    eyeScaleR: 1 + Math.cos(time * 1.55) * 0.34,
    mouthScale: 1 + Math.sin(time * 1.35) * 0.42,
    breath: 0.5 + Math.sin((time * 2 * Math.PI) / 3.4) * 0.5,
    breathHead: 0.5 + Math.sin((time * 2 * Math.PI) / 3.4 - 0.6) * 0.5,
  }
}

function runtimeLayers(): UpstreamRuntimeLayer[] {
  return [
    layer('irides_l', 'irides', 'head', 'eyeOpen', 'L', 80, 91, 24, 20, 1.2),
    layer('eyelash_r', 'eyelash', 'head', 'eyeOpen', 'R', 142, 84, 45, 34, 1.3),
    layer(
      'eye_close_l',
      'eye_close',
      'head',
      'eyeClose',
      'L',
      72,
      98,
      42,
      15,
      1.31,
    ),
    layer('eyebrow_r', 'eyebrow', 'head', null, 'R', 143, 68, 44, 13, 1.34),
    layer(
      'mouth_open',
      'mouth_open',
      'head',
      'mouthOpen',
      null,
      98,
      153,
      62,
      35,
      1.4,
    ),
    layer(
      'mouth_close',
      'mouth_close',
      'head',
      'mouthClose',
      null,
      99,
      163,
      60,
      14,
      1.41,
    ),
    layer('face', 'face', 'head', null, null, 52, 25, 152, 224, 1),
    layer('neck', 'neck', 'body', null, null, 102, 193, 54, 82, 0.96),
    layer('topwear', 'topwear', 'body', null, null, 39, 218, 181, 132, 0.9),
    layer('handwear_l', 'handwear', 'body', null, 'L', 20, 227, 43, 118, 0.88),
    hairLayer(),
  ]
}

function layer(
  name: string,
  bn: string,
  group: 'head' | 'body',
  fade: UpstreamRuntimeLayer['fade'],
  side: UpstreamRuntimeLayer['side'],
  x: number,
  y: number,
  w: number,
  h: number,
  depth: number,
): UpstreamRuntimeLayer {
  const base = new Float32Array([x, y, x + w, y, x, y + h, x + w, y + h])
  return {
    name,
    bn,
    group,
    fade,
    side,
    x,
    y,
    w,
    h,
    depth,
    base,
    cur: new Float32Array(base),
    strands: null,
  }
}

function hairLayer(): UpstreamRuntimeLayer {
  const result = layer(
    'front hair_1',
    'front hair',
    'head',
    null,
    null,
    45,
    20,
    166,
    222,
    1.52,
  )
  result.strands = [
    { x: 83, rootY: 34, tipY: 226 },
    { x: 169, rootY: 31, tipY: 229 },
  ]
  result.sw = new Float32Array([1, 0, 0.65, 0.35, 0.35, 0.65, 0, 1])
  result.su = new Float32Array([0, 0.28, 0.73, 1])
  result.bw = new Float32Array([1, 0, 0, 0.6, 0.4, 0, 0, 0.35, 0.65, 0, 0, 1])
  result.spr = [
    {
      stiff: { x: 0, v: 0, dx: 0 },
      soft: { x: 0, v: 0, dx: 0 },
      phase: 1.52,
    },
    {
      stiff: { x: 0, v: 0, dx: 0 },
      soft: { x: 0, v: 0, dx: 0 },
      phase: 2.89,
    },
  ]
  return result
}

function cloneRuntimeLayer(layer: UpstreamRuntimeLayer): UpstreamRuntimeLayer {
  return {
    ...layer,
    base: new Float32Array(layer.base),
    cur: new Float32Array(layer.cur),
    strands: layer.strands?.map((strand) => ({ ...strand })) ?? null,
    sw: layer.sw ? new Float32Array(layer.sw) : undefined,
    su: layer.su ? new Float32Array(layer.su) : undefined,
    bw: layer.bw ? new Float32Array(layer.bw) : undefined,
    spr: layer.spr?.map((spring) => ({
      stiff: { ...spring.stiff },
      soft: { ...spring.soft },
      phase: spring.phase,
    })),
  }
}

function applySpringFrame(layer: UpstreamRuntimeLayer, frame: number): void {
  for (const spring of layer.spr ?? []) {
    spring.stiff.dx = Math.sin(frame * 0.07 + spring.phase) * 3.2
    spring.soft.dx = Math.cos(frame * 0.043 + spring.phase * 1.7) * 6.4
  }
}

function sequenceRandom(values: readonly number[]): () => number {
  let index = 0
  return () => {
    const value = values[index % values.length]
    index += 1
    return value
  }
}

function stabilizeSnapshot(value: unknown): unknown {
  if (typeof value === 'number') {
    return Number.isFinite(value) ? Math.round(value * 1e6) / 1e6 : value
  }
  if (Array.isArray(value)) {
    return value.map((item) => stabilizeSnapshot(item))
  }
  if (value && typeof value === 'object') {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>).map(([key, item]) => [
        key,
        stabilizeSnapshot(item),
      ]),
    )
  }
  return value
}

function plainTickSnapshot(
  state: ReturnType<typeof createUpstreamRuntimeTickState>,
  expression: UpstreamRuntimeExpression,
  layers: readonly UpstreamRuntimeLayer[],
): unknown {
  return structuredClone({
    lastTimeMs: state.lastTimeMs,
    blinkElapsed: state.blinkElapsed,
    nextBlinkAtMs: state.nextBlinkAtMs,
    cameraPhysicsScale: state.cameraPhysicsScale,
    current: state.current,
    expression,
    bounce: state.bounce,
    springs: layers.map((layer) => layer.spr),
  })
}
