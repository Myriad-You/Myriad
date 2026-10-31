import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { createTestTappKernel } from './kernel.test-support.ts'
import { newSharedWidgetInstanceId } from './TappRuntimeGrant.ts'

describe('newSharedWidgetInstanceId', () => {
  it('stays within backend MAX_INSTANCE_ID_LENGTH (100)', () => {
    const instanceId = newSharedWidgetInstanceId()
    assert.ok(instanceId.length <= 100, instanceId)
    assert.match(instanceId, /^[\w.-]+$/)
    assert.match(instanceId, /^ws\./)
  })

  it('is unique per call so tabs do not share an instance identity', () => {
    assert.notEqual(newSharedWidgetInstanceId(), newSharedWidgetInstanceId())
  })
})

describe('TappGrantRegistry.acquireSharedWidget', () => {
  it('shares one grant across acquires and destroys only after last release', () => {
    const grants = createTestTappKernel().kernel.grants
    const a = grants.acquireSharedWidget('com.example.multi')
    const b = grants.acquireSharedWidget('com.example.multi')
    assert.equal(a.grant, b.grant)
    assert.equal(a.grant.getInstanceId(), b.grant.getInstanceId())
    assert.equal(grants.sharedWidgetRefCount('com.example.multi'), 2)
    assert.equal(a.grant.isDestroyed(), false)

    a.release()
    assert.equal(grants.sharedWidgetRefCount('com.example.multi'), 1)
    assert.equal(a.grant.isDestroyed(), false)

    b.release()
    assert.equal(grants.sharedWidgetRefCount('com.example.multi'), 0)
    assert.equal(a.grant.isDestroyed(), true)
  })

  it('creates a fresh grant after all refs released', () => {
    const grants = createTestTappKernel().kernel.grants
    const a = grants.acquireSharedWidget('com.example.again')
    const first = a.grant
    a.release()
    const b = grants.acquireSharedWidget('com.example.again')
    assert.notEqual(b.grant, first)
    assert.equal(b.grant.isDestroyed(), false)
    b.release()
  })

  it('destroyAll clears the share table', () => {
    const grants = createTestTappKernel().kernel.grants
    const a = grants.acquireSharedWidget('com.example.wipe')
    grants.destroyAll()
    assert.equal(a.grant.isDestroyed(), true)
    assert.equal(grants.sharedWidgetRefCount('com.example.wipe'), 0)
    const b = grants.acquireSharedWidget('com.example.wipe')
    assert.equal(b.grant.isDestroyed(), false)
    b.release()
  })

  it('clearSharedWidgetGrants destroys pool entries without full destroyAll', () => {
    const grants = createTestTappKernel().kernel.grants
    const a = grants.acquireSharedWidget('com.example.clear')
    grants.clearSharedWidgetGrants()
    assert.equal(a.grant.isDestroyed(), true)
    assert.equal(grants.sharedWidgetRefCount('com.example.clear'), 0)
  })
})
