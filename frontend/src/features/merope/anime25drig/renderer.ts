import type { CollarClipMesh } from './collarRuntime'
import type { NeckSurfaceContour } from './neckSurfaceContour'
import type { Anime25DFrameWork } from './performanceTelemetry'
import type { Anime25DPlaybackLayer } from './types'
import { requiredUniform } from './webglRuntime'

const COLLAR_STENCIL = 4
const CROWN_STENCIL = 8
const EYE_MASK_UNIT = 1

/** Mask channel per eye: red for the left, green for the right. */
function eyeMaskChannel(layer: Anime25DRenderableLayer): 0 | 1 | null {
  return layer.source.side === 'L' ? 0 : layer.source.side === 'R' ? 1 : null
}

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
  crownOccluders?: { layer: Anime25DRenderableLayer; start: number; end: number }[]
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
  crownBand: WebGLUniformLocation
  eyeMaskChannel: WebGLUniformLocation
  eyeMaskPass: WebGLUniformLocation
  /** Canvas-sized eye-white coverage, rebuilt when the drawing buffer resizes. */
  eyeMask: Anime25DEyeMaskTarget
}

/**
 * Soft eye-white coverage for clipping irises. A stencil is binary and alpha
 * tested, so the clip stair-stepped along the white's edge (upstream 7ddbd99).
 */
export interface Anime25DEyeMaskTarget {
  framebuffer: WebGLFramebuffer | null
  texture: WebGLTexture | null
  width: number
  height: number
}

export interface Anime25DRenderFrame {
  viewWidth: number
  viewHeight: number
  bodyPivotX: number
  bodyPivotY: number
  /** The lean the shoulders and everything above them take. */
  bodyRotationCosine: number
  bodyRotationSine: number
  /** Height above the pivot over which the torso bends into the lean; 0 is rigid. */
  bodyBendHeight: number
  time: number
  eyeCry: number
}

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
    crownBand: requiredUniform(gl, program, 'u_crown_band'),
    eyeMaskChannel: requiredUniform(gl, program, 'u_eye_mask_channel'),
    eyeMaskPass: requiredUniform(gl, program, 'u_eye_mask_pass'),
    eyeMask: { framebuffer: null, texture: null, width: 0, height: 0 },
  }
  gl.useProgram(program)
  gl.uniform1i(requiredUniform(gl, program, 'u_texture'), 0)
  gl.uniform1i(requiredUniform(gl, program, 'u_eye_mask'), EYE_MASK_UNIT)
  gl.enable(gl.BLEND)
  gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA)
  gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, 1)
  return bindings
}

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
  gl.disable(gl.STENCIL_TEST)
  gl.stencilMask(255)
  gl.clearStencil(0)
  gl.colorMask(true, true, true, true)
  gl.clearColor(0, 0, 0, 0)
  gl.clear(gl.COLOR_BUFFER_BIT | gl.STENCIL_BUFFER_BIT)
  gl.useProgram(program)
  gl.uniform2f(bindings.view, frame.viewWidth, frame.viewHeight)
  gl.uniform4f(
    bindings.bodyTransform,
    frame.bodyPivotX,
    frame.bodyPivotY,
    Math.atan2(frame.bodyRotationSine, frame.bodyRotationCosine),
    frame.bodyBendHeight,
  )
  gl.uniform1f(bindings.cryTime, frame.time)
  gl.activeTexture(gl.TEXTURE0)
  if (!atlasTexture) return
  gl.bindTexture(gl.TEXTURE_2D, atlasTexture)
  drawEyeMask(gl, bindings, layers, frame, work)
  for (const layer of layers) {
    const opacity = layer.frameOpacity
    if (opacity < 0.004 && !layer.retainWhenHidden) continue
    if (layer.renderKind === 'iris' && eyeMaskChannel(layer) === null) continue
    const usesOwnGeometry = anime25DLayerUsesOwnGeometry(
      layer.renderKind,
      Boolean(collarClip),
    )
    if (usesOwnGeometry && !layer.vao) continue
    if (work) {
      work.drawnLayers += 1
      work.drawCalls += usesOwnGeometry ? 1 : 2
    }
    bindLayerUniforms(gl, bindings, layer, frame)
    if (!usesOwnGeometry && collarClip) {
      drawCollarMaskedLayer(gl, bindings, collarClip, opacity)
      continue
    }
    gl.bindVertexArray(layer.vao)
    if (layer.renderKind === 'iris') {
      const channel = eyeMaskChannel(layer)
      gl.uniform2f(
        bindings.eyeMaskChannel,
        channel === 0 ? 1 : 0,
        channel === 1 ? 1 : 0,
      )
      gl.uniform1f(bindings.cut, 0)
      gl.drawElements(gl.TRIANGLES, layer.indexCount, gl.UNSIGNED_SHORT, 0)
      gl.uniform2f(bindings.eyeMaskChannel, 0, 0)
    } else {
      gl.uniform1f(bindings.cut, 0)
      gl.drawElements(gl.TRIANGLES, layer.indexCount, gl.UNSIGNED_SHORT, 0)
    }
    if (layer.crownOccluders?.length) {
      // Restrict the upper hair replay to opaque face pixels. The original
      // back-hair draw still owns everything outside the face, including its
      // antialiased silhouette; drawing it twice there would darken the fringe.
      gl.enable(gl.STENCIL_TEST)
      gl.stencilMask(CROWN_STENCIL)
      gl.clear(gl.STENCIL_BUFFER_BIT)
      gl.stencilFunc(gl.ALWAYS, CROWN_STENCIL, CROWN_STENCIL)
      gl.stencilOp(gl.KEEP, gl.KEEP, gl.REPLACE)
      gl.colorMask(false, false, false, false)
      gl.uniform1f(bindings.cut, 0.25)
      gl.drawElements(gl.TRIANGLES, layer.indexCount, gl.UNSIGNED_SHORT, 0)
      if (work) work.drawCalls += 1
      gl.colorMask(true, true, true, true)
      gl.stencilMask(0)
      gl.stencilFunc(gl.EQUAL, CROWN_STENCIL, CROWN_STENCIL)
      gl.stencilOp(gl.KEEP, gl.KEEP, gl.KEEP)
      for (const crown of layer.crownOccluders) {
        if (!crown.layer.vao || crown.layer.frameOpacity < 0.004) continue
        bindLayerUniforms(gl, bindings, crown.layer, frame)
        gl.uniform2f(bindings.crownBand, crown.start, crown.end)
        gl.uniform1f(bindings.cut, 0)
        gl.bindVertexArray(crown.layer.vao)
        gl.drawElements(gl.TRIANGLES, crown.layer.indexCount, gl.UNSIGNED_SHORT, 0)
        if (work) work.drawCalls += 1
      }
      gl.disable(gl.STENCIL_TEST)
    }
  }
  gl.stencilMask(255)
  gl.bindVertexArray(null)
}

