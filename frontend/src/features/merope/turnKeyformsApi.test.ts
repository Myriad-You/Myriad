import type { TurnKeyformsAsked } from './turnKeyformsApi'
import assert from 'node:assert/strict'
import { afterEach, it, mock } from 'node:test'
import { turnKeyformsFor } from '@myriad/figure/testing'
import { ApiError, apiService } from '../../services/api'
import { MeropeApiError } from './api'
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

/** Waits out a promise with the polling waits made instant. */
async function settled<T>(pending: Promise<T>): Promise<T> {
  const state = { settled: false }
  void pending.finally(() => { state.settled = true }).catch(() => {})
  while (!state.settled) {
    await new Promise((resolve) => setImmediate(resolve))
    mock.timers.tick(5000)
  }
  mock.timers.reset()
  return pending
}

/**
 * Picks up `job` as a reloaded page does, the server answering `poll` when asked for it by its id.
 * Returns what was posted (path and body) and how the pick-up ended.
 */
async function pickUp(
  job: Record<string, unknown>,
  poll: (path: string, polls: number) => unknown,
  outfitId?: string,
) {
  mock.timers.enable({ apis: ['setTimeout'] })
  const posted: Array<{ path: string; body: unknown }> = []
  mock.method(apiService, 'post', async (path: string, body: unknown) => {
    posted.push({ path, body })
    return { jobId: `again${posted.length}` }
  })
  let polls = 0
  mock.method(apiService, 'get', async (path: string) => {
    if (path.endsWith('/see-through/turn-keyforms')) return { job }
    if (path.endsWith('/psd')) return new Blob([new Uint8Array([1])])
    if (path.endsWith('/keyforms')) return keys
    return poll(path, ++polls)
  })
  const seen: string[] = []
  const pending = resumeTurnKeyforms(
    { sourceMasterAssetId: '/media/assets/a/b.png' },
    { outfitId, onStatus: (status) => seen.push(status.stage) },
  )
  const outcome: { file?: File | null, reason?: unknown } = await settled(pending).then((file) => ({ file }), (reason: unknown) => ({ reason }))
  return { posted, seen, polls: () => polls, ...outcome }
}

function asked(patch: Partial<TurnKeyformsAsked>): TurnKeyformsAsked {
  return { head: true, body: false, plain: false, fromArchive: null, expressions: null, arms: null, ...patch }
}

const pickedUp = { jobId: 'run1', seconds: 700, sourceMasterAssetId: '/media/assets/a/b.png', outfitId: null }

it('a job done while no page was there is picked up and its result fetched, nothing started', async () => {
  const done = { ...pickedUp, stage: 'done', done: 1, total: 1, asked: asked({}) }
  const fetched: string[] = []
  const { posted, seen, file } = await pickUp(done, (path) => {
    fetched.push(path)
    return done
  })
  assert.ok(file instanceof File)
  assert.ok(turnKeyformsFor(file))
  assert.deepEqual(posted, [], 'nothing started')
  assert.deepEqual(seen, ['done', 'done'], 'done as found, and as read by its id')
  assert.ok(fetched[0].endsWith('/turn-keyforms/run1'), 'asked for by its id: only then does the server count it fetched')
})

it('a job that failed while no page was there is picked up to say why', async () => {
  const failed = { ...pickedUp, stage: 'failed', done: 1, total: 4, error: { error: 'timed out', code: 'see_through_timeout' }, asked: asked({}) }
  const { posted, seen, reason, polls } = await pickUp(failed, () => failed)
  assert.ok(reason instanceof MeropeApiError)
  assert.equal(reason.code, 'see_through_timeout')
  assert.equal(polls(), 1, 'read by its id once: only then does the server count the failure told')
  assert.equal(seen[0], 'failed')
  assert.deepEqual(posted, [], 'a failure is not a lost job: nothing started')
})

it('a job done or failed for another portrait or set is left for its own page', async () => {
  for (const stage of ['done', 'failed']) {
    const asked: string[] = []
    mock.method(apiService, 'get', async (path: string) => {
      asked.push(path)
      return { job: { ...pickedUp, outfitId: 'w-1', stage, done: 1, total: 1 } }
    })
    assert.equal(await resumeTurnKeyforms({ sourceMasterAssetId: '/media/assets/a/b.png' }), null, 'the bust is not the full-body set')
    assert.equal(await resumeTurnKeyforms({ sourceMasterAssetId: '/media/assets/other.png' }, { outfitId: 'w-1' }), null)
    // Not asked for by its id: the server still holds its result, or its failure, for the page whose job it is.
    assert.ok(asked.length === 2 && asked.every((path) => path.endsWith('/see-through/turn-keyforms')), asked.join(' '))
    mock.restoreAll()
  }
})

