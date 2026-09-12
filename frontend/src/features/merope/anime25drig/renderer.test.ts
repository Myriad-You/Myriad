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
      'draw:eyewhite_L:9',
      'draw:clip:6',
      'draw:clip:6',
      'draw:eyewhite_L:9',
      'draw:irides_L:9',
      'draw:face:18',
    ],
  )
  assert.deepEqual(
    calls.filter((call) => call.startsWith('stencilFunc:')),
    ['stencilFunc:20', 'stencilFunc:20', 'stencilFunc:21', 'stencilFunc:21'],
  )
  assert.equal(calls.at(-1), 'bindVao:none')
  assert.equal(calls.includes('bindVao:neck'), false)
  assert.equal(work.drawnLayers, 4)
  assert.equal(work.drawCalls, 6)
})

test('collar clip is the sole geometry source for a replaced neck', () => {
  assert.equal(anime25DLayerUsesOwnGeometry('neck', true), false)
  assert.equal(anime25DLayerUsesOwnGeometry('neck', false), true)
  assert.equal(anime25DLayerUsesOwnGeometry('ordinary', true), true)
})

test('stencil execution isolates both eyes and collars across paint orders and frames', () => {
  // Overlapping eye pixels must retain both bits.
  for (const collarIndex of [0, 2, 6]) {
    let bound = ''
    const gl = fakeGl(
      [],
      (vao) => {
        bound = vao
      },
      () => bound,
    )
    const stencil = new Uint8Array(5)
    const painted = new Map<string, number[]>()
    const coverage: Record<string, number[]> = {
      eyewhite_L: [0],
      eyewhite_fragment_L: [1],
      eyewhite_R: [1, 2],
      hidden_white_L: [3],
      clip: [3, 4],
      irides_L: [0, 1, 2, 3, 4],
      irides_R: [0, 1, 2, 3, 4],
      accessory: [0, 1, 2, 3, 4],
    }
    let writeMask = 255
    let readMask = 255
    let reference = 0
    let func = gl.ALWAYS
    let operation = gl.KEEP
    let enabled = false
    let color = true
    let opacity = 1
    Object.assign(gl, {
      clear: (mask: number) => {
        if (mask & gl.STENCIL_BUFFER_BIT) {
          for (let i = 0; i < stencil.length; i += 1) stencil[i] &= ~writeMask
        }
      },
      stencilMask: (mask: number) => {
        writeMask = mask
      },
      stencilFunc: (next: number, ref: number, mask: number) => {
        func = next
        reference = ref
        readMask = mask
      },
      stencilOp: (_fail: number, _depthFail: number, pass: number) => {
        operation = pass
      },
      enable: (cap: number) => {
        if (cap === gl.STENCIL_TEST) enabled = true
      },
      disable: (cap: number) => {
        if (cap === gl.STENCIL_TEST) enabled = false
      },
      colorMask: (red: boolean) => {
        color = red
      },
      uniform1f: (location: WebGLUniformLocation, value: number) => {
        if (String(location) === 'opacity') opacity = value
      },
      drawElements: () => {
        const visible: number[] = []
        for (const pixel of coverage[bound] ?? []) {
          const pass =
            !enabled ||
            func === gl.ALWAYS ||
            (stencil[pixel] & readMask) === (reference & readMask)
          if (!pass) continue
          if (enabled && operation === gl.REPLACE) {
            stencil[pixel] =
              (stencil[pixel] & ~writeMask) | (reference & writeMask)
          }
          if (color && opacity > 0) visible.push(pixel)
        }
        if (color && opacity > 0) painted.set(bound, visible)
      },
    })
    let layers = [
      renderLayer('irides_L', 'iris', 1, 6),
      renderLayer('eyewhite_R', 'eyewhite', 0, 6),
      renderLayer('irides_R', 'iris', 1, 6),
      renderLayer('eyewhite_L', 'eyewhite', 0, 6),
      renderLayer('eyewhite_fragment_L', 'eyewhite', 0, 6),
      renderLayer('accessory', 'ordinary', 1, 6),
    ]
    const hiddenVariant = renderLayer('hidden_white_L', 'ordinary', 0, 6)
    hiddenVariant.renderKind = 'eyewhite'
    layers.push(hiddenVariant)
    layers = layers.toSpliced(collarIndex, 0, renderLayer('neck', 'neck', 1, 6))
    const frame = {
      viewWidth: 5,
      viewHeight: 1,
      bodyPivotX: 0,
      bodyPivotY: 0,
      bodyRotationCosine: 1,
      bodyRotationSine: 0,
      time: 0,
      eyeCry: 0,
    }
    const draw = (current: Anime25DRenderableLayer[]) =>
      drawAnime25DFrame(
        gl,
        {} as WebGLProgram,
        fakeBindings(),
        current,
        {} as WebGLTexture,
        { vao: 'clip', indexCount: 6 } as unknown as CollarClipMesh,
        frame,
      )
    draw(layers)
    assert.deepEqual(painted.get('irides_L'), [0, 1])
    assert.deepEqual(painted.get('irides_R'), [1, 2])
    assert.deepEqual(painted.get('clip'), [3, 4])
    assert.deepEqual(painted.get('accessory'), [0, 1, 2, 3, 4])
    assert.equal(stencil[1] & 3, 3, 'overlapping eyes retain independent bits')
    draw(layers.filter((layer) => layer.renderKind !== 'eyewhite'))
    assert.deepEqual(painted.get('irides_L'), [])
    assert.deepEqual(painted.get('irides_R'), [])
    assert.deepEqual(painted.get('clip'), [3, 4])
  }
})

