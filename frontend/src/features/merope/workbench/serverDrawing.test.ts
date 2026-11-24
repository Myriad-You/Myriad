import assert from 'node:assert/strict'
import test from 'node:test'
import { drawingFailures, untilNoneDrawn } from './serverDrawing'

const taken: PromiseSettledResult<unknown> = { status: 'fulfilled', value: undefined }

function refused(reason: unknown): PromiseSettledResult<unknown> {
  return { status: 'rejected', reason }
}

function lost(): string {
  return 'lost'
}

function kept(before: Record<string, string>, after: Record<string, string>) {
  return {
    before: (key: string) => before[key],
    after: (key: string) => after[key],
  }
}

test('a drawing kept anew is no failure, whether or not an older one was kept', () => {
  const failures = drawingFailures(
    ['wave-left', 'wave-right'],
    [taken, taken],
    kept({ 'wave-right': '/old' }, { 'wave-left': '/a', 'wave-right': '/new' }),
    {},
    lost,
  )
  assert.deepEqual(failures, [])
})

test('one the server refused to take up fails with what it answered', () => {
  const busy = new Error('already being drawn')
  assert.deepEqual(drawingFailures(['chin-left'], [refused(busy)], kept({}, {}), {}, lost), [busy])
})

test('one taken up and not kept fails with what the list says went wrong, in the order asked', () => {
  const rejected = new Error('design changed')
  const failures = drawingFailures(
    ['wave-left', 'point-left', 'chest-left'],
    [taken, taken, refused('no portrait')],
    kept({}, { 'wave-left': '/a' }),
    { 'point-left': rejected },
    lost,
  )
  assert.deepEqual(failures, [rejected, 'no portrait'])
})

test('one neither kept anew nor failed was lost by the server', () => {
  // Drawn again over a kept one: the old picture still listed is not the new drawing.
  assert.deepEqual(drawingFailures(['wave-left'], [taken], kept({ 'wave-left': '/old' }, { 'wave-left': '/old' }), {}, lost), ['lost'])
  assert.deepEqual(drawingFailures(['wave-left'], [taken], kept({}, {}), {}, lost), ['lost'])
})

test('the list is read once more after the wait, and waited on again while it names some', async () => {
  // The list that ended the first wait was read before the server took the drawings up.
  const lists = [['wave-left'], []]
  let drawing: string[] = []
  const calls: string[] = []
  await untilNoneDrawn({
    settled: async () => { calls.push('settled') },
    reread: async () => {
      calls.push('reread')
      drawing = lists.shift() ?? []
    },
    drawing: () => drawing,
  })
  assert.deepEqual(calls, ['settled', 'reread', 'settled', 'reread'])
})

test('a read that fails leaves the list as it was, and a picture gone is not waited for', async () => {
  let rereads = 0
  await untilNoneDrawn({
    settled: async () => {},
    reread: async () => {
      rereads += 1
      throw new Error('offline')
    },
    drawing: () => [],
  })
  assert.equal(rereads, 1)
  await untilNoneDrawn({
    settled: async () => {},
    reread: async () => { rereads += 1 },
    drawing: () => ['wave-left'],
    gone: () => true,
  })
  assert.equal(rereads, 1)
})

test('a list that never comes to name none is waited on only so long', async () => {
  let rereads = 0
  await untilNoneDrawn({
    settled: () => new Promise<void>(() => {}),
    reread: async () => { rereads += 1 },
    drawing: () => ['wave-left'],
  }, 20)
  assert.equal(rereads, 0)
})
