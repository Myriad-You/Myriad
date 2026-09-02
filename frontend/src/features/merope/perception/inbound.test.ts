import type { PerceptionSnapshot } from './registry'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { setAgentContextConsent } from '../../../components/agent-panel/agentContextConsent'
import {
  presenceInboundArmingForTest,
  reportPresence,
  resetPresenceInboundForTest,
  setPresenceArmedForTest,
  setPresenceCaptureForTest,
  setPresenceEnabledForTest,
  setPresenceFactsForTest,
  setPresencePostForTest,
  startPresenceInbound,
} from './inbound'

function snapshot(
  sourceId: string,
  revision: number,
  summary: string,
): PerceptionSnapshot {
  return {
    sourceId,
    kind: sourceId === 'page' ? 'page' : 'presence',
    revision,
    capturedAt: 1,
    expiresAt: 1_000_000,
    ttlMs: 1000,
    summary,
    safeFacts: {},
    privacy: sourceId === 'page' ? 'consented' : 'system',
  }
}

test.describe('presence inbound', { concurrency: false }, () => {
  test('same revision set is posted once', async () => {
    resetPresenceInboundForTest()
    setPresenceArmedForTest(true)
    const posts: unknown[] = []
    setPresenceFactsForTest(() => ({ speaking: false }))
    setPresenceCaptureForTest(() => [
      snapshot('presence', 1, 'page visible'),
    ])
    setPresencePostForTest(async (body) => {
      posts.push(body)
    })
    await reportPresence('route')
    await reportPresence('route')
    assert.equal(posts.length, 1)
  })

  test('unarmed inbound does not post', async () => {
    resetPresenceInboundForTest()
    const posts: unknown[] = []
    setPresenceFactsForTest(() => ({ speaking: false }))
    setPresenceCaptureForTest(() => [
      snapshot('presence', 1, 'page visible'),
    ])
    setPresencePostForTest(async (body) => {
      posts.push(body)
    })
    await reportPresence('route')
    assert.equal(posts.length, 0)
  })

  test('disabled merope does not arm inbound', async () => {
    resetPresenceInboundForTest()
    setPresenceEnabledForTest(() => false)
    const posts: unknown[] = []
    setPresenceFactsForTest(() => ({ speaking: false }))
    setPresenceCaptureForTest(() => [
      snapshot('presence', 1, 'page visible'),
    ])
    setPresencePostForTest(async (body) => {
      posts.push(body)
    })
    const stop = startPresenceInbound()
    await presenceInboundArmingForTest()
    await reportPresence('route')
    assert.equal(posts.length, 0)
    stop()
  })

  test('enabled merope arms inbound and reports once', async () => {
    resetPresenceInboundForTest()
    setPresenceEnabledForTest(() => true)
    const posts: unknown[] = []
    setPresenceFactsForTest(() => ({ speaking: false }))
    setPresenceCaptureForTest(() => [
      snapshot('presence', 1, 'page visible'),
    ])
    setPresencePostForTest(async (body) => {
      posts.push(body)
    })
    const stop = startPresenceInbound()
    await presenceInboundArmingForTest()
    await Promise.resolve()
    assert.equal(posts.length, 1)
    stop()
  })

  test('page consent off omits page summary from the payload', async () => {
    resetPresenceInboundForTest()
    setPresenceArmedForTest(true)
    setAgentContextConsent(false)
    const posts: unknown[] = []
    setPresenceFactsForTest(() => ({ speaking: false }))
    setPresenceCaptureForTest((input) => {
      if (!input.pageConsent) return [snapshot('presence', 2, 'page visible')]
      return [
        snapshot('page', 3, 'PAGE_BODY_MUST_NOT_SHIP'),
        snapshot('presence', 2, 'page visible'),
      ]
    })
    setPresencePostForTest(async (body) => {
      posts.push(body)
    })
    await reportPresence('page-consent')
    assert.equal(posts.length, 1)
    const body = JSON.stringify(posts[0])
    assert.equal(body.includes('PAGE_BODY_MUST_NOT_SHIP'), false)
    assert.equal(body.includes('"sourceId":"page"'), false)
  })
})

test('inbound reports changes without a timer', () => {
  const source = readFileSync(new URL('./inbound.ts', import.meta.url), 'utf8')
  assert.doesNotMatch(source, /setInterval/)
  assert.match(source, /reportPresence\(/)
  assert.match(source, /visibilitychange/)
  assert.match(source, /meropeEnabled/)
  assert.match(source, /inboundArmed/)
})
