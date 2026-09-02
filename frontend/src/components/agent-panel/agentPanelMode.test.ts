import assert from 'node:assert/strict'
import { afterEach, test } from 'node:test'
import {
  cycleAgentPanelMode,
  DEFAULT_AGENT_PANEL_MODE,
  getAgentPanelMode,
  isAgentPanelMode,
  setAgentPanelMode,
  shouldCaptureModeTab,
  subscribeAgentPanelMode,
} from './agentPanelMode'

afterEach(() => {
  setAgentPanelMode(DEFAULT_AGENT_PANEL_MODE)
})

test('default mode is work', () => {
  assert.equal(DEFAULT_AGENT_PANEL_MODE, 'work')
  assert.equal(getAgentPanelMode(), 'work')
  assert.equal(isAgentPanelMode('work'), true)
  assert.equal(isAgentPanelMode('chat'), true)
  assert.equal(isAgentPanelMode('lite'), false)
})

test('cycle walks work then chat then work', () => {
  assert.equal(cycleAgentPanelMode(1), 'chat')
  assert.equal(getAgentPanelMode(), 'chat')
  assert.equal(cycleAgentPanelMode(1), 'work')
  assert.equal(cycleAgentPanelMode(-1), 'chat')
  assert.equal(cycleAgentPanelMode(-1), 'work')
})

test('set is a no-op when the mode did not change', () => {
  let ticks = 0
  const stop = subscribeAgentPanelMode(() => {
    ticks += 1
  })
  setAgentPanelMode('work')
  assert.equal(ticks, 0)
  setAgentPanelMode('chat')
  assert.equal(ticks, 1)
  setAgentPanelMode('chat')
  assert.equal(ticks, 1)
  stop()
})

function tabEvent(
  partial: Partial<KeyboardEvent> & { target?: unknown } = {},
): KeyboardEvent {
  return {
    key: 'Tab',
    altKey: false,
    metaKey: false,
    ctrlKey: false,
    shiftKey: false,
    defaultPrevented: false,
    isComposing: false,
    keyCode: 9,
    ...partial,
  } as KeyboardEvent
}

test('Tab is captured only when focus sits inside the panel shell', () => {
  const field = { id: 'field' }
  const outside = { id: 'outside' }
  const root = {
    contains: (node: Node) => node === (field as unknown as Node),
  }

  assert.equal(shouldCaptureModeTab(tabEvent(), root), false)
  assert.equal(
    shouldCaptureModeTab(tabEvent({ target: field as EventTarget }), root),
    true,
  )
  assert.equal(
    shouldCaptureModeTab(
      tabEvent({ target: field as EventTarget, shiftKey: true }),
      root,
    ),
    true,
  )
  assert.equal(
    shouldCaptureModeTab(tabEvent({ target: outside as EventTarget }), root),
    false,
  )
  assert.equal(
    shouldCaptureModeTab(
      tabEvent({ target: field as EventTarget, isComposing: true }),
      root,
    ),
    false,
  )
  assert.equal(
    shouldCaptureModeTab(
      tabEvent({ target: field as EventTarget, metaKey: true }),
      root,
    ),
    false,
  )
  assert.equal(
    shouldCaptureModeTab(
      tabEvent({ target: field as EventTarget, defaultPrevented: true }),
      root,
    ),
    false,
  )
  assert.equal(
    shouldCaptureModeTab(tabEvent({ target: field as EventTarget }), null),
    false,
  )
})
