import type { PerceptionSnapshot } from './registry'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { setAgentContextConsent } from '../../../components/agent-panel/agentContextConsent'
import { setCurrentPageContent } from '../../../contexts/currentPage'
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
    await Promise.resolve()
    await Promise.resolve()
    assert.equal(posts.length, 1)
    stop()
  })

  test('lease renews even when the revision set is unchanged', async () => {
    resetPresenceInboundForTest()
    setPresenceArmedForTest(true)
    const posts: unknown[] = []
    setPresenceFactsForTest(() => ({ speaking: false, pageVisible: true }))
    setPresenceCaptureForTest(() => [
      snapshot('presence', 1, 'page visible'),
    ])
    setPresencePostForTest(async (body) => {
      posts.push(body)
    })
    await reportPresence('route')
    await reportPresence('lease')
    assert.equal(posts.length, 2)
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

  test('renewed revisions of unchanged content do not look like new observations', async () => {
    resetPresenceInboundForTest()
    setPresenceArmedForTest(true)
    let revision = 0
    let posts = 0
    setPresenceFactsForTest(() => ({}))
    setPresenceCaptureForTest(() => [snapshot('presence', ++revision, 'same')])
    setPresencePostForTest(async () => { posts += 1 })
    await reportPresence('route')
    await reportPresence('track')
    assert.equal(posts, 1)
  })

  test('rapid changes deliver the final state after the throttle window', async (t) => {
    resetPresenceInboundForTest()
    t.mock.timers.enable({ apis: ['Date', 'setTimeout'], now: 10_000 })
    setPresenceArmedForTest(true)
    let title = 'first'
    const posts: unknown[] = []
    setPresenceFactsForTest(() => ({}))
    setPresenceCaptureForTest(() => [snapshot('page', 1, title)])
    setPresencePostForTest(async (body) => { posts.push(body) })
    await reportPresence('track')
    title = 'second'
    await reportPresence('track')
    title = 'latest'
    await reportPresence('track')
    assert.equal(posts.length, 1)
    t.mock.timers.tick(2_000)
    for (let i = 0; i < 6; i += 1) await Promise.resolve()
    assert.equal(posts.length, 2)
    assert.match(JSON.stringify(posts[1]), /latest/)
    resetPresenceInboundForTest()
  })

  test('asynchronous page publication and clearing report without another route event', async (t) => {
    resetPresenceInboundForTest()
    t.mock.timers.enable({ apis: ['Date', 'setTimeout'], now: 10_000 })
    setCurrentPageContent(null)
    setAgentContextConsent(true)
    setPresenceEnabledForTest(() => true)
    const posts: unknown[] = []
    let captures = 0
    setPresenceFactsForTest(() => ({}))
    setPresenceCaptureForTest(({ page }) => {
      captures += 1
      return page ? [snapshot('page', 1, page.title!)] : []
    })
    setPresencePostForTest(async (body) => { posts.push(body) })
    const stop = startPresenceInbound()
    try {
      await presenceInboundArmingForTest()
      for (let i = 0; i < 6; i += 1) await Promise.resolve()
      setCurrentPageContent({ type: 'brew_article', title: 'loaded article' })
      t.mock.timers.tick(2_000)
      for (let i = 0; i < 6; i += 1) await Promise.resolve()
      assert.equal(posts.length, 2)
      assert.match(JSON.stringify(posts[1]), /loaded article/)
      setCurrentPageContent(null)
      t.mock.timers.tick(2_000)
      for (let i = 0; i < 6; i += 1) await Promise.resolve()
      assert.equal(posts.length, 3)
      assert.doesNotMatch(JSON.stringify(posts[2]), /loaded article/)
      setAgentContextConsent(false)
      for (let i = 0; i < 6; i += 1) await Promise.resolve()
      setCurrentPageContent({ type: 'brew_article', title: 'not consented' })
      t.mock.timers.tick(2_000)
      for (let i = 0; i < 6; i += 1) await Promise.resolve()
      assert.doesNotMatch(JSON.stringify(posts), /not consented/)
      stop()
      setPresenceArmedForTest(true)
      const before = captures
      setCurrentPageContent(null)
      assert.equal(captures, before, 'stop unsubscribes page publication')
    } finally {
      stop()
      setCurrentPageContent(null)
      setAgentContextConsent(true)
      resetPresenceInboundForTest()
    }
  })

  test('consent revocation bypasses throttling and follows an in-flight post', async () => {
    resetPresenceInboundForTest()
    setPresenceArmedForTest(true)
    setAgentContextConsent(true)
    const posts: unknown[] = []
    let release!: () => void
    setPresenceFactsForTest(() => ({}))
    setPresenceCaptureForTest((input) => input.pageConsent ? [snapshot('page', 1, 'private')] : [])
    setPresencePostForTest(async (body) => {
      posts.push(body)
      if (posts.length === 1) await new Promise<void>((resolve) => { release = resolve })
    })
    const first = reportPresence('route')
    for (let i = 0; i < 6; i += 1) await Promise.resolve()
    setAgentContextConsent(false)
    const revoked = reportPresence('page-consent')
    assert.equal(posts.length, 1)
    release()
    await Promise.all([first, revoked])
    assert.equal(posts.length, 2)
    assert.doesNotMatch(JSON.stringify(posts[1]), /private/)
    resetPresenceInboundForTest()
  })

  test('failed posts retry once without an unbounded background loop', async (t) => {
    resetPresenceInboundForTest()
    t.mock.timers.enable({ apis: ['Date', 'setTimeout'], now: 10_000 })
    setPresenceArmedForTest(true)
    let posts = 0
    setPresenceFactsForTest(() => ({}))
    setPresenceCaptureForTest(() => [snapshot('presence', 1, 'same')])
    setPresencePostForTest(async () => { posts += 1; throw new Error('offline') })
    await reportPresence('route')
    t.mock.timers.tick(2_000)
    for (let i = 0; i < 6; i += 1) await Promise.resolve()
    assert.equal(posts, 2)
    t.mock.timers.tick(10_000)
    for (let i = 0; i < 6; i += 1) await Promise.resolve()
    assert.equal(posts, 2)
    resetPresenceInboundForTest()
  })

  test('stopping inbound cancels its trailing observation', async (t) => {
    resetPresenceInboundForTest()
    t.mock.timers.enable({ apis: ['Date', 'setTimeout'], now: 10_000 })
    setPresenceEnabledForTest(() => true)
    let title = 'first'
    let posts = 0
    setPresenceFactsForTest(() => ({}))
    setPresenceCaptureForTest(() => [snapshot('page', 1, title)])
    setPresencePostForTest(async () => { posts += 1 })
    const stop = startPresenceInbound()
    await presenceInboundArmingForTest()
    for (let i = 0; i < 6; i += 1) await Promise.resolve()
    title = 'new'
    await reportPresence('track')
    stop()
    t.mock.timers.tick(2_000)
    for (let i = 0; i < 6; i += 1) await Promise.resolve()
    assert.equal(posts, 1)
    resetPresenceInboundForTest()
  })

  test('queued observations do not renew content which expired while waiting', async (t) => {
    resetPresenceInboundForTest()
    t.mock.timers.enable({ apis: ['Date', 'setTimeout'], now: 10_000 })
    setPresenceArmedForTest(true)
    const posts: unknown[] = []
    let release!: () => void
    setPresenceFactsForTest(() => ({}))
    setPresenceCaptureForTest(() => [snapshot('page', 1, 'expires-soon')])
    setPresencePostForTest(async (body) => {
      posts.push(body)
      if (posts.length === 1) await new Promise<void>((resolve) => { release = resolve })
    })
    const first = reportPresence('route')
    for (let i = 0; i < 6; i += 1) await Promise.resolve()
    const queued = reportPresence('panel')
    t.mock.timers.tick(1_500)
    release()
    await Promise.all([first, queued])
    assert.equal(posts.length, 2)
    assert.doesNotMatch(JSON.stringify(posts[1]), /expires-soon/)
    resetPresenceInboundForTest()
  })
})

test('inbound reports changes and renews an observation lease while visible', () => {
  const source = readFileSync(new URL('./inbound.ts', import.meta.url), 'utf8')
  assert.match(source, /setInterval/)
  assert.match(source, /reason !== 'lease'/)
  assert.match(source, /subscribeAgentPanelVisible/)
  assert.match(source, /reportPresence\(/)
  assert.match(source, /visibilitychange/)
  assert.match(source, /meropeEnabled/)
  assert.match(source, /inboundArmed/)
  assert.match(source, /subscribeAgentSelection/)
  assert.match(source, /turnSelectionText/)
  assert.match(source, /must not/)
})
