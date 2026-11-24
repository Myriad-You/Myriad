import assert from 'node:assert/strict'
import { afterEach, it, mock } from 'node:test'
import { apiService } from '../../../services/api'
import { movingPartsSearching, stagedPartSearch } from './movingPartsSearch'

afterEach(() => mock.restoreAll())

it('a search running on the server is found for its own figure only', async () => {
  mock.method(apiService, 'get', async () => ({ searches: [{ outfitId: null, seconds: 12 }, { outfitId: 'w-1', seconds: 3 }] }))
  assert.equal(await movingPartsSearching(), true, 'the worn bust')
  assert.equal(await movingPartsSearching('w-1'), true, 'that full-body set')
  assert.equal(await movingPartsSearching('w-2'), false, 'another set')
  mock.method(apiService, 'get', async () => ({ searches: [] }))
  assert.equal(await movingPartsSearching(), false, 'nothing runs')
})

it('the search a preview began is read as looking, found or gone', async () => {
  const asked: string[] = []
  const answer = (data: unknown) => mock.method(apiService, 'get', async (path: string) => {
    asked.push(path)
    return data
  })
  const id = 'a'.repeat(64)
  answer({ state: 'looking' })
  assert.deepEqual(await stagedPartSearch(id), { state: 'looking' })
  assert.deepEqual(asked, [`/merope/rig/parts/staged/${id}`])
  const partProfile = {
    version: 1,
    source: 'ai-vision',
    parts: [{
      name: 'charm', layer: null, separate: false, box: [40, 60, 60, 90], anchor: [50, 60],
      bone: 'chest', motion: 'dangle', material: 'metal', stiffness: 0.7, weight: 0.8,
    }],
  }
  answer({ state: 'found', partProfile })
  assert.deepEqual(await stagedPartSearch(id), { state: 'found', partProfile })
  answer({ state: 'found', partProfile: { ...partProfile, parts: [] } })
  assert.deepEqual(await stagedPartSearch(id), { state: 'found', partProfile: { ...partProfile, parts: [] } }, 'none found is an answer too')
  answer({ state: 'found', partProfile: { parts: [{ name: 'charm' }] } })
  assert.deepEqual(await stagedPartSearch(id), { state: 'gone' }, 'a profile the page cannot list is not shown')
  answer({ state: 'gone' })
  assert.deepEqual(await stagedPartSearch(id), { state: 'gone' })
  answer({})
  assert.deepEqual(await stagedPartSearch(id), { state: 'gone' })
})
