import assert from 'node:assert/strict'
import test from 'node:test'
import { pagePerceptionCopy } from './pageCopy'
import { PerceptionRegistry } from './registry'

test('page body text is not the Lite summary', () => {
  const copy = pagePerceptionCopy(
    {
      type: 'brew_article',
      title: 'Hello',
      plainText: 'This is a long article body that must not enter Lite.',
    },
    '/x',
  )
  assert.equal(copy.summary, 'Hello')
  assert.equal(copy.hasBody, true)
  assert.equal(copy.summary.includes('long article'), false)
  const withSummary = pagePerceptionCopy(
    {
      type: 'brew_article',
      title: 'Hello',
      summary: 'Short take.',
      plainText: 'This is a long article body that must not enter Lite.',
    },
    '/x',
  )
  assert.equal(withSummary.summary, 'Short take.')
})

test('auto-truncated body is not treated as an authored summary', () => {
  const body = 'The harbour was quiet after midnight.'.repeat(8)
  const copy = pagePerceptionCopy(
    {
      type: 'brew_article',
      title: 'Harbour Notes',
      summary: `${body.slice(0, 200)}...`,
      plainText: body,
    },
    '/brew',
  )
  assert.equal(copy.summary, 'Harbour Notes')
  assert.equal(copy.hasBody, true)
})

test('same source replaces instead of appending', () => {
  const registry = new PerceptionRegistry()
  registry.replace({
    sourceId: 'page',
    kind: 'page',
    expiresAt: Date.now() + 10_000,
    summary: 'first',
    safeFacts: { title: 'a' },
    privacy: 'consented',
  })
  registry.replace({
    sourceId: 'page',
    kind: 'page',
    expiresAt: Date.now() + 10_000,
    summary: 'second',
    safeFacts: { title: 'b' },
    privacy: 'consented',
  })
  const live = registry.active()
  assert.equal(live.length, 1)
  assert.equal(live[0]?.summary, 'second')
  assert.equal(live[0]?.revision, 2)
})

test('expired snapshots disappear', () => {
  const registry = new PerceptionRegistry()
  const now = 1_000
  registry.replace({
    sourceId: 'voice',
    kind: 'voice',
    capturedAt: now,
    expiresAt: now + 50,
    summary: 'speaking',
    safeFacts: { speaking: true },
    privacy: 'local',
  })
  assert.equal(registry.active(now + 10).length, 1)
  assert.equal(registry.active(now + 10)[0]?.ttlMs, 40)
  assert.equal(registry.active(now + 50).length, 0)
})

test('kind order is page, pointer, surface, music, voice, presence, screen', () => {
  const registry = new PerceptionRegistry()
  const later = Date.now() + 5_000
  registry.replace({
    sourceId: 'screen',
    kind: 'screen',
    expiresAt: later,
    summary: 's',
    safeFacts: {},
    privacy: 'consented',
  })
  registry.replace({
    sourceId: 'page',
    kind: 'page',
    expiresAt: later,
    summary: 'p',
    safeFacts: {},
    privacy: 'consented',
  })
  registry.replace({
    sourceId: 'music',
    kind: 'music',
    expiresAt: later,
    summary: 'm',
    safeFacts: {},
    privacy: 'system',
  })
  assert.deepEqual(
    registry.active().map((item) => item.kind),
    ['page', 'music', 'screen'],
  )
})
