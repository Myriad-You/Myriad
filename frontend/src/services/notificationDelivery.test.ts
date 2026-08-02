/**
 *   pnpm exec tsx --test src/services/notificationDelivery.test.ts
 */

import type { AppNotification } from './notificationApi.ts'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { notificationSourceFor } from './notificationDelivery.ts'

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
