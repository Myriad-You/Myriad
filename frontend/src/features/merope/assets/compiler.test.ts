import type { MeropeRigManifest } from '../rig/types'
import type { RigAssetCompileEvent } from './compiler'
import assert from 'node:assert/strict'
import test from 'node:test'
import { compileRigAsset, persistRigAsset, preflightRigAsset } from './compiler'

const file = new File([new Uint8Array([1])], 'character.psd')
const manifest = {
  schemaVersion: 1,
  quality: 'layered-2d',
  canvas: { width: 1, height: 1 },
  textures: [],
  bones: [{ id: 'root', parent: null, pivot: { x: 0.5, y: 0.9 } }],
  parts: [],
} as MeropeRigManifest

test('rig compiler exposes the complete successful artifact DAG', async () => {
  const events: RigAssetCompileEvent[] = []
  const result = await compileRigAsset(
    file,
    'master-asset',
    {
      prepare: async (_file, _assetId, onStage) => {
        onStage?.('validated')
        onStage?.('packing')
        return {
          atlas: new Blob(),
          analysisReference: new Blob(),
          source: {} as never,
          partCount: 12,
        }
      },
      preview: async () => manifest,
      upload: async () => manifest,
    },
    (event) => events.push(event),
  )
  assert.equal(result.manifest, manifest)
  assert.equal(result.partCount, 12)
  assert.equal(typeof result.report.score, 'number')
  assert.deepEqual(
    events.map(({ stage, status }) => `${stage}:${status}`),
    [
      'validate-source:started',
      'validate-source:completed',
      'pack-atlas:started',
      'pack-atlas:completed',
      'compile-preview:started',
      'compile-preview:completed',
      'analyze-capabilities:started',
      'analyze-capabilities:completed',
      'persist-manifest:started',
      'persist-manifest:completed',
    ],
  )
})

test('rig compiler marks persistence failure as terminal', async () => {
  const events: RigAssetCompileEvent[] = []
  await assert.rejects(
    compileRigAsset(
      file,
      'master-asset',
      {
        prepare: async () => ({
          atlas: new Blob(),
          analysisReference: new Blob(),
          source: {} as never,
          partCount: 2,
        }),
        preview: async () => manifest,
        upload: async () => {
          throw new Error('storage unavailable')
        },
      },
      (event) => events.push(event),
    ),
    /storage unavailable/,
  )
  assert.deepEqual(events.at(-1), {
    stage: 'persist-manifest',
    status: 'failed',
    error: 'storage unavailable',
  })
})

test('preflight fully compiles and diagnoses without calling persistence', async () => {
  let persisted = false
  const prepared = {
    atlas: new Blob(),
    analysisReference: new Blob(),
    source: { sourceMasterAssetId: 'master-asset' } as never,
    partCount: 45,
  }
  const result = await preflightRigAsset(file, 'master-asset', {
    prepare: async () => prepared,
    preview: async () => manifest,
  })
  assert.equal(result.prepared, prepared)
  assert.equal(result.partCount, 45)
  assert.equal(persisted, false)
  await persistRigAsset(result, async () => {
    persisted = true
    return manifest
  })
  assert.equal(persisted, true)
})

test('preflight sends the imported PSD composition to one-shot vision analysis', async () => {
  const analysisReference = new Blob([new Uint8Array([7])], {
    type: 'image/png',
  })
  let receivedReference: Blob | undefined
  await preflightRigAsset(file, 'master-asset', {
    prepare: async () => ({
      atlas: new Blob(),
      analysisReference,
      source: { sourceMasterAssetId: 'master-asset' } as never,
      partCount: 1,
    }),
    preview: async (_source, _atlas, reference) => {
      receivedReference = reference
      return manifest
    },
  })

  assert.equal(receivedReference, analysisReference)
})

test('preflight carries one-shot chest analysis into the persisted source', async () => {
  const source = {
    anime25dPlayback: {
      kind: 'anime-2.5d-rig',
      version: 1,
      engine: 'Anime2.5DRig',
      engineUrl: 'https://github.com/852wa/Anime2.5DRig',
      license: 'MIT',
      copyright: 'Copyright (c) 2026 hakoniwa',
      pixelCanvas: { width: 100, height: 120 },
      layers: [],
      anchors: {},
    },
  }
  const analyzed = structuredClone(manifest)
  analyzed.anime25dPlayback = {
    ...source.anime25dPlayback,
    chestProfile: {
      version: 1,
      enabled: true,
      source: 'ai-vision',
      centerX: 50,
      centerY: 75,
      radiusX: 20,
      radiusY: 18,
      visibleScale: 0.7,
      motionScale: 1.05,
      frequencyScale: 0.96,
      confidence: 0.9,
    },
  } as never
  const result = await preflightRigAsset(file, 'master-asset', {
    prepare: async () => ({
      atlas: new Blob(),
      analysisReference: new Blob(),
      source: source as never,
      partCount: 1,
    }),
    preview: async () => analyzed,
  })
  assert.deepEqual(
    result.prepared.source.anime25dPlayback?.chestProfile,
    analyzed.anime25dPlayback.chestProfile,
  )
})

test('failed preview cannot reach persistence', async () => {
  let persisted = false
  await assert.rejects(
    compileRigAsset(file, 'master-asset', {
      prepare: async () => ({
        atlas: new Blob(),
        analysisReference: new Blob(),
        source: {} as never,
        partCount: 45,
      }),
      preview: async () => {
        throw new Error('semantic chain rejected')
      },
      upload: async () => {
        persisted = true
        return manifest
      },
    }),
    /semantic chain rejected/,
  )
  assert.equal(persisted, false)
})
