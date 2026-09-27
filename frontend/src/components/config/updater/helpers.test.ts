import type { UpdaterStatus } from '../../../services/updaterApi.ts'
import type { U } from './helpers.ts'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { currentCopy } from '../../../i18n/localeCopy.ts'
import { UpdaterError } from '../../../services/updaterApi.ts'
import {
  explainUpdaterError,
  infraCompatibility,
  isComposeOverrideFailure,
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

describe('explainUpdaterError', () => {
  const u = {
    updaterErr401: 'TOKEN',
    updaterErr401Admin: 'LOGIN',
    updaterErr403: 'OVERRIDE',
    updaterErr403Csrf: 'CSRF',
    updaterErr403Admin: 'ADMIN',
    updaterErr403Generic: 'FORBIDDEN',
    updaterErr409: 'BUSY',
    updaterErr412: 'PRECONDITION',
    updaterErrServer: 'SERVER: {msg}',
    updaterErrNotConfigured: 'NOT_CONFIGURED',
    updaterErrUpstream: 'UPSTREAM',
  } as unknown as U
  const byCode = currentCopy().errors.byCode as Record<string, string>

  it('keeps operator wording for a missing or unreachable updater', () => {
    for (const label of [
      'updater service is not configured on this backend',
      'updater not configured (set MYRIAD_UPDATER_URL)',
    ]) {
      assert.equal(explainUpdaterError(new UpdaterError(503, label), u), 'NOT_CONFIGURED')
    }
    assert.equal(
      explainUpdaterError(new UpdaterError(502, 'updater transport error', 'updater_unreachable'), u),
      'UPSTREAM',
    )
  })

  it('reads other coded failures from the shared table', () => {
    assert.equal(
      explainUpdaterError(
        new UpdaterError(500, 'updater credentials are not a valid HTTP header value', 'updater_misconfigured'),
        u,
      ),
      byCode.updater_misconfigured,
    )
    assert.equal(
      explainUpdaterError(
        new UpdaterError(400, 'allow_skip_versions is not supported', 'allow_skip_versions_unsupported'),
        u,
      ),
      byCode.allow_skip_versions_unsupported,
    )
  })

  it('keeps status wording for uncoded updater replies', () => {
    assert.equal(explainUpdaterError(new UpdaterError(401, 'Please login'), u), 'LOGIN')
    assert.equal(explainUpdaterError(new UpdaterError(401, 'bad token'), u), 'TOKEN')
    assert.equal(explainUpdaterError(new UpdaterError(403, 'CSRF token missing'), u), 'CSRF')
    assert.equal(explainUpdaterError(new UpdaterError(403, 'manual-override required'), u), 'OVERRIDE')
    assert.equal(explainUpdaterError(new UpdaterError(409, 'updater upstream 409 Conflict'), u), 'BUSY')
    assert.equal(explainUpdaterError(new UpdaterError(503, 'backend cannot authenticate'), u), 'UPSTREAM')
  })
})

describe('isComposeOverrideFailure', () => {
  const reason =
    'preflight: precondition failed: the deployment compose will be overwritten; ' +
    're-submit with allow_compose_override=true (or allow_risk=true)'

  it('accepts structured codes and the exact legacy reason without a code', () => {
    assert.equal(
      isComposeOverrideFailure({
        code: 'compose_override_required',
        reason: 'localized',
      }),
      true,
    )
    assert.equal(isComposeOverrideFailure({ reason }), true)
    assert.equal(isComposeOverrideFailure({ code: null, reason }), true)
    assert.equal(
      isComposeOverrideFailure({ reason: reason.replace('preflight: ', '') }),
      true,
    )
  })

  it('does not reinterpret another code or unrelated compose failures as consent', () => {
    assert.equal(isComposeOverrideFailure(null), false)
    assert.equal(
      isComposeOverrideFailure({ code: 'another_error', reason }),
      false,
    )
    assert.equal(
      isComposeOverrideFailure({ reason: 'compose mount permission denied' }),
      false,
    )
    assert.equal(
      isComposeOverrideFailure({ reason: `pull failed: ${reason}` }),
      false,
    )
    assert.equal(
      isComposeOverrideFailure({ reason: 'allow_compose_override=true' }),
      false,
    )
  })
})
