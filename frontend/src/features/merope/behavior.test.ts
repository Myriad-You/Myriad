import assert from 'node:assert/strict'
import test from 'node:test'
import {
  meropeExpansionGenerationIsCurrent,
  meropePollInterval,
  deferMeropeExpansionGesture,
  isMeropeOverlayReady,
  latestUnreadProactive,
  shouldAnimateMeropeUnread,
  shouldMountMerope,
  snapshotChanged,
} from './behavior'

test('feature gate requires checked auth, login, and strict true flag', () => {
  assert.equal(shouldMountMerope(true, true, true), true)
  assert.equal(shouldMountMerope(false, true, true), false)
  assert.equal(shouldMountMerope(true, false, true), false)
  assert.equal(shouldMountMerope(true, true, 'true'), false)
  assert.equal(shouldMountMerope(true, true, undefined), false)
})

test('polling backs off while the document is hidden', () => {
  assert.equal(meropePollInterval(false), 30_000)
  assert.equal(meropePollInterval(true), 120_000)
})

test('fingerprint suppresses unchanged snapshot replacement', () => {
  const snapshot = { fingerprint: 'same' } as never
  assert.equal(snapshotChanged(snapshot, snapshot), false)
  assert.equal(
    snapshotChanged(snapshot, { fingerprint: 'next' } as never),
    true,
  )
})

test('latest unread proactive message drives the collapsed bubble', () => {
  const messages = [
    { id: 1, role: 'proactive', meta: {}, content: 'first' },
    { id: 2, role: 'assistant', meta: {}, content: 'reply' },
    { id: 3, role: 'proactive', meta: {}, content: 'latest' },
  ] as never
  assert.equal(latestUnreadProactive(messages)?.content, 'latest')
  assert.equal(
    latestUnreadProactive([
      { id: 1, role: 'proactive', meta: { readAt: 'now' } },
    ] as never),
    null,
  )
})

test('overlay chat requires a finished persona onboarding character', () => {
  assert.equal(isMeropeOverlayReady(false, undefined), false)
  assert.equal(isMeropeOverlayReady(true, false), false)
  assert.equal(isMeropeOverlayReady(true, undefined), false)
  assert.equal(isMeropeOverlayReady(true, true), true)
})

test('overlay expansion paints a phase-continuous pose before starting its greeting', () => {
  const frames = new Map<number, FrameRequestCallback>()
  let nextHandle = 1
  let greeted = false
  const cancel = deferMeropeExpansionGesture(
    () => {
      greeted = true
    },
    (callback) => {
      const handle = nextHandle++
      frames.set(handle, callback)
      return handle
    },
    (handle) => frames.delete(handle),
  )

  const first = frames.get(1)
  frames.delete(1)
  first?.(16)
  assert.equal(greeted, false)
  const second = frames.get(2)
  frames.delete(2)
  second?.(32)
  assert.equal(greeted, true)
  cancel()
  assert.equal(frames.size, 0)
})

test('a collapsed-again overlay cancels its pending expansion greeting', () => {
  const frames = new Map<number, FrameRequestCallback>()
  let nextHandle = 1
  let greeted = false
  const cancel = deferMeropeExpansionGesture(
    () => {
      greeted = true
    },
    (callback) => {
      const handle = nextHandle++
      frames.set(handle, callback)
      return handle
    },
    (handle) => frames.delete(handle),
  )
  cancel()
  frames.get(1)?.(16)
  assert.equal(greeted, false)
})

test('collapsing after the retained frame cancels the second-frame greeting', () => {
  const frames = new Map<number, FrameRequestCallback>()
  let nextHandle = 1
  let greeted = false
  const cancel = deferMeropeExpansionGesture(
    () => {
      greeted = true
    },
    (callback) => {
      const handle = nextHandle++
      frames.set(handle, callback)
      return handle
    },
    (handle) => frames.delete(handle),
  )

  const first = frames.get(1)
  frames.delete(1)
  first?.(16)
  assert.ok(frames.has(2))
  cancel()
  frames.get(2)?.(32)
  assert.equal(greeted, false)
  assert.equal(frames.size, 0)
})

test('overlay expansion callbacks belong only to the latest open generation', () => {
  assert.equal(meropeExpansionGenerationIsCurrent(4, 4, false), true)
  assert.equal(meropeExpansionGenerationIsCurrent(3, 4, false), false)
  assert.equal(meropeExpansionGenerationIsCurrent(4, 4, true), false)
})

test('collapsed unread refreshes update state without starting a rig animation', () => {
  let previousUnreadCount = 2
  const refreshedUnreadCount = 3
  assert.equal(
    shouldAnimateMeropeUnread(
      false,
      previousUnreadCount,
      refreshedUnreadCount,
    ),
    false,
  )
  previousUnreadCount = refreshedUnreadCount
  assert.equal(
    shouldAnimateMeropeUnread(true, previousUnreadCount, 3),
    false,
    'opening later does not replay a notification already observed collapsed',
  )
  assert.equal(shouldAnimateMeropeUnread(true, 2, 3), true)
  assert.equal(shouldAnimateMeropeUnread(true, 3, 3), false)
  assert.equal(meropePollInterval(false), 30_000)
  assert.equal(meropePollInterval(true), 120_000)
})
