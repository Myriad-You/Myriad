import assert from 'node:assert/strict'
import { afterEach, test } from 'node:test'
import {
  getAgentPanelVisible,
  isLookingAtAgentPanel,
  setAgentPanelVisible,
  subscribeAgentPanelVisible,
} from './agentPanelVisible'

afterEach(() => {
  setAgentPanelVisible(false)
})

test('agent panel visibility defaults off and notifies on change', () => {
  assert.equal(getAgentPanelVisible(), false)
  const seen: boolean[] = []
  const stop = subscribeAgentPanelVisible(() => seen.push(getAgentPanelVisible()))
  setAgentPanelVisible(true)
  setAgentPanelVisible(true)
  setAgentPanelVisible(false)
  stop()
  assert.deepEqual(seen, [true, false])
})

test('looking at the panel requires it to be open', () => {
  assert.equal(isLookingAtAgentPanel(), false)
  setAgentPanelVisible(true)
  assert.equal(isLookingAtAgentPanel(), true)
  setAgentPanelVisible(false)
  assert.equal(isLookingAtAgentPanel(), false)
})
