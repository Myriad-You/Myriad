import type { AppNotification } from '../services/notificationApi'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { currentCopy, formatCurrent } from '../i18n/localeCopy.ts'
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

function typedNotice(
  notification_type: AppNotification['notification_type'],
  title: string,
  body: string,
  event_key?: string,
  extra?: Record<string, unknown>,
): AppNotification {
  return { ...notice(title, body, event_key, extra), notification_type }
}

describe('notificationFacing', () => {
  it('maps leftover phantasi Chinese titles', () => {
    const text = notificationFacingTitle(
      notice('科技新闻 连续抓取失败', 'timeout', 'phantasi.source_error', {
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

  it('keeps title-only Tapp notification bodies empty', () => {
    const text = notificationFacingBody(
      typedNotice('tapp_notification', 'Sticker saved', '', 'tapp.message', {
        tapp_id: 'com.myriad.aro',
      }),
    )
    assert.equal(text, '')
  })

  it('preserves Tapp-authored titles and bodies as content', () => {
    const notification = typedNotice(
      'tapp_notification',
      'Upload failed',
      '前端任务执行失败',
      'tapp.error',
    )
    assert.equal(notificationFacingTitle(notification), 'Upload failed')
    assert.equal(notificationFacingBody(notification), '前端任务执行失败')
  })

  it('localizes the backend default Tapp title', () => {
    const notification = typedNotice(
      'tapp_notification',
      'App notification',
      'Saved',
      'tapp.message',
    )
    assert.equal(notificationFacingTitle(notification), currentCopy().errors.noticeTapp)
  })

  it('preserves federation message previews as user content', () => {
    const notification = typedNotice(
      'federation_message',
      '任务失败',
      'Unauthorized',
      'federation.channel_message',
      { message_type: 'text' },
    )
    assert.equal(notificationFacingTitle(notification), '任务失败')
    assert.equal(notificationFacingBody(notification), 'Unauthorized')
  })

  it('still localizes scheduled Tapp failures', () => {
    const notification = typedNotice(
      'tapp_notification',
      'Scheduled task failed',
      'Unauthorized',
      'tapp.error',
    )
    assert.equal(notificationFacingTitle(notification), currentCopy().errors.noticeScheduleFailed)
    assert.equal(notificationFacingBody(notification), currentCopy().errors.unauthorized)
  })

  it('pluralizes federation revoked queued items in English', () => {
    const one = notificationFacingBody(
      notice('Unlinked', 'x', 'federation.domain_revoked', {
        target_domain: 'peer.example',
        cancelled_deliveries: 1,
      }),
    )
    assert.match(one, /1 queued item was cancelled/)
    const many = notificationFacingBody(
      notice('Unlinked', 'x', 'federation.domain_revoked', {
        target_domain: 'peer.example',
        cancelled_deliveries: 4,
      }),
    )
    assert.match(many, /4 queued items were cancelled/)
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

  it('maps phantasi new-item and heartbeat notices by their event', () => {
    const phantasi = notificationFacingTitle(
      notice('科技新闻 · 3 篇新内容', '发现 3 篇新内容', 'phantasi.new_items', {
        source_name: '科技新闻',
        new_count: 3,
      }),
    )
    assert.equal(phantasi.includes('篇新内容'), false)
    assert.match(phantasi, /科技新闻/)
    const phantasiBody = notificationFacingBody(
      notice('科技新闻 · 3 new items', '3 new items found', 'phantasi.new_items', {
        new_count: 3,
      }),
    )
    assert.equal(
      phantasiBody,
      formatCurrent(currentCopy().errors.noticePhantasiNewItemsBody, { n: 3 }),
    )
    const heartbeat = notificationFacingTitle(
      notice('Scheduled task: 备份', 'ok', 'heartbeat.succeeded', {
        task_name: '备份',
      }),
    )
    assert.equal(
      heartbeat,
      formatCurrent(currentCopy().errors.noticeHeartbeatTask, { name: '备份' }),
    )
    const seo = notificationFacingTitle(
      notice('Scheduled task: SEO review', 'rewrite', 'heartbeat.seo_review'),
    )
    assert.equal(seo, currentCopy().errors.noticeSeoReview)
    const seoBody = notificationFacingBody(
      notice(
        'Agent SEO',
        '站点描述空了。\n\nsite_description\n新的简介',
        'heartbeat.seo_review',
        { site_description: '新的简介' },
      ),
    )
    assert.match(seoBody, /新的简介/)
  })

  it('maps leftover skill evolution Chinese bodies', () => {
    const improved = notificationFacingBody(
      notice(
        'Skill improved: demo',
        'AI 已根据近期失败原因改写该自动技能。',
        'skill.improved',
        { skill_id: 'demo' },
      ),
    )
    assert.equal(improved.includes('改写该自动技能'), false)
    const pruned = notificationFacingBody(
      notice(
        'Skill removed: demo',
        '自动技能「demo」因失败率过高被淘汰（已移入回收站）。',
        'skill.pruned',
        { skill_id: 'demo' },
      ),
    )
    assert.equal(pruned.includes('回收站'), false)
    assert.match(pruned, /demo/)
  })

  it('maps leftover English phantasi and schedule titles', () => {
    const phantasi = notificationFacingTitle(
      notice('Tech News feed failed repeatedly', 'timeout', undefined, {
        source_name: 'Tech News',
      }),
    )
    assert.equal(/feed failed repeatedly/i.test(phantasi), false)
    assert.match(phantasi, /Tech News/)
    const schedule = notificationFacingTitle(
      notice('Scheduled task failed', 'All 3 retries failed'),
    )
    assert.equal(/All 3 retries/.test(schedule), false)
  })

  it('maps MCP retry bodies', () => {
    const retry = notificationFacingBody(
      notice('MCP files is connected', 'Maintenance retry succeeded', 'mcp.connected', {
        server_id: 'files',
      }),
    )
    assert.equal(retry, currentCopy().errors.noticeMcpMaintenanceRetry)
    const restart = notificationFacingBody(
      notice('MCP files is connected', 'Auto-restart succeeded', 'mcp.connected', {
        server_id: 'files',
      }),
    )
    assert.equal(restart, currentCopy().errors.noticeMcpAutoRestart)
  })

  it('maps leftover agent task failure titles', () => {
    const text = notificationFacingTitle(
      notice('任务失败', '前端任务执行失败', 'agent.task_failed'),
    )
    assert.notEqual(text, '任务失败')
    assert.equal(text.includes('前端任务'), false)
  })
})
