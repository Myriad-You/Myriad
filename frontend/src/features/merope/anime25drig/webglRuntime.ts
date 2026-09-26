import type { Anime25DPlaybackLayer } from './types'
import { currentCopy } from '../../../i18n/localeCopy'
import { NECK_SURFACE_COLUMNS } from './neckSurfaceContour'

const VERTEX_SHADER = `#version 300 es
in vec2 a_pos;
in vec2 a_uv;
uniform vec2 u_view;
uniform mat3 u_layer_transform;
// Pivot on the canvas cut, lean angle, and the height over which the torso
// bends into it: the cut stays on the frame while the shoulders take it all.
uniform vec4 u_body_transform;
out vec2 v_uv;
void main() {
  vec2 layer_position = (u_layer_transform * vec3(a_pos, 1.0)).xy;
  vec2 offset = layer_position - u_body_transform.xy;
  float share = u_body_transform.w > 0.0
    ? smoothstep(0.0, 1.0, -offset.y / u_body_transform.w)
    : 1.0;
  float angle = u_body_transform.z * share;
  float c = cos(angle);
  float s = sin(angle);
  vec2 transformed = u_body_transform.xy + vec2(
    offset.x * c - offset.y * s,
    offset.x * s + offset.y * c
  );
  vec2 clip = vec2(transformed.x / u_view.x * 2.0 - 1.0, 1.0 - transformed.y / u_view.y * 2.0);
  gl_Position = vec4(clip, 0.0, 1.0);
  v_uv = a_uv;
}`

