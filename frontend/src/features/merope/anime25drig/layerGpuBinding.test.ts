import type { CollarClipMesh } from './collarRuntime'
import type {
  Anime25DCompiledGpuLayers,
  Anime25DGpuLayer,
} from './layerGpuBinding'
import assert from 'node:assert/strict'
import test from 'node:test'
import { disposeAnime25DGpuLayers } from './layerGpuBinding'

test('releases owned layer meshes and the collar clip exactly once', () => {
  const deletedBuffers: string[] = []
  const deletedVaos: string[] = []
  const gl = {
    deleteBuffer: (buffer: string) => deletedBuffers.push(buffer),
    deleteVertexArray: (vao: string) => deletedVaos.push(vao),
  } as unknown as WebGL2RenderingContext
  const compiled = {
    layers: [
      {
        vao: 'layer-vao',
        vertexBuffer: 'layer-position',
        uvBuffer: 'layer-uv',
        indexBuffer: 'layer-index',
      } as unknown as Anime25DGpuLayer,
      {
        vao: null,
        vertexBuffer: null,
        uvBuffer: null,
        indexBuffer: null,
      } as unknown as Anime25DGpuLayer,
      {
        vao: null,
        vertexBuffer: 'partial-position',
        uvBuffer: null,
        indexBuffer: null,
      } as unknown as Anime25DGpuLayer,
    ],
    collarClip: {
      vao: 'clip-vao',
      vertexBuffer: 'clip-position',
      uvBuffer: 'clip-uv',
      indexBuffer: 'clip-index',
    } as unknown as CollarClipMesh,
  } satisfies Anime25DCompiledGpuLayers

  disposeAnime25DGpuLayers(gl, compiled)

  assert.deepEqual(deletedBuffers, [
    'layer-position',
    'layer-uv',
    'layer-index',
    'partial-position',
    'clip-position',
    'clip-uv',
    'clip-index',
  ])
  assert.deepEqual(deletedVaos, ['layer-vao', 'clip-vao'])
})
