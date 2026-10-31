import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { afterEach, test } from 'node:test'
import {
  DEFAULT_AGENT_PANEL_MODE,
  getAgentPanelMode,
  isAgentPanelMode,
  setAgentPanelMode,
  subscribeAgentPanelMode,
} from './agentPanelMode'

afterEach(() => {
  setAgentPanelMode(DEFAULT_AGENT_PANEL_MODE)
})

test('the panel opens on the one conversation with her', () => {
  assert.equal(DEFAULT_AGENT_PANEL_MODE, 'chat')
  assert.equal(getAgentPanelMode(), 'chat')
  assert.equal(isAgentPanelMode('work'), true)
  assert.equal(isAgentPanelMode('chat'), true)
  assert.equal(isAgentPanelMode('lite'), false)
})

test('set is a no-op when the mode did not change', () => {
  let ticks = 0
  const stop = subscribeAgentPanelMode(() => {
    ticks += 1
  })
  setAgentPanelMode('chat')
  assert.equal(ticks, 0)
  setAgentPanelMode('work')
  assert.equal(ticks, 1)
  setAgentPanelMode('work')
  assert.equal(ticks, 1)
  stop()
})

test('no key cycles the modes: Work is entered only on its own', () => {
  const panel = readFileSync(new URL('./AgentPanel.tsx', import.meta.url), 'utf8')
  assert.doesNotMatch(panel, /cycleAgentPanelMode|shouldCaptureModeTab/)
  const composer = readFileSync(
    new URL('./AgentPanelComposer.tsx', import.meta.url),
    'utf8',
  )
  assert.match(composer, /\{resident \|\| mode !== 'work' \? null : <BackToHer \/>\}/)
  assert.match(composer, /onClick=\{\(\) => setAgentPanelMode\('chat'\)\}/)
})