const FRAGMENT_SHADER = `#version 300 es
precision mediump float;
in vec2 v_uv;
uniform sampler2D u_texture;
uniform float u_cut;
uniform float u_opacity;
uniform float u_cry_time;
uniform float u_cry;
uniform vec4 u_atlas_rect;
uniform vec2 u_neck_surface_fade;
uniform vec2 u_crown_band;
uniform vec2 u_neck_surface_bounds;
uniform vec2 u_neck_surface_contour[${NECK_SURFACE_COLUMNS}];
// Eye whites paint their coverage into this canvas-sized mask (left eye in red,
// right in green); an iris multiplies by its own eye's channel.
uniform sampler2D u_eye_mask;
uniform vec2 u_eye_mask_channel;
uniform float u_eye_mask_pass;
out vec4 out_color;

vec2 atlas_uv(vec2 local_uv) {
  return u_atlas_rect.xy + local_uv * u_atlas_rect.zw;
}

float tear_water_mask(vec4 color, float y) {
  vec3 straight = color.rgb / max(color.a, 0.001);
  float blue_water = smoothstep(0.06, 0.18, straight.b - straight.r)
    * smoothstep(0.04, 0.16, straight.g - straight.r);
  float pale_highlight = smoothstep(0.72, 0.94, max(straight.r, straight.g))
    * smoothstep(-0.02, 0.08, straight.b - straight.r);
  return max(blue_water, pale_highlight)
    * smoothstep(0.25, 0.33, y)
    * smoothstep(0.01, 0.12, color.a);
}

float tear_center(float y, float side) {
  if (side < 0.0) {
    if (y < 0.39) return mix(0.23, 0.27, clamp((y - 0.29) / 0.10, 0.0, 1.0));
    if (y < 0.52) return mix(0.27, 0.24, (y - 0.39) / 0.13);
    if (y < 0.70) return mix(0.24, 0.30, (y - 0.52) / 0.18);
    if (y < 0.89) return mix(0.30, 0.27, (y - 0.70) / 0.19);
    return 0.27;
  }
  if (y < 0.40) return mix(0.77, 0.73, clamp((y - 0.30) / 0.10, 0.0, 1.0));
  if (y < 0.53) return mix(0.73, 0.76, (y - 0.40) / 0.13);
  if (y < 0.71) return mix(0.76, 0.70, (y - 0.53) / 0.18);
  if (y < 0.90) return mix(0.70, 0.73, (y - 0.71) / 0.19);
  return 0.73;
}

void main() {
  vec4 color = texture(u_texture, v_uv);
  vec2 local_uv = (v_uv - u_atlas_rect.xy) / u_atlas_rect.zw;
  float cry_amount = abs(u_cry);
  if (cry_amount > 0.001) {
    float side = u_cry < 0.0 ? -1.0 : 1.0;
    float root_y = side < 0.0 ? 0.29 : 0.30;
    float source_span = 0.70;
    float side_phase = side < 0.0 ? 0.0 : 0.055;
    float cycle = fract(u_cry_time * 0.62 + side_phase);
    float grow = smoothstep(0.02, 0.29, cycle)
      * (1.0 - smoothstep(0.34, 0.52, cycle));
    float recoil = smoothstep(0.34, 0.58, cycle)
      * (1.0 - smoothstep(0.78, 0.98, cycle));
    float stretch = 0.775 + grow * 0.075 - recoil * 0.035;
    float source_y = root_y + (local_uv.y - root_y) / stretch;
    float stream_progress = clamp(
      (local_uv.y - root_y) / (source_span * stretch),
      0.0,
      1.0
    );
    float tip_weight = smoothstep(0.62, 0.98, stream_progress);
    float width_scale = 1.0 + tip_weight * (0.07 + grow * 0.13);
    float source_center = tear_center(source_y, side);
    float source_x = source_center + (local_uv.x - source_center) / width_scale;
    vec4 attached_sample = texture(u_texture, atlas_uv(vec2(source_x, source_y)));
    float base_water = tear_water_mask(color, local_uv.y);
    float attached_water = tear_water_mask(attached_sample, source_y)
      * step(root_y, local_uv.y)
      * step(local_uv.y, root_y + source_span * stretch);

    float drop_progress = clamp((cycle - 0.27) / 0.62, 0.0, 1.0);
    float drop_visible = smoothstep(0.25, 0.34, cycle)
      * (1.0 - smoothstep(0.84, 0.98, cycle));
    float drop_source_y = side < 0.0 ? 0.89 : 0.90;
    float drop_source_x = tear_center(drop_source_y, side);
    float drop_center_x = drop_source_x
      + side * drop_progress * 0.012
      + sin(drop_progress * 3.14159265) * side * 0.004;
    float drop_center_y = 0.83
      + drop_progress * 0.10
      + drop_progress * drop_progress * 0.035;
    float drop_radius_x = mix(0.042, 0.031, drop_progress);
    float drop_radius_y = mix(0.052, 0.039, drop_progress);
    vec2 drop_source_uv = vec2(
      drop_source_x + (local_uv.x - drop_center_x) * (0.068 / drop_radius_x),
      drop_source_y + (local_uv.y - drop_center_y) * (0.072 / drop_radius_y)
    );
    vec4 drop_sample = texture(u_texture, atlas_uv(drop_source_uv));
    float drop_water = tear_water_mask(drop_sample, drop_source_uv.y)
      * step(0.805, drop_source_uv.y)
      * step(drop_source_uv.y, 0.955)
      * drop_visible;

    vec4 dry_eye = color * (1.0 - base_water);
    vec4 attached_tear = attached_sample * attached_water;
    vec4 falling_drop = drop_sample * drop_water;
    vec4 moving_water = attached_tear
      + falling_drop * (1.0 - attached_tear.a);
    color = dry_eye + moving_water * (1.0 - dry_eye.a);
  }
  if (u_eye_mask_pass > 0.5) {
    out_color = vec4(color.a);
    return;
  }
  if (color.a < u_cut) discard;
  float neck_opacity = 1.0;
  if (u_neck_surface_fade.y > u_neck_surface_fade.x) {
    vec2 band = u_neck_surface_fade;
    if (u_neck_surface_bounds.y > u_neck_surface_bounds.x) {
      float column = clamp((local_uv.x - u_neck_surface_bounds.x)
        / (u_neck_surface_bounds.y - u_neck_surface_bounds.x), 0.0, 1.0)
        * ${NECK_SURFACE_COLUMNS - 1}.0;
      int left = min(int(floor(column)), ${NECK_SURFACE_COLUMNS - 2});
      band = mix(u_neck_surface_contour[left], u_neck_surface_contour[left + 1], column - float(left));
    }
    neck_opacity -= smoothstep(band.x, band.y, local_uv.y);
  }
  float crown_opacity = u_crown_band.y > u_crown_band.x
    ? 1.0 - smoothstep(u_crown_band.x, u_crown_band.y, local_uv.y) : 1.0;
  float eye_mask = u_eye_mask_channel == vec2(0.0)
    ? 1.0
    : dot(texelFetch(u_eye_mask, ivec2(gl_FragCoord.xy), 0).rg, u_eye_mask_channel);
  out_color = color * (u_opacity * neck_opacity * crown_opacity * eye_mask);
}`

