import type { CollarClipMesh } from './collarRuntime'
import type {
  Anime25DRenderableLayer,
  Anime25DRendererBindings,
} from './renderer'
import type { Anime25DPlaybackLayer } from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { createAnime25DFrameWork } from './performanceTelemetry'
import { anime25DLayerUsesOwnGeometry, drawAnime25DFrame } from './renderer'

test('renderer preserves collar and eye stencil order while skipping hidden art', () => {
  const calls: string[] = []
  let boundVao = 'none'
  const gl = fakeGl(
    calls,
    (vao) => {
      boundVao = vao
    },
    () => boundVao,
  )
  const bindings = fakeBindings()
  const layers = [
    renderLayer('neck', 'neck', 1, 12),
    renderLayer('eyewhite_L', 'eye-open', 0, 9),
    renderLayer('irides_L', 'iris', 0.8, 9),
    renderLayer('face', 'face', 1, 18),
    renderLayer('hidden_accent', 'accent', 0, 6),
  ]
  layers[0].vao = null
  const collarClip = {
    vao: 'clip',
    indexCount: 6,
  } as unknown as CollarClipMesh
  const work = createAnime25DFrameWork()

  drawAnime25DFrame(
    gl,
    'program' as unknown as WebGLProgram,
    bindings,
    layers,
    'atlas' as unknown as WebGLTexture,
    collarClip,
    {
      viewWidth: 900,
      viewHeight: 1200,
      bodyPivotX: 450,
      bodyPivotY: 780,
      bodyRotationCosine: 0.98,
      bodyRotationSine: 0.2,
      time: 2.5,
      eyeCry: 0.7,
    },
    work,
  )

  assert.deepEqual(
    calls.filter((call) => call.startsWith('draw:')),
    [
      'draw:clip:6',
      'draw:clip:6',
      'draw:eyewhite_L:9',
      'draw:irides_L:9',
      'draw:face:18',
    ],
  )
  assert.deepEqual(
    calls.filter((call) => call.startsWith('stencilFunc:')),
    ['stencilFunc:20', 'stencilFunc:21', 'stencilFunc:20', 'stencilFunc:21'],
  )
  assert.equal(calls.at(-1), 'bindVao:none')
  assert.equal(calls.includes('bindVao:neck'), false)
  assert.equal(work.drawnLayers, 4)
  assert.equal(work.drawCalls, 5)
})

test('collar clip is the sole geometry source for a replaced neck', () => {
  assert.equal(anime25DLayerUsesOwnGeometry('neck', true), false)
  assert.equal(anime25DLayerUsesOwnGeometry('neck', false), true)
  assert.equal(anime25DLayerUsesOwnGeometry('ordinary', true), true)
})

test('renderer clears the frame but submits no layers before atlas readiness', () => {
  const calls: string[] = []
  let boundVao = 'none'
  const gl = fakeGl(
    calls,
    (vao) => {
      boundVao = vao
    },
    () => boundVao,
  )

  drawAnime25DFrame(
    gl,
    'program' as unknown as WebGLProgram,
    fakeBindings(),
    [renderLayer('face', 'face', 1, 18)],
    null,
    null,
    {
      viewWidth: 1,
      viewHeight: 1,
      bodyPivotX: 0,
      bodyPivotY: 0,
      bodyRotationCosine: 1,
      bodyRotationSine: 0,
      time: 0,
      eyeCry: 0,
    },
  )

  assert.ok(calls.includes('clear'))
  assert.equal(
    calls.some((call) => call.startsWith('draw:')),
    false,
  )
})

function renderLayer(
  name: string,
  role: string,
  frameOpacity: number,
  indexCount: number,
): Anime25DRenderableLayer {
  const renderKind =
    role === 'neck'
      ? 'neck'
      : name.startsWith('eyewhite')
        ? 'eyewhite'
        : name.startsWith('irides')
          ? 'iris'
          : 'ordinary'
  return {
    source: {
      name,
      role,
      atlas: { x: 0, y: 0, w: 1, h: 1 },
    } as Anime25DPlaybackLayer,
    vao: name as unknown as WebGLVertexArrayObject,
    indexCount,
    layerTransform: new Float32Array(9),
    frameOpacity,
    renderKind,
    retainWhenHidden: name.startsWith('eyewhite'),
    cryDirection: 0,
  }
}

function fakeBindings(): Anime25DRendererBindings {
  const location = (name: string) => name as unknown as WebGLUniformLocation
  return {
    view: location('view'),
    layerTransform: location('layerTransform'),
    bodyTransform: location('bodyTransform'),
    opacity: location('opacity'),
    cut: location('cut'),
    cryTime: location('cryTime'),
    cry: location('cry'),
    atlasRect: location('atlasRect'),
  }
}

function fakeGl(
  calls: string[],
  setBoundVao: (vao: string) => void,
  getBoundVao: () => string,
): WebGL2RenderingContext {
  return {
    COLOR_BUFFER_BIT: 1,
    STENCIL_BUFFER_BIT: 2,
    TEXTURE0: 3,
    TEXTURE_2D: 4,
    TRIANGLES: 5,
    UNSIGNED_SHORT: 6,
    STENCIL_TEST: 7,
    KEEP: 10,
    REPLACE: 11,
    ALWAYS: 20,
    EQUAL: 21,
    clearColor: () => calls.push('clearColor'),
    clear: () => calls.push('clear'),
    useProgram: () => calls.push('useProgram'),
    uniform2f: () => calls.push('uniform2f'),
    uniform4f: () => calls.push('uniform4f'),
    uniform1f: () => calls.push('uniform1f'),
    uniformMatrix3fv: () => calls.push('uniformMatrix3fv'),
    activeTexture: () => calls.push('activeTexture'),
    bindTexture: () => calls.push('bindTexture'),
    bindVertexArray: (vao: WebGLVertexArrayObject | null) => {
      const name = vao == null ? 'none' : String(vao)
      setBoundVao(name)
      calls.push(`bindVao:${name}`)
    },
    enable: () => calls.push('enable'),
    disable: () => calls.push('disable'),
    stencilMask: () => calls.push('stencilMask'),
    stencilFunc: (func: number) => calls.push(`stencilFunc:${func}`),
    stencilOp: () => calls.push('stencilOp'),
    colorMask: () => calls.push('colorMask'),
    drawElements: (_mode: number, count: number) => {
      calls.push(`draw:${getBoundVao()}:${count}`)
    },
  } as unknown as WebGL2RenderingContext
}