/**
 * Paints every eye white's own alpha into its eye's mask channel. Hidden
 * ordinary whites still count, so an iris stays clipped during expression fades.
 */
function drawEyeMask(
  gl: WebGL2RenderingContext,
  bindings: Readonly<Anime25DRendererBindings>,
  layers: readonly Anime25DRenderableLayer[],
  frame: Readonly<Anime25DRenderFrame>,
  work?: Anime25DFrameWork,
): void {
  const whites = layers.filter(
    (layer) =>
      layer.renderKind === 'eyewhite' &&
      layer.vao &&
      eyeMaskChannel(layer) !== null &&
      (layer.frameOpacity >= 0.004 || layer.retainWhenHidden),
  )
  const hasIris = layers.some((layer) => layer.renderKind === 'iris')
  if (!hasIris) return
  const target = ensureEyeMaskTarget(gl, bindings.eyeMask)
  // The mask is written below; it must not also sit on a sampled unit.
  gl.activeTexture(gl.TEXTURE0 + EYE_MASK_UNIT)
  gl.bindTexture(gl.TEXTURE_2D, null)
  gl.bindFramebuffer(gl.FRAMEBUFFER, target.framebuffer)
  gl.colorMask(true, true, true, true)
  gl.clearColor(0, 0, 0, 0)
  gl.clear(gl.COLOR_BUFFER_BIT)
  gl.uniform1f(bindings.eyeMaskPass, 1)
  for (const layer of whites) {
    const channel = eyeMaskChannel(layer)
    bindLayerUniforms(gl, bindings, layer, frame)
    gl.uniform1f(bindings.cut, 0)
    gl.colorMask(channel === 0, channel === 1, false, false)
    gl.bindVertexArray(layer.vao)
    gl.drawElements(gl.TRIANGLES, layer.indexCount, gl.UNSIGNED_SHORT, 0)
    if (work) work.drawCalls += 1
  }
  gl.uniform1f(bindings.eyeMaskPass, 0)
  gl.colorMask(true, true, true, true)
  gl.bindFramebuffer(gl.FRAMEBUFFER, null)
  gl.bindTexture(gl.TEXTURE_2D, target.texture)
  gl.activeTexture(gl.TEXTURE0)
}

