import assert from 'node:assert/strict'
import { it } from 'node:test'
import { BrewItemState } from './brewItemState'

it('projects confirmed flags across views and rejects responses started before a mutation', () => {
  const state = new BrewItemState()
  const before = state.getSnapshot()
  state.commit(1, { is_starred: true })
  state.observe(1, { is_starred: false, is_read: true }, before)
  assert.deepEqual(state.project({ id: 1, title: 'reader' }), {
    id: 1,
    title: 'reader',
    is_starred: true,
    is_read: true,
  })
  assert.equal(
    state.project({ id: 1, title: 'card', is_starred: false }).is_starred,
    true,
  )
  state.observe(1, { is_starred: false }, state.getSnapshot())
  assert.equal(state.project({ id: 1, is_starred: true }).is_starred, false)
  state.clear()
  assert.deepEqual(state.project({ id: 1 }), { id: 1 })
})

it('even identical confirmations prevent older reads from undoing a mutation', () => {
  const state = new BrewItemState()
  state.commit(1, { is_read: true })
  const before = state.getSnapshot()
  state.commit(1, { is_read: true })
  state.observe(1, { is_read: false }, before)
  assert.equal(state.project({ id: 1, is_read: false }).is_read, true)
})

it('projects in-flight previews until confirm or matching rollback', () => {
  const state = new BrewItemState()
  state.preview(1, { is_read: true })
  assert.equal(state.project({ id: 1, is_read: false }).is_read, true)
  state.discardPreview(1, { is_read: false })
  assert.equal(state.project({ id: 1, is_read: false }).is_read, true)
  state.discardPreview(1, { is_read: true })
  assert.equal(state.project({ id: 1, is_read: false }).is_read, false)
  state.preview(1, { is_read: false })
  state.commit(1, { is_read: true })
  assert.equal(state.project({ id: 1 }).is_read, true)
})

it('publishes a batch once and applies global read confirmation to known articles', () => {
  const state = new BrewItemState()
  let notifications = 0
  state.subscribe(() => {
    notifications++
  })
  state.observeMany(
    [
      { id: 1, is_read: false },
      { id: 2, is_read: false },
    ],
    state.getSnapshot(),
  )
  assert.equal(notifications, 1)
  const stale = state.getSnapshot()
  state.markAllRead()
  assert.equal(notifications, 2)
  state.observeMany(
    [
      { id: 1, is_read: false },
      { id: 2, is_read: false },
    ],
    stale,
  )
  assert.equal(state.project({ id: 1, is_read: false }).is_read, true)
  assert.equal(state.project({ id: 2, is_read: false }).is_read, true)
  assert.equal(notifications, 2)
})
