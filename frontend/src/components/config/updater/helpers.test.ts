import type { UpdaterStatus } from '../../../services/updaterApi.ts'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  infraCompatibility,
  isDismissedLastFailed,
  isFreshInfraOutcome,
} from './helpers.ts'

describe('isFreshInfraOutcome', () => {
  const last = {
    status: 'succeeded' as const,
    target_tag: 'v0.3.32',
    at: '2026-08-17T02:00:00Z',
  }

  it('ignores a missing or unchanged record', () => {
    assert.equal(isFreshInfraOutcome(null, last.at, ''), null)
    assert.equal(isFreshInfraOutcome(last, last.at, ''), null)
  })

  it('accepts a new terminal outcome when no schedule tag is known', () => {
    assert.equal(
      isFreshInfraOutcome(last, '2026-08-17T01:00:00Z', ''),
      'succeeded',
    )
    assert.equal(
      isFreshInfraOutcome(
        { ...last, status: 'failed' },
        '2026-08-17T01:00:00Z',
        '',
      ),
      'failed',
    )
  })

  it('does not treat the app tip as the updater target', () => {
    assert.equal(
      isFreshInfraOutcome(last, '2026-08-17T01:00:00Z', 'v0.4.0'),
      null,
    )
    assert.equal(
      isFreshInfraOutcome(last, '2026-08-17T01:00:00Z', 'v0.3.32'),
      'succeeded',
    )
  })

  it('keeps polling while the helper is still pending', () => {
    assert.equal(
      isFreshInfraOutcome(
        { ...last, status: 'pending' },
        '2026-08-17T01:00:00Z',
        '',
      ),
      null,
    )
  })
})

describe('infraCompatibility', () => {
  const status: UpdaterStatus = {
    schema_version: 1,
    current_version: 'v0.4.13',
    updater_version: 'v0.4.6',
    channel: 'stable',
    maintenance_active: false,
    maintenance_phase: 'idle',
    job_in_flight: null,
    requires_self_update: false,
    latest_available: {
      version: 'v0.4.14',
      channel: 'stable',
      seen_at: '',
      notes_url: '',
      requires_self_update: false,
      min_updater_version: 'v0.4.5',
    },
  }

  it('allows different compatible application and updater versions', () => {
    assert.deepEqual(infraCompatibility(status), {
      requiresSelfUpdate: false,
      minUpdaterVersion: 'v0.4.5',
    })
  })

  it('uses the explicit minimum instead of the application target for a required update', () => {
    assert.deepEqual(
      infraCompatibility({
        ...status,
        requires_self_update: true,
        latest_available: {
          ...status.latest_available!,
          requires_self_update: true,
          min_updater_version: 'v0.4.7',
        },
      }),
      {
        requiresSelfUpdate: true,
        minUpdaterVersion: 'v0.4.7',
      },
    )
  })

  it('does not infer an update from unknown running versions or missing release information', () => {
    assert.equal(
      infraCompatibility({
        ...status,
        updater_version: '',
      }).requiresSelfUpdate,
      false,
    )
    assert.deepEqual(infraCompatibility(null), {
      requiresSelfUpdate: false,
      minUpdaterVersion: null,
    })
    assert.equal(
      infraCompatibility({
        ...status,
        requires_self_update: true,
        latest_available: null,
      }).requiresSelfUpdate,
      false,
    )
  })
})

describe('isDismissedLastFailed', () => {
  it('does not treat a missing job id as dismissed', () => {
    assert.equal(isDismissedLastFailed(undefined), false)
    assert.equal(isDismissedLastFailed(''), false)
  })
})
