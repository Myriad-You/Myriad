import assert from 'node:assert/strict'
import test from 'node:test'
import { agentPanelSubmitDetail } from './agentPanelEvents'

test('accepted intention stays attached to an explicit Work submit', () => {
  const event = {
    detail: {
      text: 'Prepare the recovery summary',
      mode: 'work',
      intentionId: 'int_test',
    },
  } as unknown as Event

  assert.deepEqual(agentPanelSubmitDetail(event), {
    text: 'Prepare the recovery summary',
    mode: 'work',
    intentionId: 'int_test',
  })
})

test('missing mode remains backwards-compatible with Work', () => {
  const event = {
    detail: { text: 'Keep the old path' },
  } as unknown as Event

  assert.deepEqual(agentPanelSubmitDetail(event), {
    text: 'Keep the old path',
    mode: 'work',
  })
})
