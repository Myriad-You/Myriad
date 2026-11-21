import assert from 'node:assert/strict'
import test from 'node:test'
import { herTurnMarks } from './herTurnMarks'
import { projectAgentMessage } from './projectAgentMessage'
import { restoreSessionMessage } from './sessionHistoryMessage'

const CREATED = '2026-11-20T08:00:00.000Z'

test('a turn she left reads as 已读, never as words', () => {
  assert.deepEqual(herTurnMarks('[[quiet]]'), { text: '', left: true })
  const shown = projectAgentMessage(
    restoreSessionMessage(
      { id: 1, role: 'assistant', content: '[[quiet]]', createdAt: CREATED },
      'ses_a',
    ),
  )
  assert.equal(shown.left, true)
  assert.equal(shown.content, '')
  assert.equal(shown.reaction, undefined)
})

test('a reaction goes on their line, with her words or alone', () => {
  assert.deepEqual(herTurnMarks('[[react:👍]]'), { text: '', left: false, reaction: '👍' })
  assert.deepEqual(herTurnMarks('好诶\n[[react：❤️]]'), {
    text: '好诶',
    left: false,
    reaction: '❤',
  })
  const shown = projectAgentMessage(
    restoreSessionMessage(
      { id: 2, role: 'assistant', content: '好诶\n[[react:😁]]', createdAt: CREATED },
      'ses_a',
    ),
  )
  assert.equal(shown.reaction, '😁')
  assert.equal(shown.content, '好诶')
  assert.equal(shown.left, undefined)
})

test('words with the quiet mark are an answer, and their lines are theirs', () => {
  assert.deepEqual(herTurnMarks('嗯嗯\n[[quiet]]'), { text: '嗯嗯', left: false })
  const theirs = projectAgentMessage(
    restoreSessionMessage(
      { id: 3, role: 'user', content: '[[quiet]]', createdAt: CREATED },
      'ses_a',
    ),
  )
  assert.equal(theirs.content, '[[quiet]]')
  assert.equal(theirs.left, undefined)
})
