/* eslint-disable test/no-import-node-test -- node:test is the repository test runner */
/**
 * Unit tests for delivery-queue UI classification helpers.
 * Mirrors backend is_user_cancelled_delivery_error / should_offer_retry_for_dead_error.
 */
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  isCancelledDeliveryError,
  shouldOfferDeliveryRetry,
} from './federationDeliveryUi'

describe('isCancelledDeliveryError', () => {
  it('matches cancelled: prefix (case-insensitive)', () => {
    assert.equal(isCancelledDeliveryError('cancelled: by user'), true)
    assert.equal(
      isCancelledDeliveryError('cancelled: local room dissolved'),
      true,
    )
    assert.equal(
      isCancelledDeliveryError('cancelled: local channel closed'),
      true,
    )
    assert.equal(isCancelledDeliveryError('CANCELLED: suite'), true)
  })

  it('rejects peer failures and mid-string cancelled', () => {
    assert.equal(isCancelledDeliveryError('HTTP 500: boom'), false)
    assert.equal(isCancelledDeliveryError('suite seeded dead'), false)
    assert.equal(
      isCancelledDeliveryError('remote said: cancelled by policy'),
      false,
    )
    assert.equal(isCancelledDeliveryError(null), false)
    assert.equal(isCancelledDeliveryError(undefined), false)
    assert.equal(isCancelledDeliveryError(''), false)
  })
})

describe('shouldOfferDeliveryRetry', () => {
  it('hides retry for intentional dead cancels', () => {
    assert.equal(
      shouldOfferDeliveryRetry({
        status: 'dead',
        error_message: 'cancelled: local room dissolved',
      }),
      false,
    )
    assert.equal(
      shouldOfferDeliveryRetry({
        status: 'dead',
        error_message: 'cancelled: by user',
      }),
      false,
    )
    assert.equal(
      shouldOfferDeliveryRetry({
        status: 'dead',
        intentional_cancel: true,
        error_message: null,
      }),
      false,
    )
  })

  it('allows retry for real dead and pending', () => {
    assert.equal(
      shouldOfferDeliveryRetry({
        status: 'dead',
        error_message: 'PERMANENT HTTP 401',
      }),
      true,
    )
    assert.equal(
      shouldOfferDeliveryRetry({ status: 'pending', error_message: null }),
      true,
    )
    assert.equal(
      shouldOfferDeliveryRetry({ status: 'delivering', error_message: null }),
      false,
    )
  })

  it('respects server retryable flag when present', () => {
    assert.equal(
      shouldOfferDeliveryRetry({
        status: 'dead',
        retryable: false,
        error_message: 'HTTP 500',
      }),
      false,
    )
    assert.equal(
      shouldOfferDeliveryRetry({
        status: 'dead',
        retryable: true,
        error_message: 'cancelled: by user',
      }),
      true,
    )
  })
})
