/**
 * Unit tests for updater → maintenance page auto-navigation helpers.
 *
 * Run from frontend/:
 *   pnpm exec tsx --test src/components/config/updaterMaintenanceNav.test.ts
 */

/* eslint-disable test/no-import-node-test -- node:test; project has no vitest dep */

import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import {
  hasNavigatedForJob,
  isLikelyMaintenanceHtml,
  MAINT_NAV_STORAGE_KEY,
  markNavigatedForJob,
  parseProxyStatus,
  shouldNavigateToMaintenance,
  startMaintenancePoll,
} from './updaterMaintenanceNav'

describe('parseProxyStatus', () => {
  it('parses active maintenance', () => {
    assert.deepEqual(
      parseProxyStatus({
        schema_version: 1,
        maintenance: { active: true, phase: 'pulling' },
      }),
      { active: true, phase: 'pulling' },
    )
  })

  it('parses inactive without phase', () => {
    assert.deepEqual(
      parseProxyStatus({ maintenance: { active: false } }),
      { active: false, phase: undefined },
    )
  })

  it('returns null for bad shapes', () => {
    assert.equal(parseProxyStatus(null), null)
    assert.equal(parseProxyStatus({}), null)
    assert.equal(parseProxyStatus({ maintenance: { active: 'yes' } }), null)
  })
})

describe('isLikelyMaintenanceHtml', () => {
  it('detects HTML maintenance pages', () => {
    assert.equal(
      isLikelyMaintenanceHtml('<!DOCTYPE html><html>maintenance</html>'),
      true,
    )
    assert.equal(isLikelyMaintenanceHtml('{"error":"busy"}'), false)
  })
})

describe('shouldNavigateToMaintenance', () => {
  afterEach(() => {
    try {
      sessionStorage.removeItem(MAINT_NAV_STORAGE_KEY)
    } catch {
      /* ignore */
    }
  })

  it('requires a job id', () => {
    assert.equal(
      shouldNavigateToMaintenance({
        jobId: '',
        proxyActive: true,
        alreadyNavigated: false,
      }),
      false,
    )
  })

  it('navigates on proxy active, status active, or non-JSON 503', () => {
    assert.equal(
      shouldNavigateToMaintenance({
        jobId: 'j1',
        proxyActive: true,
        alreadyNavigated: false,
      }),
      true,
    )
    assert.equal(
      shouldNavigateToMaintenance({
        jobId: 'j1',
        statusMaintenanceActive: true,
        alreadyNavigated: false,
      }),
      true,
    )
    assert.equal(
      shouldNavigateToMaintenance({
        jobId: 'j1',
        nonJson503DuringJob: true,
        alreadyNavigated: false,
      }),
      true,
    )
  })

  it('does not navigate when nothing signals maintenance', () => {
    assert.equal(
      shouldNavigateToMaintenance({
        jobId: 'j1',
        proxyActive: false,
        statusMaintenanceActive: false,
        alreadyNavigated: false,
      }),
      false,
    )
  })

  it('guards once per job via alreadyNavigated / sessionStorage', () => {
    assert.equal(
      shouldNavigateToMaintenance({
        jobId: 'j1',
        proxyActive: true,
        alreadyNavigated: true,
      }),
      false,
    )
    markNavigatedForJob('j1')
    assert.equal(hasNavigatedForJob('j1'), true)
    assert.equal(
      shouldNavigateToMaintenance({
        jobId: 'j1',
        proxyActive: true,
      }),
      false,
    )
    assert.equal(
      shouldNavigateToMaintenance({
        jobId: 'j2',
        proxyActive: true,
      }),
      true,
    )
  })
})

describe('startMaintenancePoll', () => {
  afterEach(() => {
    try {
      sessionStorage.removeItem(MAINT_NAV_STORAGE_KEY)
    } catch {
      /* ignore */
    }
  })

  it('navigates when proxy reports active', async () => {
    let navigated = false
    const stop = startMaintenancePoll({
      jobId: 'job-a',
      intervalMs: 10,
      fetchImpl: async () =>
        new Response(
          JSON.stringify({ maintenance: { active: true, phase: 'on' } }),
          {
            status: 200,
            headers: { 'content-type': 'application/json' },
          },
        ),
      onNavigate: () => {
        navigated = true
      },
    })

    await new Promise((r) => setTimeout(r, 30))
    stop()
    assert.equal(navigated, true)
  })

  it('falls back to status.maintenance_active when proxy status missing', async () => {
    let navigated = false
    let statusActive = false
    const stop = startMaintenancePoll({
      jobId: 'job-b',
      intervalMs: 10,
      getStatusMaintenanceActive: () => statusActive,
      fetchImpl: async () => new Response('not found', { status: 404 }),
      onNavigate: () => {
        navigated = true
      },
    })

    await new Promise((r) => setTimeout(r, 25))
    assert.equal(navigated, false)
    statusActive = true
    await new Promise((r) => setTimeout(r, 40))
    stop()
    assert.equal(navigated, true)
  })

  it('stop() prevents further navigation', async () => {
    let navigated = false
    let calls = 0
    const stop = startMaintenancePoll({
      jobId: 'job-c',
      intervalMs: 10,
      fetchImpl: async () => {
        calls++
        return new Response(
          JSON.stringify({ maintenance: { active: false } }),
          {
            status: 200,
            headers: { 'content-type': 'application/json' },
          },
        )
      },
      onNavigate: () => {
        navigated = true
      },
    })

    await new Promise((r) => setTimeout(r, 20))
    stop()
    const callsAfterStop = calls
    await new Promise((r) => setTimeout(r, 40))
    assert.equal(navigated, false)
    assert.ok(callsAfterStop >= 1)
    assert.equal(calls, callsAfterStop)
  })
})
