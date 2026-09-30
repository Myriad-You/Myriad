import assert from 'node:assert/strict'
import test from 'node:test'
import { agentStatusActivity } from './activity'

test('live work state drives thinking while speech remains speech-owned', () => {
  assert.equal(agentStatusActivity('thinking'), 'thinking')
  assert.equal(agentStatusActivity('working'), 'thinking')
  assert.equal(agentStatusActivity('idle'), 'idle')
  assert.equal(agentStatusActivity('listening'), 'idle')
  assert.equal(agentStatusActivity('talking'), 'idle')
})
