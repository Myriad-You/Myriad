import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { getQuotaManager } from './QuotaManager.ts'

describe('QuotaManager bridge soft limits', () => {
  it('rate-limits lifecycle control-plane signals', () => {
    const q = getQuotaManager()
    const tappId = `quota-test-${Date.now()}`
    let blocked = 0
    for (let i = 0; i < 40; i++) {
      const check = q.checkQuota(tappId, 'lifecycle.ready')
      if (!check.allowed) {
        blocked += 1
        break
      }
      q.recordUsage(tappId, 'lifecycle.ready')
    }
    assert.ok(blocked >= 1, 'lifecycle.ready should hit lifecycle bucket')
  })

  it('applies global bridge.action bucket to platform reads', () => {
    const q = getQuotaManager()
    const tappId = `quota-plat-${Date.now()}`
    for (let i = 0; i < 5; i++) {
      const check = q.checkQuota(tappId, 'platform.getData')
      assert.equal(check.allowed, true)
      q.recordUsage(tappId, 'platform.getData')
    }
    const after = q.checkQuota(tappId, 'platform.getData')
    assert.equal(after.allowed, true)
  })

  it('carves storage.* out of the global bridge.action bucket', () => {
    const q = getQuotaManager()
    const tappId = `quota-storage-${Date.now()}`
    for (let i = 0; i < 180; i++) {
      const check = q.checkQuota(tappId, 'context.getApp')
      if (!check.allowed) break
      q.recordUsage(tappId, 'context.getApp')
    }
    const generic = q.checkQuota(tappId, 'context.getApp')
    assert.equal(generic.allowed, false, 'global bridge bucket should be full')

    const storage = q.checkQuota(tappId, 'storage.get')
    assert.equal(storage.allowed, true, 'storage should be carved out')
    q.recordUsage(tappId, 'storage.get')

    const priv = q.checkQuota(tappId, 'private.get')
    assert.equal(priv.allowed, true, 'private should share the storage bucket')
    const shared = q.checkQuota(tappId, 'shared.set')
    assert.equal(shared.allowed, true, 'shared should share the storage bucket')
    const settings = q.checkQuota(tappId, 'settings.get')
    assert.equal(settings.allowed, true, 'settings should share the storage bucket')

    const theme = q.checkQuota(tappId, 'ui.getTheme')
    assert.equal(theme.allowed, true, 'ui.getTheme should be carved out')
  })

  it('still rate-limits storage under its own generous cap', () => {
    const q = getQuotaManager()
    const tappId = `quota-storage-cap-${Date.now()}`
    let blocked = 0
    for (let i = 0; i < 50; i++) {
      const check = q.checkQuota(tappId, 'storage.set')
      if (!check.allowed) {
        blocked += 1
        break
      }
      q.recordUsage(tappId, 'storage.set')
    }
    assert.equal(blocked, 0, 'storage should allow at least 50/min freely')
  })

  it('counts private/shared/settings against the same storage cap', () => {
    const q = getQuotaManager()
    const tappId = `quota-kv-shared-cap-${Date.now()}`
    for (let i = 0; i < 600; i++) {
      const check = q.checkQuota(tappId, 'private.get')
      assert.equal(check.allowed, true, `private.get #${i} should be in storage bucket`)
      q.recordUsage(tappId, 'private.get')
    }
    const deniedPrivate = q.checkQuota(tappId, 'private.set')
    assert.equal(deniedPrivate.allowed, false, 'private writes share the storage cap')
    const deniedStorage = q.checkQuota(tappId, 'storage.get')
    assert.equal(deniedStorage.allowed, false, 'storage.get shares the cap with private')
    const deniedShared = q.checkQuota(tappId, 'shared.get')
    assert.equal(deniedShared.allowed, false)
    const deniedSettings = q.checkQuota(tappId, 'settings.set')
    assert.equal(deniedSettings.allowed, false)
    const generic = q.checkQuota(tappId, 'context.getApp')
    assert.equal(generic.allowed, true, 'bridge.action bucket stays independent')
  })

  it('returns retryAfter when a dedicated lifecycle bucket is exhausted', () => {
    const q = getQuotaManager()
    const tappId = `quota-retry-${Date.now()}`
    for (let i = 0; i < 30; i++) {
      const check = q.checkQuota(tappId, 'lifecycle.ready')
      assert.equal(check.allowed, true)
      q.recordUsage(tappId, 'lifecycle.ready')
    }
    const denied = q.checkQuota(tappId, 'lifecycle.ready')
    assert.equal(denied.allowed, false)
    assert.ok(
      typeof denied.retryAfter === 'number' && denied.retryAfter >= 0,
      'denied lifecycle check should include retryAfter ms',
    )
  })
})
