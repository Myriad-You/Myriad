import type { CollarClipMesh } from './collarRuntime'
import type { NeckSurfaceContour } from './neckSurfaceContour'
import type { Anime25DFrameWork } from './performanceTelemetry'
import type { Anime25DPlaybackLayer } from './types'
import { requiredUniform } from './webglRuntime'

export interface Anime25DRenderableLayer {
  source: Anime25DPlaybackLayer
  vao: WebGLVertexArrayObject | null
  indexCount: number
  layerTransform: Float32Array
  frameOpacity: number
  renderKind: Anime25DRenderKind
  retainWhenHidden: boolean
  cryDirection: number
  neckSurfaceFade?: { start: number; end: number; contour?: NeckSurfaceContour }
}

export type Anime25DRenderKind = 'ordinary' | 'neck' | 'eyewhite' | 'iris'

export function anime25DLayerUsesOwnGeometry(
  renderKind: Anime25DRenderKind,
  hasCollarClip: boolean,
): boolean {
  return renderKind !== 'neck' || !hasCollarClip
}

export interface Anime25DRendererBindings {
  view: WebGLUniformLocation
  layerTransform: WebGLUniformLocation
  bodyTransform: WebGLUniformLocation
  opacity: WebGLUniformLocation
  cut: WebGLUniformLocation
  cryTime: WebGLUniformLocation
  cry: WebGLUniformLocation
  atlasRect: WebGLUniformLocation
  neckSurfaceFade: WebGLUniformLocation
  neckSurfaceContour: WebGLUniformLocation
  neckSurfaceBounds: WebGLUniformLocation
}

export interface Anime25DRenderFrame {
  viewWidth: number
  viewHeight: number
  bodyPivotX: number
  bodyPivotY: number
  bodyRotationCosine: number
  bodyRotationSine: number
  time: number
  eyeCry: number
}

/** Resolves stable shader bindings and initializes the shared blend state once. */
export function createAnime25DRendererBindings(
  gl: WebGL2RenderingContext,
  program: WebGLProgram,
): Anime25DRendererBindings {
  const bindings = {
    view: requiredUniform(gl, program, 'u_view'),
    layerTransform: requiredUniform(gl, program, 'u_layer_transform'),
    bodyTransform: requiredUniform(gl, program, 'u_body_transform'),
    opacity: requiredUniform(gl, program, 'u_opacity'),
    cut: requiredUniform(gl, program, 'u_cut'),
    cryTime: requiredUniform(gl, program, 'u_cry_time'),
    cry: requiredUniform(gl, program, 'u_cry'),
    atlasRect: requiredUniform(gl, program, 'u_atlas_rect'),
    neckSurfaceFade: requiredUniform(gl, program, 'u_neck_surface_fade'),
    neckSurfaceContour: requiredUniform(
      gl,
      program,
      'u_neck_surface_contour[0]',
    ),
    neckSurfaceBounds: requiredUniform(gl, program, 'u_neck_surface_bounds'),
  }
  gl.useProgram(program)
  gl.uniform1i(requiredUniform(gl, program, 'u_texture'), 0)
  gl.enable(gl.BLEND)
  gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA)
  gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, 1)
  return bindings
}

