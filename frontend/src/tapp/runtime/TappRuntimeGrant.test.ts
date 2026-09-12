/** 已销毁的 grant 不得挡住重铸。 */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { TappRuntimeGrant } from './TappRuntimeGrant.ts'

describe('TappRuntimeGrant destroy / re-mint seed', () => {
  it('marks destroyed and exposes seed fields for bridge re-mint', () => {
    const g = new TappRuntimeGrant('com.myriad.aro', 'page_sess1', 'page')
    assert.equal(g.isDestroyed(), false)
    assert.equal(g.getTappId(), 'com.myriad.aro')
    assert.equal(g.getInstanceId(), 'page_sess1')
    assert.equal(g.getKind(), 'page')
    g.destroy()
    assert.equal(g.isDestroyed(), true)
  })

  it('destroyAll kills every live grant; a new instance is live again', () => {
    const a = new TappRuntimeGrant('com.a', 'i1', 'page')
    const b = new TappRuntimeGrant('com.b', 'i2', 'widget')
    TappRuntimeGrant.destroyAll()
    assert.equal(a.isDestroyed(), true)
    assert.equal(b.isDestroyed(), true)
    const c = new TappRuntimeGrant('com.a', 'i1', 'page')
    assert.equal(c.isDestroyed(), false)
    c.destroy()
  })

  it('recoverRejectedToken returns null for destroyed owners', async () => {
    const g = new TappRuntimeGrant('com.x', 'i', 'page')
    const r = await TappRuntimeGrant.recoverRejectedToken('not-a-real-token')
    assert.equal(r, null)
    g.destroy()
    const r2 = await TappRuntimeGrant.recoverRejectedToken('not-a-real-token')
    assert.equal(r2, null)
  })
})
