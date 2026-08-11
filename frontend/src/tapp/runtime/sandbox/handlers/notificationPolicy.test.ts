import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  canMutateDynamicContent,
  prepareTappNotification,
} from './notificationPolicy'

describe('TAPP notification delivery policy', () => {
  it('uses a bounded session-only toast for guests', () => {
    const prepared = prepareTappNotification(
      {
        title: 't'.repeat(200),
        message: 'm'.repeat(2000),
        type: 'success',
      },
      'guest',
    )

    assert.deepEqual(prepared, {
      delivery: 'session',
      title: 't'.repeat(120),
      message: 'm'.repeat(1000),
      type: 'success',
    })
    assert.equal(canMutateDynamicContent('guest'), false)
  })

  it('preserves durable notifications for authenticated roles', () => {
    assert.equal(
      prepareTappNotification({ message: 'ready', type: 'invalid' as 'info' }, 'user')
        .delivery,
      'durable',
    )
    assert.equal(canMutateDynamicContent('user'), true)
    assert.equal(canMutateDynamicContent('admin'), true)
  })
})