it('a picked-up job the server lost starts again as it was asked, not as the page asks by default', async () => {
  const lost = (path: string, polls: number) => {
    if (polls === 1) throw new ApiError('No such turn keys job', 404)
    assert.ok(path.endsWith('/turn-keyforms/again1'), path)
    return { stage: 'done', done: 1, total: 1 }
  }
  // A figure keyed by its head alone, no gesture arms: not the default, which keys both.
  const head = await pickUp(
    { ...pickedUp, outfitId: 'w-1', stage: 'decomposing', done: 1, total: 3, asked: asked({ head: true, body: false, expressions: false, arms: false }) },
    lost,
    'w-1',
  )
  assert.ok(head.file instanceof File)
  assert.equal(head.posted.length, 1)
  assert.ok(head.posted[0].path.endsWith('/full-body/w-1/see-through/turn-keyforms?expressions=false&arms=false'), head.posted[0].path)
  assert.deepEqual(head.posted[0].body, { sourceMasterAssetId: '/media/assets/a/b.png', head: true, body: false })
  mock.restoreAll()
  // The bust split once: not the default, which draws four turns.
  const plain = await pickUp(
    { ...pickedUp, stage: 'decomposing', done: 0, total: 1, asked: asked({ head: false, plain: true, expressions: true, arms: false }) },
    lost,
  )
  assert.deepEqual(plain.posted.map(({ body }) => body), [{ sourceMasterAssetId: '/media/assets/a/b.png', plain: true }])
  assert.ok(plain.posted[0].path.endsWith('/rig/see-through/turn-keyforms?expressions=true&arms=false'), plain.posted[0].path)
  mock.restoreAll()
  // A kept job fitted again: not a new job.
  const refit = await pickUp(
    { ...pickedUp, stage: 'fitting', done: 0, total: 1, asked: asked({ head: false, fromArchive: { id: 'kept-1', refit: true }, arms: true }) },
    lost,
  )
  assert.ok(refit.posted[0].path.endsWith('/see-through/turn-archives/kept-1/refit?arms=true'), refit.posted[0].path)
  assert.deepEqual(refit.posted[0].body, {})
})

it('a job picked up at its end is not started again when the server lost it meanwhile', async () => {
  const lost = () => { throw new ApiError('No such turn keys job', 404, 'turn_keyforms_missing', undefined, undefined, { error: 'No such turn keys job', code: 'turn_keyforms_missing' }) }
  for (const stage of ['done', 'failed'] as const) {
    const { posted, reason } = await pickUp(
      { ...pickedUp, stage, done: 1, total: 1, asked: asked({ head: true, expressions: true, arms: true }) },
      lost,
    )
    assert.deepEqual(posted, [], `${stage}: nothing is run over again`)
    assert.ok(reason instanceof MeropeApiError)
    assert.equal(reason.code, 'turn_keyforms_missing')
    mock.restoreAll()
  }
})

it('a picked-up job the server lost is not started again when its status did not say what it was asked', async () => {
  const { posted, reason } = await pickUp(
    { ...pickedUp, stage: 'decomposing', done: 2, total: 5 },
    () => { throw new ApiError('No such turn keys job', 404, 'turn_keyforms_missing', undefined, undefined, { error: 'No such turn keys job', code: 'turn_keyforms_missing' }) },
  )
  assert.deepEqual(posted, [], 'no request with the defaults')
  assert.ok(reason instanceof MeropeApiError)
  assert.equal(reason.code, 'turn_keyforms_missing', 'shown as the job being lost')
})

it('a job started here keeps what the page puts on its result, and starts again with the same', async () => {
  mock.timers.enable({ apis: ['setTimeout'] })
  const posted: unknown[] = []
  mock.method(apiService, 'post', async (path: string, body: unknown) => {
    posted.push([path.split('/see-through/')[1], body])
    return { jobId: `job${posted.length}` }
  })
  mock.method(apiService, 'get', async (path: string) => {
    if (path.endsWith('/psd')) return new Blob([new Uint8Array([1])])
    if (path.endsWith('/keyforms')) return keys
    if (path.endsWith('job1')) throw new ApiError('No such turn keys job', 404)
    return { stage: 'done', done: 1, total: 1 }
  })
  await settled(decomposeWithTurnKeyforms(
    { sourceMasterAssetId: '/media/assets/a/b.png' },
    { outfitId: 'w-1', parts: { head: false, body: true }, beside: { expressions: false, arms: true } },
  ))
  const asked = ['turn-keyforms?expressions=false&arms=true', { sourceMasterAssetId: '/media/assets/a/b.png', head: false, body: true }]
  assert.deepEqual(posted, [asked, asked])
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
