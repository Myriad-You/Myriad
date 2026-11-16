import assert from 'node:assert/strict'
import { beforeEach, it } from 'node:test'
import * as drafts from './noteDraft'

const base = { title: 'Title', contentMd: 'first\nsecond', topic: 'old', cover: null, publishedAt: 1 }

beforeEach(() => {
  const storage = new Map<string, string>()
  Object.defineProperty(globalThis, 'localStorage', {
    configurable: true,
    value: {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
      removeItem: (key: string) => storage.delete(key),
    },
  })
})

it('does not recover an item-number draft into a cloud document with the same number', () => {
  drafts.writeNoteDraft(7, { ...base, contentMd: 'another article' })
  assert.equal(typeof drafts.readNoteRecovery, 'function')
  assert.equal(drafts.readNoteRecovery({ userId: 1, docId: 7 }), null)
})

it('a previously acknowledged local copy never replaces a newer cloud document', () => {
  assert.equal(typeof drafts.writeNoteRecovery, 'function')
  drafts.writeNoteRecovery({ userId: 1, docId: 7 }, base, base, 2)
  const remote = { ...base, contentMd: 'remote rewrite' }
  assert.deepEqual(drafts.recoverNoteFields(drafts.readNoteRecovery({ userId: 1, docId: 7 }), remote), remote)
})

it('recovers only unsaved changes and preserves independent newer remote changes', () => {
  assert.equal(typeof drafts.writeNoteRecovery, 'function')
  drafts.writeNoteRecovery({ userId: 1, docId: 7 }, { ...base, contentMd: 'LOCAL\nsecond', topic: null }, base, 2)
  const remote = { ...base, contentMd: 'first\nREMOTE', cover: '/remote.png' }
  assert.deepEqual(drafts.recoverNoteFields(drafts.readNoteRecovery({ userId: 1, docId: 7 }), remote), {
    ...base, contentMd: 'LOCAL\nREMOTE', topic: null, cover: '/remote.png', slug: null, oldPaths: null,
  })
})

it('keeps an unsaved publication time when the remote time did not change', () => {
  assert.equal(typeof drafts.writeNoteRecovery, 'function')
  drafts.writeNoteRecovery({ userId: 1, docId: 7 }, { ...base, publishedAt: 500 }, base, 2)
  assert.equal(drafts.recoverNoteFields(drafts.readNoteRecovery({ userId: 1, docId: 7 }), base).publishedAt, 500)
})

it('does not mistake null fields or a malformed recovery baseline for trusted recovery', () => {
  assert.equal(typeof drafts.writeNoteRecovery, 'function')
  drafts.writeNoteRecovery({ userId: 1, docId: 7 }, { ...base, topic: null }, base, 2)
  const recovery = drafts.readNoteRecovery({ userId: 1, docId: 7 })
  assert.equal(recovery?.fields.topic, null)
  localStorage.setItem(drafts.noteDraftKey('user:1:doc:8'), JSON.stringify({ fields: base, base: {}, revision: 2, savedAt: Date.now() }))
  assert.equal(drafts.readNoteRecovery({ userId: 1, docId: 8 }), null)
})

it('keeps recovery private to the stable user identity even when another admin opens the same document', () => {
  drafts.writeNoteRecovery({ userId: 10, docId: 7 }, { ...base, contentMd: 'private unconfirmed edit' }, base, 2)
  assert.equal(drafts.readNoteRecovery({ userId: 20, docId: 7 }), null)
  assert.equal(drafts.readNoteRecovery({ userId: 10, docId: 7 })?.fields.contentMd, 'private unconfirmed edit')
})

it('restores for the same user after a new session without depending on a session generation', () => {
  drafts.writeNoteRecovery({ userId: 10, docId: 7 }, { ...base, contentMd: 'unfinished' }, base, 2)
  assert.equal(drafts.readNoteRecovery({ userId: 10, docId: 7 })?.fields.contentMd, 'unfinished')
  assert.equal(drafts.readNoteRecovery({ userId: 10, docId: 8 }), null)
})

it('an unidentified write keeps local text intact, and a matching snapshot does not duplicate submitted text', () => {
  const submitted = { ...base, contentMd: 'a' }
  const local = { ...base, contentMd: 'ab' }
  const pending = { requestId: 'pending', fields: submitted }
  drafts.writeNoteRecovery({ userId: 1, docId: 7 }, local, { ...base, contentMd: '' }, 1, Date.now(), pending)
  const recovered = drafts.readNoteRecovery({ userId: 1, docId: 7 })!
  assert.deepEqual(recovered.pending, pending)
  assert.equal(drafts.recoverNoteFields(recovered, { ...submitted, contentMd: 'a remote' }).contentMd, 'ab')
  assert.equal(drafts.recoverNoteFields(recovered, submitted).contentMd, 'ab')
})

it('keeps a pending command even when its submitted content is unchanged', () => {
  const pending = { requestId: 'publish', fields: base }
  drafts.writeNoteRecovery({ userId: 1, docId: 7 }, base, base, 1, Date.now(), pending)
  assert.deepEqual(drafts.readNoteRecovery({ userId: 1, docId: 7 })?.pending, pending)
})

it('recovery retains both authors when the same paragraph conflicts', () => {
  const local = { ...base, contentMd: 'Local rewrite' }
  drafts.writeNoteRecovery({ userId: 1, docId: 7 }, local, base, 1)
  const remote = { ...base, contentMd: 'Remote rewrite' }
  assert.equal(drafts.recoverNoteFields(drafts.readNoteRecovery({ userId: 1, docId: 7 }), remote).contentMd,
    'Local rewrite\nRemote rewrite')
})

it('an unchanged server preserves all local fields through account-scoped recovery', () => {
  const local = { ...base, title: 'Edited', contentMd: 'Unsent', topic: 'local category', cover: '/local.png', publishedAt: 500 }
  drafts.writeNoteRecovery({ userId: 1, docId: 7 }, local, base, 1)
  assert.deepEqual(drafts.recoverNoteFields(drafts.readNoteRecovery({ userId: 1, docId: 7 }), base), { ...local, slug: null, oldPaths: null })
})

it('opening legacy records never assigns them to an account or destroys them', () => {
  for (const id of ['new', 'doc:7', 7] as const) {
    drafts.writeNoteDraft(id, { ...base, contentMd: 'Unowned text' })
    const key = drafts.noteDraftKey(id)
    const before = localStorage.getItem(key)
    assert.equal(drafts.readNoteRecovery({ userId: 1, docId: 7 }), null)
    assert.equal(localStorage.getItem(key), before)
  }
})

it('reopening after a lost restore acknowledgement recognizes the restored version', () => {
  const current = { ...base, contentMd: 'current draft' }
  const target = { ...base, contentMd: 'historical draft' }
  drafts.writeNoteRecovery({ userId: 1, docId: 7 }, current, base, 1, Date.now(), {
    requestId: 'restore', fields: current, expectedFields: target,
  })
  assert.deepEqual(drafts.recoverNoteFields(drafts.readNoteRecovery({ userId: 1, docId: 7 }), target), { ...target, slug: null, oldPaths: null })
})
