import type { PerceptionSnapshot } from './registry'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { setAgentContextConsent } from '../../../components/agent-panel/agentContextConsent'
import { currentPagePublisher, setCurrentPageContent } from '../../../contexts/currentPage'
import { authSubject } from '../../../utils/authSubject'
import { PERSONA_UPDATED_EVENT } from '../events'
import { livePresenceFacts } from '../livePresence'
import { getProductionMotionRuntime } from '../motion/runtimeHost'
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
  test('real capture cannot renew the old subject page after identity loss or late publication', async () => {
    resetPresenceInboundForTest()
    setAgentContextConsent(true)
    setPresenceArmedForTest(true)
    setPresenceFactsForTest(() => ({}))
    const posts: unknown[] = []
    setPresencePostForTest(async body => { posts.push(body) })
    const publishA = currentPagePublisher()
    try {
      publishA({ type: 'custom', title: 'A-private-title', summary: 'A-private-summary' })
      await reportPresence('lease')
      assert.match(JSON.stringify(posts[0]), /A-private-summary/)
      authSubject.change('B', true)
      publishA({ type: 'custom', title: 'A-late-private' })
      await reportPresence('lease')
      assert.doesNotMatch(JSON.stringify(posts[1]), /A-private|A-late/)
      currentPagePublisher()({ type: 'custom', title: 'B-current' })
      await reportPresence('lease')
      assert.match(JSON.stringify(posts[2]), /B-current/)
    } finally {
      resetPresenceInboundForTest()
      authSubject.change('guest', true)
    }
  })

  test('identity changes abort transport and discard queued snapshots without blocking the new subject', async () => {
    resetPresenceInboundForTest()
    authSubject.change('A', true)
    setPresenceArmedForTest(true)
    const held = Promise.withResolvers<void>()
    const entered = Promise.withResolvers<void>()
    const posts: unknown[] = []
    const signals: AbortSignal[] = []
    let summary = 'A-private'
    setPresenceCaptureForTest(() => [snapshot('page', 1, summary)])
    setPresenceFactsForTest(() => ({}))
    setPresencePostForTest(async (body, signal) => {
      posts.push(body)
      signals.push(signal)
      if (posts.length === 1) { entered.resolve(); await held.promise }
    })
    try {
      const first = reportPresence('lease')
      await entered.promise
      const queued = reportPresence('lease')
      for (let i = 0; i < 6; i++) await Promise.resolve()
      authSubject.change('B')
      assert.equal(signals[0].aborted, true)
      summary = 'B-current'
      await reportPresence('route')
      assert.equal(posts.length, 2)
      assert.match(JSON.stringify(posts[1]), /B-current/)
      held.resolve()
      await Promise.all([first, queued])
      assert.equal(posts.length, 2)
    } finally {
      held.resolve()
      resetPresenceInboundForTest()
      authSubject.change('guest', true)
    }
  })

  test('identity change during asynchronous capture discards the old observation', async () => {
    resetPresenceInboundForTest()
    setPresenceArmedForTest(true)
    const held = Promise.withResolvers<PerceptionSnapshot[]>()
    let posts = 0
    setPresenceCaptureForTest(() => held.promise)
    setPresenceFactsForTest(() => ({}))
    setPresencePostForTest(async () => { posts++ })
    try {
      const pending = reportPresence('lease')
      authSubject.change('B', true)
      held.resolve([snapshot('page', 1, 'A-private')])
      await pending
      assert.equal(posts, 0)
    } finally {
      resetPresenceInboundForTest()
      authSubject.change('guest', true)
    }
  })

  test('identity change cancels trailing reports and stale failure cannot schedule a retry', async (t) => {
    resetPresenceInboundForTest()
    t.mock.timers.enable({ apis: ['Date', 'setTimeout'], now: 10_000 })
    setPresenceArmedForTest(true)
    const held = Promise.withResolvers<void>()
    const entered = Promise.withResolvers<void>()
    let summary = 'first'
    let posts = 0
    setPresenceCaptureForTest(() => [snapshot('page', 1, summary)])
    setPresenceFactsForTest(() => ({}))
    setPresencePostForTest(async () => {
      posts++
      if (posts === 2) { entered.resolve(); await held.promise; throw new Error('old failure') }
    })
    try {
      await reportPresence('route')
      summary = 'throttled'
      await reportPresence('route')
      authSubject.change('B', true)
      t.mock.timers.tick(2_000)
      for (let i = 0; i < 6; i++) await Promise.resolve()
      assert.equal(posts, 1)
      const pending = reportPresence('lease')
      await entered.promise
      authSubject.change('C')
      held.resolve()
      await pending
      t.mock.timers.tick(10_000)
      for (let i = 0; i < 6; i++) await Promise.resolve()
      assert.equal(posts, 2)
    } finally {
      held.resolve()
      resetPresenceInboundForTest()
      authSubject.change('guest', true)
    }
  })

  test('production presence carries current semantic rig state, without raw drivers', async () => {
    resetPresenceInboundForTest()
    setPresenceArmedForTest(true)
    const runtime = getProductionMotionRuntime()
    const posts: any[] = []
    setPresenceCaptureForTest(() => [])
    setPresenceFactsForTest(livePresenceFacts)
    setPresencePostForTest(async body => { posts.push(body) })
    try {
      runtime.setCapabilities(['head-body', 'cry-eye'])
      await reportPresence('panel')
      assert.deepEqual(posts[0].presence.rigState.capabilities, ['head-body', 'cry-eye'])
      runtime.setCapabilities(['head-body'])
      await reportPresence('panel')
      assert.deepEqual(posts[1].presence.rigState.capabilities, ['head-body'])
      assert.doesNotMatch(JSON.stringify(posts), /"(?:driver|vertices|angles)"/)
    } finally {
      runtime.setCapabilities([])
      resetPresenceInboundForTest()
    }
  })

  test('persona changes arm, disarm and rearm without remounting; stale config cannot revive it', async () => {
    resetPresenceInboundForTest()
    const original = Object.getOwnPropertyDescriptor(globalThis, 'window')
    const events = new EventTarget()
    Object.defineProperty(globalThis, 'window', { configurable: true, value: events })
    let enabled = false
    let posts = 0
    setPresenceEnabledForTest(() => enabled)
    setPresenceCaptureForTest(() => [])
    setPresenceFactsForTest(() => ({}))
    setPresencePostForTest(async () => { posts++ })
    const stop = startPresenceInbound()
    const flush = () => new Promise(resolve => setImmediate(resolve))
    try {
      await presenceInboundArmingForTest()
      assert.equal(posts, 0)
      enabled = true
      events.dispatchEvent(new Event(PERSONA_UPDATED_EVENT))
      await presenceInboundArmingForTest()
      await flush()
      assert.equal(posts, 1)
      const enabledGate = Promise.withResolvers<boolean>()
      setPresenceEnabledForTest(() => enabledGate.promise)
      events.dispatchEvent(new Event(PERSONA_UPDATED_EVENT))
      const stale = presenceInboundArmingForTest()
      setPresenceEnabledForTest(() => false)
      events.dispatchEvent(new Event(PERSONA_UPDATED_EVENT))
      await presenceInboundArmingForTest()
      enabledGate.resolve(true)
      await stale
      await reportPresence('lease')
      assert.equal(posts, 1)
      setPresenceEnabledForTest(() => true)
      events.dispatchEvent(new Event(PERSONA_UPDATED_EVENT))
      await presenceInboundArmingForTest()
      await flush()
      assert.equal(posts, 2)
      stop()
      events.dispatchEvent(new Event(PERSONA_UPDATED_EVENT))
      await reportPresence('lease')
      assert.equal(posts, 2)
    } finally {
      stop()
      if (original) Object.defineProperty(globalThis, 'window', original)
      else Reflect.deleteProperty(globalThis, 'window')
      resetPresenceInboundForTest()
    }
  })

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
    const { promise: held, resolve: release } = Promise.withResolvers<void>()
    setPresenceFactsForTest(() => ({}))
    setPresenceCaptureForTest((input) => input.pageConsent ? [snapshot('page', 1, 'private')] : [])
    setPresencePostForTest(async (body) => {
      posts.push(body)
      if (posts.length === 1) await held
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
    const { promise: held, resolve: release } = Promise.withResolvers<void>()
    setPresenceFactsForTest(() => ({}))
    setPresenceCaptureForTest(() => [snapshot('page', 1, 'expires-soon')])
    setPresencePostForTest(async (body) => {
      posts.push(body)
      if (posts.length === 1) await held
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
  assert.match(source, /post\('\/agent\/presence'/)
  assert.doesNotMatch(source, /from ['"][^'"]*consciousness/)
})
