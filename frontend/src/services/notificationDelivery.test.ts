/**
 *   pnpm exec tsx --test src/services/notificationDelivery.test.ts
 */

import type { AppNotification } from './notificationApi.ts'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  notificationSourceFor,
  shouldDeliverNotification,
  shouldEmitNotificationToast,
  shouldSurfaceNotification,
} from './notificationDelivery.ts'
import { DEFAULT_NOTIFICATION_PREFERENCES } from './notificationPreferencesApi.ts'

function note(
  partial: Partial<AppNotification> &
    Pick<AppNotification, 'notification_type'>,
): AppNotification {
  return {
    id: 'n1',
    title: 't',
    body: 'b',
    created_at: new Date().toISOString(),
    read: false,
    priority: 'normal',
    ...partial,
  } as AppNotification
}

describe('notificationSourceFor', () => {
  it('maps skill.* event keys to agent', () => {
    assert.equal(
      notificationSourceFor(
        note({
          notification_type: 'system_info',
          metadata: { event_key: 'skill.pruned' },
        }),
      ),
      'agent',
    )
    assert.equal(
      notificationSourceFor(
        note({
          notification_type: 'system_info',
          metadata: { event_key: 'skill.improved' },
        }),
      ),
      'agent',
    )
  })

  it('keeps standard source prefixes', () => {
    assert.equal(
      notificationSourceFor(
        note({
          notification_type: 'task_completed',
          metadata: { event_key: 'agent.task_completed' },
        }),
      ),
      'agent',
    )
    assert.equal(
      notificationSourceFor(
        note({
          notification_type: 'heartbeat_result',
          metadata: { event_key: 'heartbeat.succeeded' },
        }),
      ),
      'heartbeat',
    )
  })
})

describe('shouldEmitNotificationToast', () => {
  const task = note({
    notification_type: 'task_completed',
    metadata: { event_key: 'agent.task_completed' },
  })

  it('follows toast delivery when the agent panel is closed', () => {
    assert.equal(
      shouldEmitNotificationToast(
        DEFAULT_NOTIFICATION_PREFERENCES,
        task,
        false,
      ),
      shouldDeliverNotification(
        DEFAULT_NOTIFICATION_PREFERENCES,
        task,
        'toast',
      ),
    )
  })

  it('does not toast while the agent panel is on screen', () => {
    assert.equal(
      shouldEmitNotificationToast(
        DEFAULT_NOTIFICATION_PREFERENCES,
        task,
        true,
      ),
      false,
    )
  })
})

describe('shouldSurfaceNotification', () => {
  const task = note({
    notification_type: 'task_completed',
    metadata: { event_key: 'agent.task_completed' },
  })
  const progress = note({
    notification_type: 'task_progress',
    metadata: { event_key: 'agent.task_progress' },
  })
  const brew = note({
    notification_type: 'brew_new_items',
    metadata: { event_key: 'brew.new_items' },
  })

  it('hides agent notifications on every surface while looking at the panel', () => {
    for (const location of ['panel', 'toast', 'island', 'browser'] as const) {
      assert.equal(
        shouldSurfaceNotification(
          DEFAULT_NOTIFICATION_PREFERENCES,
          task,
          location,
          true,
        ),
        false,
        location,
      )
      assert.equal(
        shouldSurfaceNotification(
          DEFAULT_NOTIFICATION_PREFERENCES,
          progress,
          location,
          true,
        ),
        false,
        location,
      )
    }
  })

  it('still surfaces agent notifications when the panel is closed', () => {
    assert.equal(
      shouldSurfaceNotification(
        DEFAULT_NOTIFICATION_PREFERENCES,
        task,
        'island',
        false,
      ),
      shouldDeliverNotification(
        DEFAULT_NOTIFICATION_PREFERENCES,
        task,
        'island',
      ),
    )
  })

  it('does not hide brew notifications while looking at the agent panel', () => {
    assert.equal(
      shouldSurfaceNotification(
        DEFAULT_NOTIFICATION_PREFERENCES,
        brew,
        'island',
        true,
      ),
      shouldDeliverNotification(
        DEFAULT_NOTIFICATION_PREFERENCES,
        brew,
        'island',
      ),
    )
  })
})
