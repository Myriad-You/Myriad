import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { it } from 'node:test'
import { adoptSessionRows } from './sessionRows'

it('a new session adopts the rows of the turn that started it, and nothing else', () => {
  const rows = [
    { id: 'u', sessionId: '' },
    { id: 'a', sessionId: '' },
  ]
  assert.deepEqual(adoptSessionRows(rows, 'ses_1'), [
    { id: 'u', sessionId: 'ses_1' },
    { id: 'a', sessionId: 'ses_1' },
  ])
  const settled = [{ id: 'x', sessionId: 'ses_0' }]
  assert.equal(adoptSessionRows(settled, 'ses_1'), settled)
})

it('the engine hands session_created to adoptSessionRows, so first-turn confirmations can be answered', () => {
  const engine = readFileSync(new URL('./AgentEngine.tsx', import.meta.url), 'utf8')
  assert.match(
    engine,
    /case 'session_created': \{[\s\S]{0,400}setMessages\(\(prev\) => adoptSessionRows\(prev, event\.sessionId\), mode\)/,
  )
})
