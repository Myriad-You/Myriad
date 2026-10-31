import assert from 'node:assert/strict'
import { afterEach, it, mock } from 'node:test'
import { turnKeyformsFor } from '@myriad/figure/testing'
import { ApiError, apiService } from '../../services/api'
import { decomposeWithTurnKeyforms, resumeTurnKeyforms, uploadTurnArchive } from './turnKeyformsApi'

afterEach(() => mock.restoreAll())

const keys = {
  canvas: [100, 100],
  keyforms: { face: { plus: { box: [0, 0, 10, 10], grid: 2, back: [0, 0, 0, 0, 0, 0, 0, 0] }, minus: { box: [0, 0, 10, 10], grid: 2, back: [0, 0, 0, 0, 0, 0, 0, 0] } } },
}

/** Runs the job with polling waits made instant. */
async function run(get: (path: string) => unknown) {
  mock.timers.enable({ apis: ['setTimeout'] })
  let started = 0
  mock.method(apiService, 'post', async () => ({ jobId: `job${++started}` }))
  mock.method(apiService, 'get', async (path: string) => get(path))
  const pending = decomposeWithTurnKeyforms({ sourceMasterAssetId: '/media/assets/a/b.png' })
  const state = { settled: false }
  void pending.finally(() => { state.settled = true }).catch(() => {})
  while (!state.settled) {
    await new Promise((resolve) => setImmediate(resolve))
    mock.timers.tick(5000)
  }
  mock.timers.reset()
  return { file: await pending, started: () => started }
}

it('starts again when the server lost the job, and keeps polling through a restart', async () => {
  const polls: string[] = []
  const { file, started } = await run((path) => {
    polls.push(path)
    if (path.endsWith('/psd')) return new Blob([new Uint8Array([1])])
    if (path.endsWith('/keyforms')) return keys
    if (path.endsWith('job1')) {
      // Generating, then the server goes away for a moment, then has forgotten the job.
      if (polls.length === 1) return { stage: 'generating', done: 1, total: 4 }
      if (polls.length === 2) throw new ApiError('Bad Gateway', 502)
      throw new ApiError('No such turn keys job', 404)
    }
    return { stage: 'done', done: 1, total: 1 }
  })
  assert.equal(started(), 2)
  assert.ok(file instanceof File)
  assert.ok(turnKeyformsFor(file))
})

it('a refusal is not retried', async () => {
  await assert.rejects(run((path) => {
    if (path.endsWith('job1')) throw new ApiError('Forbidden', 403)
    return { stage: 'done' }
  }))
})

it('a reloaded page picks up the job running for its portrait, without starting another', async () => {
  const running = { jobId: 'run1', stage: 'decomposing', done: 2, total: 5, seconds: 700, sourceMasterAssetId: '/media/assets/a/b.png', outfitId: null }
  mock.timers.enable({ apis: ['setTimeout'] })
  const post = mock.method(apiService, 'post', async () => ({ jobId: 'fresh' }))
  const polled: string[] = []
  mock.method(apiService, 'get', async (path: string) => {
    if (path.endsWith('/see-through/turn-keyforms')) return { job: running }
    polled.push(path)
    if (path.endsWith('/psd')) return new Blob([new Uint8Array([1])])
    if (path.endsWith('/keyforms')) return keys
    return { stage: 'done', done: 5, total: 5 }
  })
  const seen: string[] = []
  const pending = resumeTurnKeyforms({ sourceMasterAssetId: '/media/assets/a/b.png' }, { onStatus: (status) => seen.push(status.stage) })
  const state = { settled: false }
  void pending.finally(() => { state.settled = true }).catch(() => {})
  while (!state.settled) {
    await new Promise((resolve) => setImmediate(resolve))
    mock.timers.tick(5000)
  }
  mock.timers.reset()
  assert.ok(await pending)
  assert.equal(post.mock.callCount(), 0, 'nothing new started')
  assert.ok(polled[0].endsWith('/turn-keyforms/run1'))
  assert.equal(seen[0], 'decomposing', 'its progress shows at once')
})

it('a job for another portrait or another set is not picked up', async () => {
  const job = { jobId: 'run1', stage: 'decomposing', done: 2, total: 5, seconds: 700, sourceMasterAssetId: '/media/assets/a/b.png', outfitId: 'w-1' }
  mock.method(apiService, 'get', async () => ({ job }))
  const post = mock.method(apiService, 'post', async () => ({ jobId: 'fresh' }))
  assert.equal(await resumeTurnKeyforms({ sourceMasterAssetId: '/media/assets/a/b.png' }), null, 'the bust is not the full-body set')
  assert.equal(await resumeTurnKeyforms({ sourceMasterAssetId: '/media/assets/other.png' }, { outfitId: 'w-1' }), null)
  assert.equal(post.mock.callCount(), 0)
  mock.method(apiService, 'get', async () => ({ job: null }))
  assert.equal(await resumeTurnKeyforms({ sourceMasterAssetId: '/media/assets/a/b.png' }), null, 'nothing runs')
})

it('an uploaded zip goes in pieces, in order, then is kept for the set it was chosen in', async () => {
  const pieces: Array<{ path: string; size: number }> = []
  mock.method(apiService, 'put', async (path: string, piece: Blob) => {
    pieces.push({ path, size: piece.size })
    return { received: 0 }
  })
  const post = mock.method(apiService, 'post', async () => ({ id: 'kept1' }))
  const zip = new Blob([new Uint8Array(20 * 1024 * 1024)])
  const sent: number[] = []
  const id = await uploadTurnArchive(zip, 'w-1', (done) => sent.push(done))
  assert.equal(id, 'kept1')
  assert.deepEqual(pieces.map(({ path }) => path.split('?')[1]), ['offset=0', 'offset=8388608', 'offset=16777216'])
  assert.deepEqual(pieces.map(({ size }) => size), [8388608, 8388608, 4194304])
  assert.deepEqual(sent, [8388608, 16777216, 20971520])
  const upload = pieces[0].path.split('?')[0].split('/').pop()
  assert.match(upload ?? '', /^[0-9a-f]{32}$/)
  assert.ok(pieces.every(({ path }) => path.includes(`/see-through/turn-archive-uploads/${upload}?`)))
  const finish = post.mock.calls[0].arguments[0] as string
  assert.ok(finish.endsWith(`/full-body/w-1/see-through/turn-archive-uploads/${upload}`), finish)
})
