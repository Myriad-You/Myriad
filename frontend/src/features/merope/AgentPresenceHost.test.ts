import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'

describe('AgentPresenceHost', () => {
  it('keeps inbound and playback off the static module graph', () => {
    const host = readFileSync(new URL('./AgentPresenceHost.tsx', import.meta.url), 'utf8')
    assert.match(host, /import\('\.\/perception\/inbound'\)/)
    assert.match(host, /import\('\.\/motion\/playbackDirectionHost'\)/)
    assert.equal(host.includes("from './perception/inbound'"), false)
    assert.equal(host.includes("from './motion/playbackDirectionHost'"), false)
    assert.match(host, /startPresenceInbound/)
    assert.match(host, /retainPlaybackDirection/)
    assert.match(host, /notePresenceRoute/)
  })
})
