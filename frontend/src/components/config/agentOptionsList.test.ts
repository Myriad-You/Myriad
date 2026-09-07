import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import {
  AGENT_OPTIONS_LIST_PAGE,
  AGENT_OPTIONS_LIST_SCROLL_AFTER,
  agentOptionsListWindow,
} from './agentOptionsList'

describe('agentOptionsListWindow', () => {
  it('lets short lists grow with content', () => {
    assert.deepEqual(agentOptionsListWindow(0), {
      maxHeight: null,
      maxVisibleItems: null,
    })
    assert.deepEqual(agentOptionsListWindow(AGENT_OPTIONS_LIST_SCROLL_AFTER), {
      maxHeight: null,
      maxVisibleItems: null,
    })
  })

  it('caps long lists like analytics ranks', () => {
    assert.equal(AGENT_OPTIONS_LIST_SCROLL_AFTER, 8)
    assert.equal(AGENT_OPTIONS_LIST_PAGE, 30)
    assert.deepEqual(
      agentOptionsListWindow(AGENT_OPTIONS_LIST_SCROLL_AFTER + 1),
      {
        maxHeight: '22rem',
        maxVisibleItems: AGENT_OPTIONS_LIST_PAGE,
      },
    )
  })
})

describe('AgentOptionsPanel list window', () => {
  it('applies the window to heartbeat, skills, and memory', () => {
    const src = readFileSync(
      new URL('./AgentOptionsPanel.tsx', import.meta.url),
      'utf8',
    )
    assert.match(src, /agentOptionsListWindow\(filteredTasks\.length\)/)
    assert.match(src, /agentOptionsListWindow\(filteredSkills\.length\)/)
    assert.match(src, /agentOptionsListWindow\(filteredMemories\.length\)/)
    assert.doesNotMatch(src, /maxHeight=\{null\}/)
  })
})