export interface CroppedLayerPixels {
  pixels: Uint8ClampedArray
  width: number
  height: number
}

export interface AtlasPixelPatch extends CroppedLayerPixels {
  x: number
  y: number
}

export interface IndexedDeformableMesh {
  vao: WebGLVertexArrayObject
  positionBuffer: WebGLBuffer
  uvBuffer: WebGLBuffer
  indexBuffer: WebGLBuffer
}

export function createIndexedDeformableMesh(
  gl: WebGL2RenderingContext,
  program: WebGLProgram,
  positions: Float32Array,
  uvs: Float32Array,
  indices: Uint16Array,
): IndexedDeformableMesh {
  if (positions.length !== uvs.length) {
    throw new Error(currentCopy().merope.anime25dPlaybackFailed)
  }
  const vao = gl.createVertexArray()
  const positionBuffer = gl.createBuffer()
  const uvBuffer = gl.createBuffer()
  const indexBuffer = gl.createBuffer()
  if (!vao || !positionBuffer || !uvBuffer || !indexBuffer) {
    if (positionBuffer) gl.deleteBuffer(positionBuffer)
    if (uvBuffer) gl.deleteBuffer(uvBuffer)
    if (indexBuffer) gl.deleteBuffer(indexBuffer)
    if (vao) gl.deleteVertexArray(vao)
    throw new Error(currentCopy().merope.anime25dPlaybackFailed)
  }
  const mesh = { vao, positionBuffer, uvBuffer, indexBuffer }
  try {
    const position = gl.getAttribLocation(program, 'a_pos')
    const uv = gl.getAttribLocation(program, 'a_uv')
    gl.bindVertexArray(vao)
    gl.bindBuffer(gl.ARRAY_BUFFER, positionBuffer)
    gl.bufferData(gl.ARRAY_BUFFER, positions, gl.DYNAMIC_DRAW)
    gl.enableVertexAttribArray(position)
    gl.vertexAttribPointer(position, 2, gl.FLOAT, false, 0, 0)
    gl.bindBuffer(gl.ARRAY_BUFFER, uvBuffer)
    gl.bufferData(gl.ARRAY_BUFFER, uvs, gl.STATIC_DRAW)
    gl.enableVertexAttribArray(uv)
    gl.vertexAttribPointer(uv, 2, gl.FLOAT, false, 0, 0)
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, indexBuffer)
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, indices, gl.STATIC_DRAW)
    gl.bindVertexArray(null)
    return mesh
  } catch (error) {
    gl.bindVertexArray(null)
    disposeIndexedDeformableMesh(gl, mesh)
    throw error
  }
}

export function disposeIndexedDeformableMesh(
  gl: WebGL2RenderingContext,
  mesh: Readonly<{
    vao: WebGLVertexArrayObject | null
    positionBuffer: WebGLBuffer | null
    uvBuffer: WebGLBuffer | null
    indexBuffer: WebGLBuffer | null
  }>,
): void {
  if (mesh.positionBuffer) gl.deleteBuffer(mesh.positionBuffer)
  if (mesh.uvBuffer) gl.deleteBuffer(mesh.uvBuffer)
  if (mesh.indexBuffer) gl.deleteBuffer(mesh.indexBuffer)
  if (mesh.vao) gl.deleteVertexArray(mesh.vao)
}

