import assert from 'node:assert/strict'
import { it } from 'node:test'
import { BrewSyncConflictError } from '../utils/brewSyncConflict'
import { markAllRead, markRead, syncReadingStates, updateReadProgress } from './brewApi'

it('requires the target article confirmation and rejects per-item failures', async () => {
  const previousFetch = globalThis.fetch
  const storage = Object.getOwnPropertyDescriptor(globalThis, 'sessionStorage')
  Object.defineProperty(globalThis, 'sessionStorage', { configurable: true, value: { getItem: () => null, setItem: () => {}, removeItem: () => {} } })
  let result: Record<string, unknown> = { synced: 1, conflicts: [], confirmed: [8], failed: [] }
  let sent: { states: Array<{ expected_revision?: number }> } | undefined
  globalThis.fetch = async (input, init) => {
    if (String(input).includes('csrf-token')) return Response.json({ csrf_token: null })
    if (init?.body) sent = JSON.parse(String(init.body))
    return Response.json(result)
  }
  try {
    await assert.rejects(updateReadProgress(7, 50), /not saved/)
    result = { synced: 1, conflicts: [], confirmed: [7], failed: [7] }
    await assert.rejects(updateReadProgress(7, 50), /not saved/)
    result = { synced: 1, conflicts: [], confirmed: [7], failed: [], revisions: { 7: 4 } }
    assert.equal(await updateReadProgress(7, 50, { expectedRevision: 3 }), 4)
    assert.equal(sent?.states[0]?.expected_revision, 3)
    result = { success: true, previous_revision: 3, revision: 4 }
    await markRead(7)
    result = { synced: 1, conflicts: [], confirmed: [7], revisions: { 7: 5 } }
    assert.equal(await updateReadProgress(7, 60, { expectedRevision: 3 }), 5)
    assert.equal(sent?.states[0]?.expected_revision, 4)
    result = { success: true, previous_revision: 6, revision: 7 }
    await markRead(7)
    result = { synced: 0, conflicts: [{ item_id: 7, server_revision: 7 }] }
    await assert.rejects(updateReadProgress(7, 70, { expectedRevision: 5 }), BrewSyncConflictError)
    assert.equal(sent?.states[0]?.expected_revision, 5)
    result = { synced: 0, conflicts: [{ item_id: 7, server_revision: 4 }] }
    await assert.rejects(updateReadProgress(7, 50), error => error instanceof BrewSyncConflictError && error.serverRevision === 4)
    result = { success: true, marked: 1, changes: [{ item_id: 9, previous_revision: 2, revision: 3 }] }
    assert.equal(await markAllRead({ source_id: 1 }), 1)
    result = { synced: 1, conflicts: [], confirmed: [9], revisions: { 9: 4 } }
    assert.equal(await updateReadProgress(9, 40, { expectedRevision: 2 }), 4)
    assert.equal(sent?.states[0]?.expected_revision, 3)
    result = { synced: 1, conflicts: [] }
    assert.equal(await updateReadProgress(7, 50), undefined)
  } finally {
    globalThis.fetch = previousFetch
    if (storage) Object.defineProperty(globalThis, 'sessionStorage', storage)
    else Reflect.deleteProperty(globalThis, 'sessionStorage')
  }
})

it('invalid batches are rejected before sending any request', async () => {
  const original = globalThis.fetch
  globalThis.fetch = async () => assert.fail('invalid batch reached transport')
  try {
    for (const ids of [[1, 1], [0], [-1], [1.5]]) {
      await assert.rejects(syncReadingStates(ids.map(item_id => ({ item_id, updated_at: 1, is_read: true }))), /Invalid or duplicate/)
    }
    for (const expected_revision of [-1, 1.5, Number.NaN, Number.POSITIVE_INFINITY, Number.MAX_SAFE_INTEGER + 1]) {
      await assert.rejects(syncReadingStates([{ item_id: 1, updated_at: 1, expected_revision }]), /Invalid state revision/)
    }
    assert.deepEqual(await syncReadingStates([]), { synced: 0, conflicts: [] })
  } finally {
    globalThis.fetch = original
  }
})
