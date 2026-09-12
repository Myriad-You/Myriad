import type { PreparedDataExchange } from '../services/TappApiService'
import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import {
  decideDataExchangeConsent,
  getDataExchangeConsentSnapshot,
  requestDataExchangeConsent,
} from './DataExchangeConsent.ts'

function prepared(
  requestId: string,
  expiresAt = new Date(Date.now() + 60_000).toISOString(),
): PreparedDataExchange {
  return {
    requestId,
    requesterTappId: 'com.requester.app',
    requesterName: 'Requester',
    providerTappId: 'com.provider.app',
    providerOwnerId: 1,
    providerName: 'Provider',
    exportId: 'posts',
    params: null,
    purpose: 'read posts',
    maxBytes: 1024,
    expiresAt,
  }
}

function drainQueue() {
  for (;;) {
    const current = getDataExchangeConsentSnapshot().current
    if (!current) return
    decideDataExchangeConsent(current.prepared.requestId, false)
  }
}

afterEach(() => {
  drainQueue()
})

describe('DataExchangeConsent', () => {
  it('resolves expired when the grant already elapsed', async () => {
    assert.equal(
      await requestDataExchangeConsent(
        prepared('expired-1', new Date(Date.now() - 1000).toISOString()),
      ),
      'expired',
    )
    assert.equal(getDataExchangeConsentSnapshot().current, null)
  })

  it('cancels duplicate in-flight request ids', async () => {
    const first = requestDataExchangeConsent(prepared('dup-1'))
    assert.equal(await requestDataExchangeConsent(prepared('dup-1')), 'cancelled')
    assert.equal(decideDataExchangeConsent('dup-1', true), true)
    assert.equal(await first, 'allow')
  })

  it('cancels when the abort signal fires', async () => {
    const controller = new AbortController()
    const pending = requestDataExchangeConsent(prepared('abort-1'), controller.signal)
    controller.abort()
    assert.equal(await pending, 'cancelled')
    assert.equal(getDataExchangeConsentSnapshot().current, null)
  })

  it('only the front of the queue can be decided', async () => {
    const first = requestDataExchangeConsent(prepared('q-1'))
    const second = requestDataExchangeConsent(prepared('q-2'))
    assert.equal(decideDataExchangeConsent('q-2', true), false)
    assert.equal(decideDataExchangeConsent('q-1', false), true)
    assert.equal(await first, 'deny')
    assert.equal(decideDataExchangeConsent('q-2', true), true)
    assert.equal(await second, 'allow')
  })
})
