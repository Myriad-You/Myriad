import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  sharedWidgetInstanceId,
  TappRuntimeGrant,
} from './TappRuntimeGrant.ts'

describe('sharedWidgetInstanceId', () => {
  it('stays within backend MAX_INSTANCE_ID_LENGTH (100) for long tapp ids', () => {
    const longId = `com.${'a'.repeat(120)}.app`
    const instanceId = sharedWidgetInstanceId(longId)
    assert.ok(instanceId.length <= 100, instanceId)
    assert.match(instanceId, /^[\w.-]+$/)
  })

  it('is longer than a short 8-hex hash while remaining BE-safe', () => {
    const id = sharedWidgetInstanceId('com.example.app')
    assert.ok(id.length > 12, `expected longer id, got ${id}`)
    assert.match(id, /^ws\./)
    assert.match(id, /^[\w.-]+$/)
    assert.ok(id.length <= 100)
  })

  it('is stable for the same tapp id', () => {
    assert.equal(
      sharedWidgetInstanceId('com.example.app'),
      sharedWidgetInstanceId('com.example.app'),
    )
  })
})

describe('TappRuntimeGrant.acquireSharedWidget', () => {
  it('shares one grant across acquires and destroys only after last release', () => {
    const a = TappRuntimeGrant.acquireSharedWidget('com.example.multi')
    const b = TappRuntimeGrant.acquireSharedWidget('com.example.multi')
    assert.equal(a.grant, b.grant)
    assert.equal(TappRuntimeGrant.sharedWidgetRefCount('com.example.multi'), 2)
    assert.equal(a.grant.isDestroyed(), false)

    a.release()
    assert.equal(TappRuntimeGrant.sharedWidgetRefCount('com.example.multi'), 1)
    assert.equal(a.grant.isDestroyed(), false)

    b.release()
    assert.equal(TappRuntimeGrant.sharedWidgetRefCount('com.example.multi'), 0)
    assert.equal(a.grant.isDestroyed(), true)
  })

  it('creates a fresh grant after all refs released', () => {
    const a = TappRuntimeGrant.acquireSharedWidget('com.example.again')
    const first = a.grant
    a.release()
    const b = TappRuntimeGrant.acquireSharedWidget('com.example.again')
    assert.notEqual(b.grant, first)
    assert.equal(b.grant.isDestroyed(), false)
    b.release()
  })

  it('destroyAll clears the share table', () => {
    const a = TappRuntimeGrant.acquireSharedWidget('com.example.wipe')
    TappRuntimeGrant.destroyAll()
    assert.equal(a.grant.isDestroyed(), true)
    assert.equal(TappRuntimeGrant.sharedWidgetRefCount('com.example.wipe'), 0)
    const b = TappRuntimeGrant.acquireSharedWidget('com.example.wipe')
    assert.equal(b.grant.isDestroyed(), false)
    b.release()
  })

  it('clearSharedWidgetGrants destroys pool entries without full destroyAll', () => {
    const a = TappRuntimeGrant.acquireSharedWidget('com.example.clear')
    TappRuntimeGrant.clearSharedWidgetGrants()
    assert.equal(a.grant.isDestroyed(), true)
    assert.equal(TappRuntimeGrant.sharedWidgetRefCount('com.example.clear'), 0)
  })
})
