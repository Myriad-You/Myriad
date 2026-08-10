/**
 *   pnpm exec tsx --test src/services/agent/mcpServerConfig.test.ts
 */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { parseMcpServerConfig } from './agentApi.ts'

const base = {
  id: 'docs',
  command: 'npx',
  args: ['-y', '@modelcontextprotocol/server-docs'],
  env: { TOKEN: 'x' },
  enabled: true,
  auto_restart: true,
  max_restart_attempts: 3,
}

describe('parseMcpServerConfig', () => {
  it('round-trips trust_annotations: true', () => {
    // The settings panel rebuilds each server field by field, so a dropped
    // field here would show the switch off after a reload and write `false`
    // back on the next save, revoking the operator's opt-in.
    const parsed = parseMcpServerConfig({ ...base, trust_annotations: true })
    assert.equal(parsed?.trust_annotations, true)
  })

  it('defaults trust_annotations to false when the backend omits it', () => {
    const parsed = parseMcpServerConfig(base)
    assert.equal(parsed?.trust_annotations, false)
  })

  it('treats any non-true value as false', () => {
    for (const value of [false, 'true', 1, null, undefined, {}]) {
      const parsed = parseMcpServerConfig({ ...base, trust_annotations: value })
      assert.equal(
        parsed?.trust_annotations,
        false,
        `trust must not be inferred from ${JSON.stringify(value)}`,
      )
    }
  })

  it('keeps the rest of the server shape', () => {
    const parsed = parseMcpServerConfig({ ...base, trust_annotations: true })
    assert.equal(parsed?.id, 'docs')
    assert.equal(parsed?.command, 'npx')
    assert.deepEqual(parsed?.args, [
      '-y',
      '@modelcontextprotocol/server-docs',
    ])
    assert.deepEqual(parsed?.env, { TOKEN: 'x' })
    assert.equal(parsed?.enabled, true)
    assert.equal(parsed?.auto_restart, true)
    assert.equal(parsed?.max_restart_attempts, 3)
  })

  it('rejects entries without an id or command', () => {
    assert.equal(parseMcpServerConfig({ ...base, id: '  ' }), null)
    assert.equal(parseMcpServerConfig({ ...base, command: '' }), null)
    assert.equal(parseMcpServerConfig(null), null)
  })
})