function ensureEyeMaskTarget(
  gl: WebGL2RenderingContext,
  target: Anime25DEyeMaskTarget,
): Anime25DEyeMaskTarget {
  const width = Math.max(1, gl.drawingBufferWidth)
  const height = Math.max(1, gl.drawingBufferHeight)
  if (
    target.framebuffer &&
    target.texture &&
    target.width === width &&
    target.height === height
  ) {
    return target
  }
  target.texture ??= gl.createTexture()
  target.framebuffer ??= gl.createFramebuffer()
  gl.activeTexture(gl.TEXTURE0 + EYE_MASK_UNIT)
  gl.bindTexture(gl.TEXTURE_2D, target.texture)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE)
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE)
  gl.texImage2D(
    gl.TEXTURE_2D,
    0,
    gl.RGBA,
    width,
    height,
    0,
    gl.RGBA,
    gl.UNSIGNED_BYTE,
    null,
  )
  gl.bindTexture(gl.TEXTURE_2D, null)
  gl.bindFramebuffer(gl.FRAMEBUFFER, target.framebuffer)
  gl.framebufferTexture2D(
    gl.FRAMEBUFFER,
    gl.COLOR_ATTACHMENT0,
    gl.TEXTURE_2D,
    target.texture,
    0,
  )
  gl.bindFramebuffer(gl.FRAMEBUFFER, null)
  gl.activeTexture(gl.TEXTURE0)
  target.width = width
  target.height = height
  return target
}

export function disposeAnime25DRendererBindings(
  gl: WebGL2RenderingContext,
  bindings: Anime25DRendererBindings,
): void {
  if (bindings.eyeMask.framebuffer) {
    gl.deleteFramebuffer(bindings.eyeMask.framebuffer)
  }
  if (bindings.eyeMask.texture) gl.deleteTexture(bindings.eyeMask.texture)
  bindings.eyeMask.framebuffer = null
  bindings.eyeMask.texture = null
  bindings.eyeMask.width = 0
  bindings.eyeMask.height = 0
}

function bindLayerUniforms(
  gl: WebGL2RenderingContext,
  bindings: Readonly<Anime25DRendererBindings>,
  layer: Anime25DRenderableLayer,
  frame: Readonly<Anime25DRenderFrame>,
): void {
  gl.uniformMatrix3fv(bindings.layerTransform, false, layer.layerTransform)
  gl.uniform1f(bindings.opacity, layer.frameOpacity)
  gl.uniform2f(bindings.crownBand, 0, 0)
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
}

function drawCollarMaskedLayer(
  gl: WebGL2RenderingContext,
  bindings: Readonly<Anime25DRendererBindings>,
  collarClip: Readonly<CollarClipMesh>,
  opacity: number,
): void {
  gl.enable(gl.STENCIL_TEST)
  gl.stencilMask(COLLAR_STENCIL)
  gl.clear(gl.STENCIL_BUFFER_BIT)
  gl.stencilFunc(gl.ALWAYS, COLLAR_STENCIL, COLLAR_STENCIL)
  gl.stencilOp(gl.KEEP, gl.KEEP, gl.REPLACE)
  gl.colorMask(false, false, false, false)
  gl.uniform1f(bindings.opacity, 1)
  gl.uniform1f(bindings.cry, 0)
  gl.uniform1f(bindings.cut, 0)
  gl.bindVertexArray(collarClip.vao)
  gl.drawElements(gl.TRIANGLES, collarClip.indexCount, gl.UNSIGNED_SHORT, 0)
  gl.colorMask(true, true, true, true)
  gl.stencilMask(0)
  gl.stencilFunc(gl.EQUAL, COLLAR_STENCIL, COLLAR_STENCIL)
  gl.stencilOp(gl.KEEP, gl.KEEP, gl.KEEP)
  gl.uniform1f(bindings.opacity, opacity)
  gl.bindVertexArray(collarClip.vao)
  gl.drawElements(gl.TRIANGLES, collarClip.indexCount, gl.UNSIGNED_SHORT, 0)
  gl.stencilMask(0)
  gl.disable(gl.STENCIL_TEST)
}