/** Draws the frozen collar/eye stencil plan without allocating frame objects. */
export function drawAnime25DFrame(
  gl: WebGL2RenderingContext,
  program: WebGLProgram,
  bindings: Readonly<Anime25DRendererBindings>,
  layers: readonly Anime25DRenderableLayer[],
  atlasTexture: WebGLTexture | null,
  collarClip: CollarClipMesh | null,
  frame: Readonly<Anime25DRenderFrame>,
  work?: Anime25DFrameWork,
): void {
  gl.clearColor(0, 0, 0, 0)
  gl.clear(gl.COLOR_BUFFER_BIT | gl.STENCIL_BUFFER_BIT)
  gl.useProgram(program)
  gl.uniform2f(bindings.view, frame.viewWidth, frame.viewHeight)
  gl.uniform4f(
    bindings.bodyTransform,
    frame.bodyPivotX,
    frame.bodyPivotY,
    frame.bodyRotationCosine,
    frame.bodyRotationSine,
  )
  gl.uniform1f(bindings.cryTime, frame.time)
  gl.activeTexture(gl.TEXTURE0)
  if (!atlasTexture) return
  gl.bindTexture(gl.TEXTURE_2D, atlasTexture)
  for (const layer of layers) {
    const opacity = layer.frameOpacity
    if (opacity < 0.004 && !layer.retainWhenHidden) continue
    const usesOwnGeometry = anime25DLayerUsesOwnGeometry(
      layer.renderKind,
      Boolean(collarClip),
    )
    if (usesOwnGeometry && !layer.vao) continue
    if (work) {
      work.drawnLayers += 1
      work.drawCalls += usesOwnGeometry ? 1 : 2
    }
    gl.uniformMatrix3fv(bindings.layerTransform, false, layer.layerTransform)
    gl.uniform1f(bindings.opacity, opacity)
    gl.uniform2f(
      bindings.neckSurfaceFade,
      layer.neckSurfaceFade?.start ?? 0,
      layer.neckSurfaceFade?.end ?? 0,
    )
    const contour = layer.neckSurfaceFade?.contour
    gl.uniform2f(
      bindings.neckSurfaceBounds,
      contour?.left ?? 0,
      contour?.right ?? 0,
    )
    if (contour) gl.uniform2fv(bindings.neckSurfaceContour, contour.bands)
    gl.uniform1f(bindings.cry, layer.cryDirection * frame.eyeCry)
    gl.uniform4f(
      bindings.atlasRect,
      layer.source.atlas.x,
      layer.source.atlas.y,
      layer.source.atlas.w,
      layer.source.atlas.h,
    )
    if (!usesOwnGeometry && collarClip) {
      drawCollarMaskedLayer(gl, bindings, collarClip, opacity)
      continue
    }
    gl.bindVertexArray(layer.vao)
    if (layer.renderKind === 'eyewhite') {
      gl.enable(gl.STENCIL_TEST)
      gl.stencilFunc(gl.ALWAYS, 1, 255)
      gl.stencilOp(gl.KEEP, gl.KEEP, gl.REPLACE)
      gl.uniform1f(bindings.cut, 0.25)
      gl.drawElements(gl.TRIANGLES, layer.indexCount, gl.UNSIGNED_SHORT, 0)
      gl.disable(gl.STENCIL_TEST)
      gl.uniform1f(bindings.cut, 0)
    } else if (layer.renderKind === 'iris') {
      gl.enable(gl.STENCIL_TEST)
      gl.stencilFunc(gl.EQUAL, 1, 255)
      gl.stencilOp(gl.KEEP, gl.KEEP, gl.KEEP)
      gl.uniform1f(bindings.cut, 0)
      gl.drawElements(gl.TRIANGLES, layer.indexCount, gl.UNSIGNED_SHORT, 0)
      gl.disable(gl.STENCIL_TEST)
    } else {
      gl.uniform1f(bindings.cut, 0)
      gl.drawElements(gl.TRIANGLES, layer.indexCount, gl.UNSIGNED_SHORT, 0)
    }
  }
  gl.bindVertexArray(null)
}

function drawCollarMaskedLayer(
  gl: WebGL2RenderingContext,
  bindings: Readonly<Anime25DRendererBindings>,
  collarClip: Readonly<CollarClipMesh>,
  opacity: number,
): void {
  gl.enable(gl.STENCIL_TEST)
  gl.stencilMask(255)
  gl.stencilFunc(gl.ALWAYS, 1, 255)
  gl.stencilOp(gl.KEEP, gl.KEEP, gl.REPLACE)
  gl.colorMask(false, false, false, false)
  gl.uniform1f(bindings.opacity, 1)
  gl.uniform1f(bindings.cry, 0)
  gl.uniform1f(bindings.cut, 0)
  gl.bindVertexArray(collarClip.vao)
  gl.drawElements(gl.TRIANGLES, collarClip.indexCount, gl.UNSIGNED_SHORT, 0)
  gl.colorMask(true, true, true, true)
  gl.stencilMask(0)
  gl.stencilFunc(gl.EQUAL, 1, 255)
  gl.stencilOp(gl.KEEP, gl.KEEP, gl.KEEP)
  gl.uniform1f(bindings.opacity, opacity)
  gl.bindVertexArray(collarClip.vao)
  gl.drawElements(gl.TRIANGLES, collarClip.indexCount, gl.UNSIGNED_SHORT, 0)
  gl.stencilMask(255)
  gl.disable(gl.STENCIL_TEST)
}