test('open-neck fading is draw-local and is reset before accessories and collar stencils', () => {
  const calls: string[] = []
  let boundVao = 'none'
  const gl = fakeGl(
    calls,
    (vao) => {
      boundVao = vao
    },
    () => boundVao,
  )
  const openNeck = renderLayer('open-neck', 'neck', 1, 12)
  openNeck.neckSurfaceFade = {
    start: 0.8,
    end: 0.95,
    contour: { left: 0.1, right: 0.9, bands: new Float32Array(32).fill(0.85) },
  }
  const frame = {
    viewWidth: 900,
    viewHeight: 1200,
    bodyPivotX: 450,
    bodyPivotY: 780,
    bodyRotationCosine: 1,
    bodyRotationSine: 0,
    time: 0,
    eyeCry: 0,
  }
  drawAnime25DFrame(
    gl,
    {} as WebGLProgram,
    fakeBindings(),
    [
      renderLayer('topwear', 'ordinary', 1, 18),
      openNeck,
      renderLayer('neckwear', 'ordinary', 1, 6),
    ],
    {} as WebGLTexture,
    null,
    frame,
  )
  drawAnime25DFrame(
    gl,
    {} as WebGLProgram,
    fakeBindings(),
    [renderLayer('collared-neck', 'neck', 1, 12)],
    {} as WebGLTexture,
    { vao: 'clip', indexCount: 6 } as unknown as CollarClipMesh,
    frame,
  )
  assert.deepEqual(
    calls.filter(
      (call) =>
        call.startsWith('uniform2f:neckSurfaceBounds') ||
        call.startsWith('uniform2fv:neckSurfaceContour'),
    ),
    [
      'uniform2f:neckSurfaceBounds:0:0',
      'uniform2f:neckSurfaceBounds:0.1:0.9',
      'uniform2fv:neckSurfaceContour:32',
      'uniform2f:neckSurfaceBounds:0:0',
      'uniform2f:neckSurfaceBounds:0:0',
    ],
  )
  assert.deepEqual(
    calls.filter(
      (call) =>
        call.startsWith('uniform2f:neckSurfaceFade') ||
        call.startsWith('draw:'),
    ),
    [
      'uniform2f:neckSurfaceFade:0:0',
      'draw:topwear:18',
      'uniform2f:neckSurfaceFade:0.8:0.95',
      'draw:open-neck:12',
      'uniform2f:neckSurfaceFade:0:0',
      'draw:neckwear:6',
      'uniform2f:neckSurfaceFade:0:0',
      'draw:clip:6',
      'draw:clip:6',
    ],
  )
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
      side: name.endsWith('_L') ? 'L' : name.endsWith('_R') ? 'R' : null,
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
    neckSurfaceFade: location('neckSurfaceFade'),
    neckSurfaceContour: location('neckSurfaceContour'),
    neckSurfaceBounds: location('neckSurfaceBounds'),
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
    clearStencil: () => calls.push('clearStencil'),
    clear: () => calls.push('clear'),
    useProgram: () => calls.push('useProgram'),
    uniform2f: (location: WebGLUniformLocation, x: number, y: number) =>
      calls.push(`uniform2f:${location}:${x}:${y}`),
    uniform2fv: (location: WebGLUniformLocation, values: Float32Array) =>
      calls.push(`uniform2fv:${location}:${values.length}`),
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
