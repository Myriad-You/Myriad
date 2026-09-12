import type { LastFailedUpdate } from '../../../services/updaterApi'
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { tagInstallRetryOptions } from './tagInstallRetry'

const failure: LastFailedUpdate = {
  at: '2026-09-12T00:00:00Z', job_id: 'job', reason: 'manifest unavailable',
  to_version: 'v1.2.3', confirmation_required: true,
  trust: { trust_path: 'dockerhub_tag', verification: 'unsigned' },
}

test('explicit tag retry does not grant unrelated risk exceptions', () => {
  const retry = tagInstallRetryOptions(failure)!
  assert.equal(retry.target, 'v1.2.3')
  assert.deepEqual(retry.opts, {
    mode: 'release', allowTagInstall: true, allowRisk: false,
    allowDowngrade: false, allowDiverged: false, allowUnknown: false, allowIrreversible: false,
  })
})

test('unsigned commit retries keep the resolved SHA and prior confirmed flags', () => {
  const retry = tagInstallRetryOptions({ ...failure, to_version: 'dev-abcdef0',
    trust: { trust_path: 'dockerhub_commit', verification: 'unsigned' },
    risk_flags: { allow_downgrade: true, allow_unknown: true },
  })!
  assert.equal(retry.target, 'dev-abcdef0')
  assert.equal(retry.opts.mode, 'commit')
  assert.equal(retry.opts.allowDowngrade, true)
  assert.equal(retry.opts.allowUnknown, true)
  assert.equal(retry.opts.allowRisk, false)
  assert.equal(retry.opts.allowIrreversible, false)
})

test('legacy errors and signature failures never offer a tag retry', () => {
  for (const patch of [
    { confirmation_required: false }, { confirmation_required: undefined },
    { to_version: undefined }, { trust: undefined },
    { trust: { trust_path: 'github_release' as const, verification: 'pending' as const } },
    { trust: { trust_path: 'signed_commit' as const, verification: 'pending' as const } },
  ]) assert.equal(tagInstallRetryOptions({ ...failure, ...patch }), null)
})
