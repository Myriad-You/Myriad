import type { PerceptionSnapshot } from './registry'
import assert from 'node:assert/strict'
import test from 'node:test'
import { setAgentContextConsent } from '../../../components/agent-panel/agentContextConsent'
import { setCurrentPageContent } from '../../../contexts/currentPage'
import { livePresenceFacts } from '../livePresence'
import { capturePerceptionSnapshots } from './capture'
import {
  reportPresence, resetPresenceInboundForTest, setPresenceArmedForTest,
  setPresenceCaptureForTest, setPresenceFactsForTest, setPresencePostForTest,
} from './inbound'
import { PRESENCE_LEASE_MS, STABLE_OBSERVATION_TTL_MS } from './leasePolicy'
import { perceptionRegistry } from './registry'

test('stable page facts bridge unchanged reports and renew before expiry; transient facts still expire', async (t) => {
  let now = 100_000
  t.mock.method(Date, 'now', () => now)
  const posts: { at: number; perception: PerceptionSnapshot[] }[] = []
  resetPresenceInboundForTest()
  setAgentContextConsent(true)
  setCurrentPageContent({ type: 'custom', title: 'Still reading', plainText: 'Consented page' })
  setPresenceArmedForTest(true)
  setPresenceCaptureForTest(capturePerceptionSnapshots)
  const facts = livePresenceFacts()
  setPresenceFactsForTest(() => facts)
  setPresencePostForTest(async body => {
    posts.push({ at: now, perception: (body as { perception: PerceptionSnapshot[] }).perception })
  })
  try {
    await reportPresence('start')
    const page = posts[0]!.perception.find(row => row.sourceId === 'page')!
    const voice = posts[0]!.perception.find(row => row.sourceId === 'voice')!
    assert.equal(page.ttlMs, STABLE_OBSERVATION_TTL_MS)
    assert.ok(page.ttlMs > PRESENCE_LEASE_MS)
    assert.ok(voice.ttlMs <= 4_000)
    now += 9_000
    await reportPresence('page-content')
    assert.equal(posts.length, 1, 'unchanged content need not send another request')
    assert.ok(page.ttlMs - (now - posts[0]!.at) > 0)
    assert.ok(voice.ttlMs - (now - posts[0]!.at) <= 0)
    now = posts[0]!.at + PRESENCE_LEASE_MS
    await reportPresence('lease')
    assert.equal(posts.length, 2)
    assert.equal(posts[1]!.perception.find(row => row.sourceId === 'page')!.ttlMs, STABLE_OBSERVATION_TTL_MS)
    // Revocation clears now; the longer TTL must never delay it.
    setAgentContextConsent(false)
    await reportPresence('page-consent')
    assert.ok(!posts.at(-1)!.perception.some(row => row.sourceId === 'page'))
    assert.equal(livePresenceFacts().instanceId, facts.instanceId)
    assert.ok(facts.instanceId.length <= 64)
  } finally {
    resetPresenceInboundForTest()
    setAgentContextConsent(false)
    setCurrentPageContent(null)
    for (const row of perceptionRegistry.active()) perceptionRegistry.forget(row.sourceId)
  }
})
