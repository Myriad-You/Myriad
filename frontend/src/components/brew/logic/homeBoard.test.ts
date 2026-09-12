import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { makeItem } from './fixtures.ts'
import {
  NOTES_FEATURED_MAX,
  noteSourceKey,
  pickHomeBoardNotes,
  toHomeBoardNote,
} from './homeBoard.ts'

describe('toHomeBoardNote', () => {
  it('只收精选要的字段', () => {
    const note = toHomeBoardNote(
      makeItem({
        id: 7,
        title: 'n7',
        summary: 's',
        image: '/c.jpg',
        source_id: 10,
      }),
    )
    assert.deepEqual(note, {
      id: 7,
      title: 'n7',
      summary: 's',
      image: '/c.jpg',
      published_at: note.published_at,
      source_id: 10,
    })
    assert.equal(NOTES_FEATURED_MAX, 3)
  })
})

describe('pickHomeBoardNotes', () => {
  it('只收手记源上的条目，并截到精选上限', () => {
    const notes = pickHomeBoardNotes(
      [
        makeItem({ id: 1, title: 'a', source_id: 10 }),
        makeItem({ id: 2, title: 'b', source_id: 11 }),
        makeItem({ id: 3, title: 'c', source_id: 10 }),
        makeItem({ id: 4, title: 'd', source_id: 10 }),
        makeItem({ id: 5, title: 'e', source_id: 10 }),
      ],
      [
        { id: 10, source_type: 'note' },
        { id: 11, source_type: 'rss' },
      ],
    )
    assert.deepEqual(
      notes.map((note) => note.id),
      [1, 3, 4],
    )
  })

  it('没有手记源时不收', () => {
    assert.deepEqual(
      pickHomeBoardNotes([makeItem({ source_id: 10 })], [
        { id: 10, source_type: 'rss' },
      ]),
      [],
    )
  })
})

describe('noteSourceKey', () => {
  it('只签名手记源，没有则空串', () => {
    assert.equal(
      noteSourceKey([
        { id: 10, source_type: 'note' },
        { id: 11, source_type: 'rss' },
        { id: 12, source_type: 'note' },
      ]),
      '10,12',
    )
    assert.equal(noteSourceKey([{ id: 11, source_type: 'rss' }]), '')
  })
})
