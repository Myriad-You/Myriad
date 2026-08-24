import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import type { AppNotification } from '../services/notificationApi'
import {
  notificationFacingBody,
  notificationFacingTitle,
} from './notificationFacing.ts'

function notice(
  title: string,
  body: string,
  event_key?: string,
  extra?: Record<string, unknown>,
): AppNotification {
  return {
    id: 'n1',
    notification_type: 'system_info',
    priority: 'high',
    title,
    body,
    user_id: 1,
    created_at: new Date().toISOString(),
    read: false,
    metadata: event_key ? { event_key, ...extra } : extra,
  }
}

describe('notificationFacing', () => {
  it('maps leftover brew Chinese titles', () => {
    const text = notificationFacingTitle(
      notice('科技新闻 连续抓取失败', 'timeout', 'brew.source_error', {
        source_name: '科技新闻',
      }),
    )
    assert.equal(text.includes('连续抓取失败'), false)
    assert.match(text, /科技新闻/)
  })

  it('strips leftover platform dumps from the body', () => {
    const text = notificationFacingBody(
      notice('Steam auto-refresh failed', '获取失败: error sending request'),
    )
    assert.equal(/error sending request/i.test(text), false)
  })

  it('maps leftover federation unlink Chinese', () => {
    const text = notificationFacingTitle(
      notice('联邦关系已解除', 'example.com 长期无法送达', 'federation.domain_revoked', {
        target_domain: 'example.com',
      }),
    )
    assert.equal(text.includes('联邦关系已解除'), false)
    assert.match(text, /example\.com/)
  })

  it('maps leftover follower Chinese titles', () => {
    const text = notificationFacingTitle(
      notice('新的关注者', 'alice 关注了你', 'federation.new_follower', {
        actor_label: 'alice',
      }),
    )
    assert.equal(text.includes('新的关注者'), false)
  })

  it('maps leftover agent task failure titles', () => {
    const text = notificationFacingTitle(
      notice('任务失败', '前端任务执行失败', 'agent.task_failed'),
    )
    assert.notEqual(text, '任务失败')
    assert.equal(text.includes('前端任务'), false)
  })
})
