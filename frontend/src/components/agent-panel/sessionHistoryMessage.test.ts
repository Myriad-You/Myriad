import assert from 'node:assert/strict'
import test from 'node:test'
import { projectAgentMessage } from './projectAgentMessage'
import { restoreSessionMessage } from './sessionHistoryMessage'

const CREATED = '2026-11-06T08:00:00.000Z'

test('a line said on another end keeps where it was said and its pictures', () => {
  const restored = restoreSessionMessage(
    {
      id: 7,
      role: 'user',
      content: '在 Mac 上说的',
      metadata: { said: { on: 'dev_mac', name: 'Mac', place: 'app' }, pictures: 2 },
      createdAt: CREATED,
    },
    'ses_a',
  )
  assert.deepEqual(restored.saidOn, { name: 'Mac' })
  assert.equal(restored.pictures, 2)
  const shown = projectAgentMessage(restored)
  assert.deepEqual(shown.saidOn, { name: 'Mac' })
  assert.equal(shown.pictures, 2)
})

test('a copy from an end with no name still says it was elsewhere', () => {
  const restored = restoreSessionMessage(
    {
      id: 8,
      role: 'assistant',
      content: '回：在 Mac 上说的',
      metadata: { said: { on: 'dev_mac', name: null, place: 'web' } },
      createdAt: CREATED,
    },
    'ses_a',
  )
  assert.deepEqual(restored.saidOn, { name: null })
  assert.equal(restored.pictures, undefined)
})

test('a line said here carries neither', () => {
  const restored = restoreSessionMessage(
    { id: 9, role: 'user', content: '早', createdAt: CREATED },
    'ses_a',
  )
  assert.equal(restored.saidOn, undefined)
  assert.equal(projectAgentMessage(restored).saidOn, undefined)
})