export function readLayerPixels(
  atlas: HTMLImageElement,
  source: Anime25DPlaybackLayer,
): CroppedLayerPixels | null {
  const sx = Math.max(0, Math.round(source.atlas.x * atlas.width))
  const sy = Math.max(0, Math.round(source.atlas.y * atlas.height))
  const sw = Math.max(1, Math.round(source.atlas.w * atlas.width))
  const sh = Math.max(1, Math.round(source.atlas.h * atlas.height))
  const crop = document.createElement('canvas')
  crop.width = sw
  crop.height = sh
  const context = crop.getContext('2d')
  if (!context) throw new Error(currentCopy().merope.anime25dPlaybackFailed)
  context.drawImage(atlas, sx, sy, sw, sh, 0, 0, sw, sh)
  try {
    return {
      pixels: context.getImageData(0, 0, sw, sh).data,
      width: sw,
      height: sh,
    }
  } catch {
    return null
  } finally {
    crop.width = 0
    crop.height = 0
  }
}

/** Packed atlas gutter in pixels; must match ATLAS_PADDING in anime25dAtlasCompiler. */
export const ANIME25D_ATLAS_MIP_GUTTER_PX = 8

/**
 * Highest mipmap level that still leaves two gutter texels between sprites.
 * Trilinear filtering also samples the next level, so level 3 (8 px texels)
 * would mix neighbouring packed drawings across an 8 px gutter.
 */
export function anime25DAtlasMaxMipLevel(gutterPx: number): number {
  if (!Number.isFinite(gutterPx) || gutterPx < 2) return 0
  return Math.max(0, Math.floor(Math.log2(gutterPx)) - 1)
}

/**
 * Sample half a lod finer than the generated cap so small faces stay a bit
 * sharper. Level 2 is still generated; we just do not sit fully on it.
 */
export function anime25DAtlasMaxLod(gutterPx: number): number {
  const maxLevel = anime25DAtlasMaxMipLevel(gutterPx)
  return maxLevel > 0 ? maxLevel - 0.5 : 0
}

export function createAtlasTexture(
  gl: WebGL2RenderingContext,
  atlas: HTMLImageElement,
  patches: readonly AtlasPixelPatch[] = [],
): WebGLTexture {
  const texture = gl.createTexture()
  if (!texture) throw new Error(currentCopy().merope.anime25dPlaybackFailed)
  try {
    gl.bindTexture(gl.TEXTURE_2D, texture)
    gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, 1)
    const maxLevel = anime25DAtlasMaxMipLevel(ANIME25D_ATLAS_MIP_GUTTER_PX)
    const maxLod = anime25DAtlasMaxLod(ANIME25D_ATLAS_MIP_GUTTER_PX)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE)
    if (maxLevel > 0) {
      gl.texParameteri(
        gl.TEXTURE_2D,
        gl.TEXTURE_MIN_FILTER,
        gl.LINEAR_MIPMAP_LINEAR,
      )
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAX_LEVEL, maxLevel)
      gl.texParameterf(gl.TEXTURE_2D, gl.TEXTURE_MAX_LOD, maxLod)
    } else {
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR)
    }
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, atlas)
    // Typed-array uploads are premultiplied explicitly, unlike DOM sources.
    if (patches.length) gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, 0)
    for (const patch of patches) {
      const pixels = new Uint8Array(patch.pixels.length)
      for (let i = 0; i < pixels.length; i += 4) {
        const alpha = patch.pixels[i + 3]
        for (let c = 0; c < 3; c++)
          pixels[i + c] = Math.round((patch.pixels[i + c] * alpha) / 255)
        pixels[i + 3] = alpha
      }
      gl.texSubImage2D(
        gl.TEXTURE_2D,
        0,
        patch.x,
        patch.y,
        patch.width,
        patch.height,
        gl.RGBA,
        gl.UNSIGNED_BYTE,
        pixels,
      )
    }
    if (patches.length) gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, 1)
    // After patches so accessory/neckwear edits are in the chain. MAX_LEVEL
    // limits generateMipmap to the gutter-safe levels; not a per-frame cost.
    if (maxLevel > 0) gl.generateMipmap(gl.TEXTURE_2D)
    return texture
  } catch (error) {
    gl.deleteTexture(texture)
    throw error
  }
}

