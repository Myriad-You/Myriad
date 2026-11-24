import type { MeropeRigImportSource, MeropeRigManifest } from '@myriad/figure/rig'
import assert from 'node:assert/strict'
import { afterEach, it, mock } from 'node:test'
import { currentCopy } from '../../i18n/localeCopy'
import { apiService } from '../../services/api'
import { fullBodyRigImport, importMeropeRig, previewMeropeRigImport } from './api'

afterEach(() => mock.restoreAll())

function vertex(x: number, y: number) {
  return { position: { x, y }, uv: { x, y }, joints: [0, 0, 0, 0], weights: [1, 0, 0, 0] }
}

const manifest = {
  schemaVersion: 1,
  quality: 'layered-2d',
  canvas: { width: 1, height: 1 },
  textures: [{ id: 'atlas', url: '/atlas.png', width: 512, height: 512 }],
  bones: [{ id: 'root', parent: null, pivot: { x: 0.5, y: 0.8 } }],
  parts: [{
    id: 'face',
    textureId: 'atlas',
    zIndex: 1,
    opacity: 1,
    vertices: [vertex(0, 0), vertex(1, 0), vertex(0, 1)],
    indices: [0, 1, 2],
  }],
} as MeropeRigManifest

const source = { sourceMasterAssetId: 'master' } as MeropeRigImportSource
const atlas = new Blob([new Uint8Array([1])], { type: 'image/png' })
const reference = new Blob([new Uint8Array([2])], { type: 'image/png' })

/** The fields each request carried, by the path it went to. */
function sentFields(post: { mock: { calls: { arguments: unknown[] }[] } }): Record<string, string[]> {
  return Object.fromEntries(post.mock.calls.map(({ arguments: [path, body] }) => {
    assert.ok(body instanceof FormData)
    return [path, [...body.keys()]]
  }))
}

it('a preview leaves the atlas behind and the import brings it, bust and full body alike', async () => {
  const post = mock.method(apiService, 'post', async () => ({ manifest, partSearch: null }))
  const fullBody = fullBodyRigImport('w1')
  await previewMeropeRigImport(source, reference)
  await fullBody.preview(source, reference)
  assert.deepEqual(await importMeropeRig(source, atlas, reference), manifest)
  assert.deepEqual(await fullBody.commit(source, atlas, reference), manifest)
  assert.deepEqual(sentFields(post), {
    '/merope/rig/import/preview': ['source', 'analysisReference'],
    '/merope/rig/full-body/w1/import/preview': ['source', 'analysisReference'],
    '/merope/rig/import': ['source', 'atlas', 'analysisReference'],
    '/merope/rig/full-body/w1/import': ['source', 'atlas', 'analysisReference'],
  })
  // The atlas the import sends is the one it was handed, as a PNG file.
  const imported = post.mock.calls[2].arguments[1]
  assert.ok(imported instanceof FormData)
  const sent = imported.get('atlas')
  assert.ok(sent instanceof File)
  assert.deepEqual([sent.name, sent.type, sent.size], ['rig-atlas.png', 'image/png', atlas.size])
})

it('a request that fails without saying why is named for what it was doing', async () => {
  const silent: unknown = { status: 0 }
  mock.method(apiService, 'post', async () => {
    throw silent
  })
  const { rigImportFailed, rigCommitFailed } = currentCopy().merope
  assert.notEqual(rigImportFailed, rigCommitFailed)
  const fullBody = fullBodyRigImport('w1')
  await assert.rejects(previewMeropeRigImport(source, reference), { message: rigImportFailed })
  await assert.rejects(fullBody.preview(source, reference), { message: rigImportFailed })
  await assert.rejects(importMeropeRig(source, atlas, reference), { message: rigCommitFailed })
  await assert.rejects(fullBody.commit(source, atlas, reference), { message: rigCommitFailed })
})
