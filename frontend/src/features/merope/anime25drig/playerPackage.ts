import type { MeropeRigManifest } from '../rig/types'
import type { CollarClipMesh } from './collarRuntime'
import type { Anime25DDriver } from './driver'
import type { Anime25DCompiledGpuLayers, Anime25DGpuLayer } from './layerGpuBinding'
import type { TouchAtlas, TouchPaintLayer } from './touchVisibility'
import type { Anime25DPlayback } from './types'
import { resolveAnime25DLayerSemantics } from '../rig/anime25dLayerSemantics'
import { buildChestWeightField, chestProfileUsesGeometryWeights } from './chestPhysics'
import { disposeCollarClipMesh } from './collarRuntime'
import { ContinuousMouthTexture, prepareContinuousMouth } from './continuousMouthTexture'
import { compileAnime25DGpuLayers } from './layerGpuBinding'
import { readTouchAtlas } from './touchVisibility'
import { createAtlasTexture, readLayerPixels } from './webglRuntime'

/** A package compiled for the GPU, not yet the one on screen. */
export interface PreparedLivePackage {
  resolved: Anime25DPlayback
  compiled: Anime25DCompiledGpuLayers
  texture: WebGLTexture | null
  touchAtlas: TouchAtlas | null
  /** The eyelash's line art, which the thinking sticker draws in. */
  linePixels: Uint8ClampedArray | undefined
}

export function releaseCompiledGpu(
  gl: WebGL2RenderingContext,
  layers: readonly Anime25DGpuLayer[],
  collarClip: CollarClipMesh | null,
  texture: WebGLTexture | null,
): void {
  for (const layer of layers) {
    if (layer.vertexBuffer) gl.deleteBuffer(layer.vertexBuffer)
    if (layer.uvBuffer) gl.deleteBuffer(layer.uvBuffer)
    if (layer.indexBuffer) gl.deleteBuffer(layer.indexBuffer)
    if (layer.vao) gl.deleteVertexArray(layer.vao)
  }
  if (collarClip) disposeCollarClipMesh(gl, collarClip)
  if (texture) gl.deleteTexture(texture)
}

/**
 * Compile a package's layers against its atlas and read what touch and the
 * sticker need from it. A failure part-way releases what was already made.
 */
export function prepareLivePackage(
  gl: WebGL2RenderingContext,
  program: WebGLProgram,
  playback: Anime25DPlayback,
  rigManifest: MeropeRigManifest | undefined,
  image: HTMLImageElement,
  current: Anime25DDriver,
): PreparedLivePackage {
  const resolved: Anime25DPlayback = {
    ...playback,
    layers: playback.layers.map(resolveAnime25DLayerSemantics),
  }
  const chestWeightField = chestProfileUsesGeometryWeights(
    resolved.chestProfile,
  )
    ? buildChestWeightField(rigManifest)
    : null
  const compiled = compileAnime25DGpuLayers(
    gl,
    program,
    resolved,
    resolved.shellProfile,
    current,
    chestWeightField,
    image,
  )
  let texture: WebGLTexture | null = null
  try {
    texture = createAtlasTexture(gl, image, compiled.atlasPatches)
    const touchAtlas = readTouchAtlas(image)
    const eyelash = resolved.layers.find((layer) => layer.role === 'eyelash')
    const linePixels = eyelash ? readLayerPixels(image, eyelash)?.pixels : undefined
    return { resolved, compiled, texture, touchAtlas, linePixels }
  } catch (error) {
    // Keep the live outfit; the not-yet-owned replacement must be released.
    releaseCompiledGpu(gl, compiled.layers, compiled.collarClip, texture)
    throw error
  }
}

/** The speaking mouth drawn live, when the portrait's speaking mouths are the importer's. */
export function createContinuousMouth(
  gl: WebGL2RenderingContext,
  playback: Readonly<Anime25DPlayback>,
  atlas: HTMLImageElement,
  layers: readonly Anime25DGpuLayer[],
): ContinuousMouthTexture | null {
  const setup = prepareContinuousMouth(
    playback.layers,
    playback.anchors,
    atlas.naturalWidth || atlas.width,
    atlas.naturalHeight || atlas.height,
    (layer) => readLayerPixels(atlas, layer),
  )
  const layer = setup && layers.find((candidate) => candidate.source === setup.layer)
  if (!setup || !layer) return null
  const mouth = new ContinuousMouthTexture(gl, setup)
  layer.ownTexture = mouth.own
  return mouth
}

/** What touch hit-testing reads of each layer: its paint and its live mesh. */
export function touchLayersFor(
  layers: readonly Anime25DGpuLayer[],
  collarClip: CollarClipMesh | null,
): TouchPaintLayer[] {
  return layers.map((layer) => {
    const mesh =
      layer.renderKind === 'neck' && collarClip ? collarClip : layer
    return {
      paint: layer,
      mesh: {
        positions: mesh.deformed,
        atlasUvs: mesh.atlasUvs,
        indices: mesh.indices,
        layerTransform: layer.layerTransform,
      },
    }
  })
}
