import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'

describe('AgentSessionHost', () => {
  it('delays the engine until open or idle, and never unmounts after wake', () => {
    const host = readFileSync(new URL('./AgentSessionHost.tsx', import.meta.url), 'utf8')
    assert.match(host, /AGENT_SESSION_DELAY_MS = 5000/)
    assert.match(host, /AGENT_SESSION_IDLE_TIMEOUT_MS = 4000/)
    assert.match(host, /requestIdleCallback/)
    assert.match(host, /queueAgentPanelOpen/)
    assert.match(host, /AGENT_PANEL_OPEN_EVENT/)
    assert.match(host, /arael-open-session/)
    assert.match(host, /import\('\.\/AgentEngine'\)/)
    assert.match(host, /import\('\.\/AgentPanel'\)/)
    assert.equal(host.includes("from './AgentEngine'"), false)
    assert.equal(host.includes("from './AgentPanel'"), false)
    assert.match(host, /if \(ready\) return children/)
  })
})
