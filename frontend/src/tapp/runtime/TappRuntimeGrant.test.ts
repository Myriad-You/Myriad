/** 已销毁的 grant 不得挡住重铸。 */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { createTestTappKernel } from './kernel.test-support.ts'

describe('TappRuntimeGrant destroy / re-mint seed', () => {
  it('marks destroyed and exposes seed fields for bridge re-mint', () => {
    const { grants } = createTestTappKernel().kernel
    const g = grants.create('com.myriad.aro', 'page_sess1', 'page')
    assert.equal(g.isDestroyed(), false)
    assert.equal(g.getTappId(), 'com.myriad.aro')
    assert.equal(g.getInstanceId(), 'page_sess1')
    assert.equal(g.getKind(), 'page')
    g.destroy()
    assert.equal(g.isDestroyed(), true)
  })

  it('destroyAll kills every live grant; a new instance is live again', () => {
    const { grants } = createTestTappKernel().kernel
    const a = grants.create('com.a', 'i1', 'page')
    const b = grants.create('com.b', 'i2', 'widget')
    grants.destroyAll()
    assert.equal(a.isDestroyed(), true)
    assert.equal(b.isDestroyed(), true)
    const c = grants.create('com.a', 'i1', 'page')
    assert.equal(c.isDestroyed(), false)
    c.destroy()
  })

  it('a rejected token is dropped and replaced by a fresh one', async () => {
    const { grants } = createTestTappKernel().kernel
    const g = grants.create('com.a', 'i1', 'page')
    const first = await g.getToken()
    const replacement = await grants.recoverRejectedToken(first)
    assert.ok(replacement)
    assert.notEqual(replacement, first)
    assert.equal(await grants.recoverRejectedToken('unknown-token'), null)
    g.destroy()
  })

  it('grants of one kernel are not reachable from another', async () => {
    const one = createTestTappKernel().kernel.grants
    const other = createTestTappKernel().kernel.grants
    const g = one.create('com.a', 'i1', 'page')
    const token = await g.getToken()
    assert.equal(await other.recoverRejectedToken(token), null)
    other.destroyAll()
    assert.equal(g.isDestroyed(), false)
    g.destroy()
  })
})
