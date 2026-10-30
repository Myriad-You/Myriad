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

test('renderer preserves collar stencil and eye mask order while skipping hidden art', () => {
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
    ['stencilFunc:20', 'stencilFunc:21'],
    'only the collar still uses the stencil',
  )
  assert.deepEqual(
    calls.filter(
      (call) =>
        call.startsWith('bindFramebuffer:') ||
        call.startsWith('uniform2f:eyeMaskChannel') ||
        call === 'draw:eyewhite_L:9' ||
        call === 'draw:irides_L:9',
    ),
    [
      'bindFramebuffer:mask',
      'bindFramebuffer:none',
      'bindFramebuffer:mask',
      'draw:eyewhite_L:9',
      'bindFramebuffer:none',
      'draw:eyewhite_L:9',
      'uniform2f:eyeMaskChannel:1:0',
      'draw:irides_L:9',
      'uniform2f:eyeMaskChannel:0:0',
    ],
    'whites fill the mask first; the iris reads its own eye, then resets',
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

test('eye masks isolate both eyes and collars across paint orders and frames', () => {
  // Overlapping eye pixels must keep coverage in both channels.
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
    const mask = Array.from({ length: 5 }, () => [0, 0])
    let framebuffer = 'none'
    let maskChannels = [true, true]
    let irisChannel = [0, 0]
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
      face: [0, 4],
      backHair: [0, 1, 2, 3, 4],
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
      clear: (bits: number) => {
        if (framebuffer === 'mask' && bits & gl.COLOR_BUFFER_BIT) {
          for (const pixel of mask) pixel.fill(0)
        }
        if (bits & gl.STENCIL_BUFFER_BIT) {
          for (let i = 0; i < stencil.length; i += 1) stencil[i] &= ~writeMask
        }
      },
      bindFramebuffer: (_target: number, next: unknown) => {
        framebuffer = next == null ? 'none' : String(next)
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
      colorMask: (red: boolean, green: boolean) => {
        color = red
        maskChannels = [red, green]
      },
      uniform1f: (location: WebGLUniformLocation, value: number) => {
        if (String(location) === 'opacity') opacity = value
      },
      uniform2f: (location: WebGLUniformLocation, x: number, y: number) => {
        if (String(location) === 'eyeMaskChannel') irisChannel = [x, y]
      },
      drawElements: () => {
        if (framebuffer === 'mask') {
          for (const pixel of coverage[bound] ?? []) {
            if (maskChannels[0]) mask[pixel][0] = 1
            if (maskChannels[1]) mask[pixel][1] = 1
          }
          return
        }
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
          const eye =
            irisChannel[0] || irisChannel[1]
              ? mask[pixel][0] * irisChannel[0] + mask[pixel][1] * irisChannel[1]
              : 1
          if (color && opacity > 0 && eye > 0) visible.push(pixel)
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
    const hair = renderLayer('backHair', 'ordinary', 1, 6)
    const face = renderLayer('face', 'face', 1, 6)
    face.crownOccluders = [{ layer: hair, start: 0.3, end: 0.5 }]
    layers.unshift(hair, face)
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
    assert.deepEqual(painted.get('backHair'), [0, 4], 'replay uses only the face stencil')
    assert.deepEqual(mask[1], [1, 1], 'overlapping eyes keep both channels')
    draw(layers.filter((layer) => layer.renderKind !== 'eyewhite'))
    assert.deepEqual(painted.get('irides_L'), [])
    assert.deepEqual(painted.get('irides_R'), [])
    assert.deepEqual(painted.get('clip'), [3, 4])
    assert.deepEqual(painted.get('backHair'), [0, 4])
  }
})

test('crown replay resets its shader band before later art and frames and skips hidden hair', () => {
  const calls: string[] = []; let bound = 'none'
  const gl = fakeGl(calls, vao => { bound = vao }, () => bound)
  const hair = renderLayer('backHair', 'ordinary', 1, 6)
  const face = renderLayer('face', 'face', 1, 6)
  const accessory = renderLayer('headwear', 'ordinary', 1, 6)
  face.crownOccluders = [{ layer: hair, start: 0.3, end: 0.5 }]
  const frame = { viewWidth: 100, viewHeight: 100, bodyPivotX: 50, bodyPivotY: 100,
    bodyRotationCosine: 1, bodyRotationSine: 0, time: 0, eyeCry: 0 }
  const draw = () => {
    calls.length = 0
    const work = createAnime25DFrameWork()
    drawAnime25DFrame(gl, {} as WebGLProgram, fakeBindings(), [hair, face, accessory], {} as WebGLTexture, null, frame, work)
    return work
  }
  assert.equal(draw().drawCalls, 5)
  assert.deepEqual(calls.filter(call => call.startsWith('draw:')), [
    'draw:backHair:6', 'draw:face:6', 'draw:face:6', 'draw:backHair:6', 'draw:headwear:6',
  ])
  assert.deepEqual(calls.filter(call => call.startsWith('uniform2f:crownBand')), [
    'uniform2f:crownBand:0:0', 'uniform2f:crownBand:0:0', 'uniform2f:crownBand:0:0',
    'uniform2f:crownBand:0.3:0.5', 'uniform2f:crownBand:0:0',
  ])
  hair.frameOpacity = 0
  draw()
  assert.equal(calls.includes('draw:backHair:6'), false)
  face.frameOpacity = 0
  assert.equal(draw().drawCalls, 1)
  assert.deepEqual(calls.filter(call => call.startsWith('uniform2f:crownBand')), ['uniform2f:crownBand:0:0'])
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
    bodyLift: location('bodyLift'),
    bodyPitch: location('bodyPitch'),
    opacity: location('opacity'),
    cut: location('cut'),
    cryTime: location('cryTime'),
    cry: location('cry'),
    atlasRect: location('atlasRect'),
    crownBand: location('crownBand'),
    eyeMaskChannel: location('eyeMaskChannel'),
    eyeMaskPass: location('eyeMaskPass'),
    eyeMask: { framebuffer: null, texture: null, width: 0, height: 0 },
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
    FRAMEBUFFER: 8,
    COLOR_ATTACHMENT0: 9,
    RGBA: 12,
    UNSIGNED_BYTE: 13,
    NEAREST: 14,
    CLAMP_TO_EDGE: 15,
    TEXTURE_MIN_FILTER: 16,
    TEXTURE_MAG_FILTER: 17,
    TEXTURE_WRAP_S: 18,
    TEXTURE_WRAP_T: 19,
    drawingBufferWidth: 5,
    drawingBufferHeight: 1,
    KEEP: 10,
    REPLACE: 11,
    ALWAYS: 20,
    EQUAL: 21,
    clearColor: () => calls.push('clearColor'),
    createTexture: () => 'maskTexture' as unknown as WebGLTexture,
    createFramebuffer: () => 'mask' as unknown as WebGLFramebuffer,
    bindFramebuffer: (_target: number, framebuffer: unknown) =>
      calls.push(`bindFramebuffer:${framebuffer ?? 'none'}`),
    framebufferTexture2D: () => calls.push('framebufferTexture2D'),
    texImage2D: () => calls.push('texImage2D'),
    texParameteri: () => {},
    clearStencil: () => calls.push('clearStencil'),
    clear: () => calls.push('clear'),
    useProgram: () => calls.push('useProgram'),
    uniform2f: (location: WebGLUniformLocation, x: number, y: number) =>
      calls.push(`uniform2f:${location}:${x}:${y}`),
    uniform2fv: (location: WebGLUniformLocation, values: Float32Array) =>
      calls.push(`uniform2fv:${location}:${values.length}`),
    uniform4f: () => calls.push('uniform4f'),
    uniform3f: () => calls.push('uniform3f'),
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