export function compileProgram(gl: WebGL2RenderingContext): WebGLProgram {
  const vertex = compileShader(gl, gl.VERTEX_SHADER, VERTEX_SHADER)
  let fragment: WebGLShader
  try {
    fragment = compileShader(gl, gl.FRAGMENT_SHADER, FRAGMENT_SHADER)
  } catch (error) {
    gl.deleteShader(vertex)
    throw error
  }
  const program = gl.createProgram()
  if (!program) {
    gl.deleteShader(vertex)
    gl.deleteShader(fragment)
    throw new Error(currentCopy().merope.anime25dPlaybackFailed)
  }
  try {
    gl.attachShader(program, vertex)
    gl.attachShader(program, fragment)
    gl.linkProgram(program)
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
      throw new Error(currentCopy().merope.anime25dPlaybackFailed)
    }
    return program
  } catch (error) {
    gl.deleteProgram(program)
    throw error
  } finally {
    gl.deleteShader(vertex)
    gl.deleteShader(fragment)
  }
}

function compileShader(
  gl: WebGL2RenderingContext,
  type: number,
  source: string,
): WebGLShader {
  const shader = gl.createShader(type)
  if (!shader) throw new Error(currentCopy().merope.anime25dPlaybackFailed)
  gl.shaderSource(shader, source)
  gl.compileShader(shader)
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    gl.deleteShader(shader)
    throw new Error(currentCopy().merope.anime25dPlaybackFailed)
  }
  return shader
}

export function requiredUniform(
  gl: WebGL2RenderingContext,
  program: WebGLProgram,
  name: string,
): WebGLUniformLocation {
  const location = gl.getUniformLocation(program, name)
  if (!location) throw new Error(currentCopy().merope.anime25dPlaybackFailed)
  return location
}

export function atlasUrlNeedsCors(
  url: string,
  pageHref = typeof window !== 'undefined' && window.location?.href
    ? window.location.href
    : '',
): boolean {
  if (!pageHref) return false
  try {
    return new URL(url, pageHref).origin !== new URL(pageHref).origin
  } catch {
    return false
  }
}

const MAX_CACHED_ATLAS_IMAGES = 1
const cachedAtlasImages = new Map<string, HTMLImageElement>()

function cachedAtlasImage(url: string): HTMLImageElement | undefined {
  const image = cachedAtlasImages.get(url)
  if (image?.complete && image.naturalWidth > 0) return image
  if (image) cachedAtlasImages.delete(url)
  return undefined
}

function storeAtlasImage(url: string, image: HTMLImageElement): void {
  cachedAtlasImages.delete(url)
  cachedAtlasImages.set(url, image)
  while (cachedAtlasImages.size > MAX_CACHED_ATLAS_IMAGES) {
    const oldest = cachedAtlasImages.keys().next().value
    if (!oldest) break
    const evicted = cachedAtlasImages.get(oldest)
    cachedAtlasImages.delete(oldest)
    if (evicted && evicted !== image) evicted.src = ''
  }
}

export function resetCachedAtlasImagesForTests(): void {
  cachedAtlasImages.clear()
}

export function loadImage(
  url: string,
  signal?: AbortSignal,
): Promise<HTMLImageElement> {
  const cached = cachedAtlasImage(url)
  if (cached) return Promise.resolve(cached)
  return new Promise((resolve, reject) => {
    const image = new Image()
    let settled = false
    const cleanup = () => signal?.removeEventListener('abort', abort)
    const finish = (result: () => void) => {
      if (settled) return
      settled = true
      cleanup()
      image.onload = null
      image.onerror = null
      result()
    }
    const abort = () => {
      finish(() => {
        image.src = ''
        const error = new Error('Anime2.5DRig atlas load aborted')
        error.name = 'AbortError'
        reject(error)
      })
    }
    if (atlasUrlNeedsCors(url)) image.crossOrigin = 'anonymous'
    image.onload = () =>
      finish(() => {
        storeAtlasImage(url, image)
        resolve(image)
      })
    image.onerror = () =>
      finish(() => reject(new Error('Anime2.5DRig atlas failed to load')))
    if (signal?.aborted) {
      abort()
      return
    }
    signal?.addEventListener('abort', abort, { once: true })
    image.src = url
  })
}
