import assert from 'node:assert/strict'
import { afterEach, it, mock } from 'node:test'
import { ApiError, apiService } from '../../services/api'
import { turnKeyformsFor } from './rig/turnKeyformImport'
import { decomposeWithTurnKeyforms } from './turnKeyformsApi'

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
