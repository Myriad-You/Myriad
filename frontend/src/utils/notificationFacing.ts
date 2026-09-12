import type { AppNotification } from '../services/notificationApi'
import { currentCopy, formatCurrent } from '../i18n/localeCopy'
import { isUselessErrorText, userFacingError } from './userFacingError'

function fill(
  template: string,
  params: Record<string, string | number>,
): string {
  return formatCurrent(template, params)
}

function metaString(
  notification: AppNotification,
  key: string,
): string {
  const value = notification.metadata?.[key]
  return typeof value === 'string' && value.trim() ? value.trim() : ''
}

export function notificationFacingTitle(notification: AppNotification): string {
  const t = currentCopy().errors
  const eventKey =
    typeof notification.metadata?.event_key === 'string'
      ? notification.metadata.event_key
      : ''
  const name =
    metaString(notification, 'source_name') ||
    metaString(notification, 'platform') ||
    metaString(notification, 'server_id') ||
    metaString(notification, 'tapp_id')

  switch (eventKey) {
    case 'brew.source_error':
      return fill(t.noticeBrewSourceFailed, { name: name || 'RSS' })
    case 'brew.new_items':
      return fill(t.noticeBrewNewItems, {
        name:
          name ||
          notification.title.replaceAll(/\s*·\s*\d.*$/g, '').trim() ||
          'RSS',
        n:
          typeof notification.metadata?.new_count === 'number'
            ? notification.metadata.new_count
            : 0,
      })
    case 'heartbeat.succeeded':
    case 'heartbeat.failed':
      return fill(t.noticeHeartbeatTask, {
        name:
          metaString(notification, 'task_name') ||
          notification.title
            .replaceAll(/^定时任务:\s*/g, '')
            .replaceAll(/^Scheduled task:\s*/ig, '')
            .trim() ||
          'task',
      })
    case 'platform.sync.failed':
      return fill(t.noticePlatformSyncFailed, { name: name || 'Steam' })
    case 'mcp.disconnected':
      return fill(t.noticeMcpFailed, { name: name || 'MCP' })
    case 'mcp.connected':
      return fill(t.noticeMcpConnected, { name: name || 'MCP' })
    case 'tapp.error':
      return t.noticeScheduleFailed
    case 'agent.task_failed':
      return t.noticeAgentTaskFailed
    case 'agent.task_completed':
      return t.noticeAgentTaskCompleted
    case 'agent.task_cancelled':
      return t.agentTaskCancelled
    case 'agent.clarification':
      return t.noticeAgentTaskWaiting
    case 'agent.task_progress':
      return t.noticeAgentTaskRunning
    case 'updater.succeeded':
      return t.noticeUpdaterSucceeded
    case 'updater.failed':
      return t.noticeUpdaterFailed
    case 'updater.needs_manual':
      return t.noticeUpdaterNeedsManual
    case 'updater.running':
      return t.noticeUpdaterRunning
    case 'updater.unknown':
      return t.noticeUpdaterUnknown
    case 'updater.submitted':
      return t.noticeUpdaterSubmitted
    case 'federation.domain_revoked':
      return fill(t.noticeFederationRevoked, {
        name: metaString(notification, 'target_domain') || name || 'remote',
      })
    case 'federation.new_follower':
      return t.noticeNewFollower
    case 'federation.follow_accepted':
      return t.noticeFollowAccepted
    case 'federation.channel_invite':
      return t.noticeChannelInvite
    case 'federation.room_invite':
      return t.noticeRoomInvite
    case 'federation.room_invite_accepted':
      return t.noticeRoomInviteAccepted
    case 'federation.channel_accepted':
      return t.noticeChannelAccepted
    case 'federation.delivery_failed':
      return t.noticeDeliveryFailed
    case 'skill.pruned':
      return fill(t.noticeSkillPruned, {
        name: metaString(notification, 'skill_id') || name,
      })
    case 'skill.improved':
      return fill(t.noticeSkillImproved, {
        name: metaString(notification, 'skill_id') || name,
      })
    case 'skill.changed':
      return fill(t.noticeSkillChanged, {
        name: metaString(notification, 'skill_id') || name,
      })
    default:
      break
  }

  const raw = notification.title || ''
  const leftoverBrewNew = raw.match(/^(.+) · (\d+) 篇新内容$/)
  if (leftoverBrewNew) {
    return fill(t.noticeBrewNewItems, {
      name: leftoverBrewNew[1],
      n: leftoverBrewNew[2],
    })
  }
  const leftoverHeartbeat = raw.match(/^定时任务:\s*(\S.*)$/)
  if (leftoverHeartbeat) {
    return fill(t.noticeHeartbeatTask, { name: leftoverHeartbeat[1] })
  }
  if (raw.includes('连续抓取失败')) {
    return fill(t.noticeBrewSourceFailed, { name: name || raw.replaceAll(/连续抓取失败/g, '').trim() || 'RSS' })
  }
  if (raw.includes('自动刷新失败')) {
    return fill(t.noticePlatformSyncFailed, { name: name || raw.replaceAll(/自动刷新失败/g, '').trim() })
  }
  if (raw.includes('连接失败') && raw.includes('MCP')) {
    return fill(t.noticeMcpFailed, { name: name || 'MCP' })
  }
  if (raw.includes('定时任务失败')) return t.noticeScheduleFailed
  if (/^任务失败$|^任务执行失败$|^前端任务执行失败$|^The task failed$/.test(raw)) {
    return t.noticeAgentTaskFailed
  }
  if (/^任务完成$|^任务已完成$|^The task finished$/.test(raw)) {
    return t.noticeAgentTaskCompleted
  }
  if (/^任务已取消$|^The task was cancelled$/.test(raw)) {
    return t.agentTaskCancelled
  }
  if (/任务等待通道已断开|^The wait channel closed$/.test(raw)) {
    return t.waitChannelClosed
  }
  if (/等待用户输入已超时|^Waiting for input timed out/.test(raw)) {
    return t.waitInputTimeout
  }
  if (/任务状态已不可用|^The task is no longer available$/.test(raw)) {
    return t.taskUnavailable
  }
  if (/任务等待你的回答|The task needs your reply/.test(raw)) {
    return t.noticeAgentTaskWaiting
  }
  if (/Arael 正在执行任务|^Arael is working$|^Agent is working$/.test(raw)) {
    return t.noticeAgentTaskRunning
  }
  if (raw.includes('系统更新任务失败')) return t.noticeUpdaterFailed
  if (raw.includes('系统更新需要人工')) return t.noticeUpdaterNeedsManual
  if (raw.includes('Tapp 通知')) return t.noticeTapp
  if (raw.includes('联邦关系已解除')) {
    return fill(t.noticeFederationRevoked, {
      name: metaString(notification, 'target_domain') || 'remote',
    })
  }
  if (raw.includes('新的关注者')) return t.noticeNewFollower
  if (raw.includes('关注已通过')) return t.noticeFollowAccepted
  if (raw.includes('新的私信请求')) return t.noticeChannelInvite
  if (raw.includes('群组邀请已接受')) return t.noticeRoomInviteAccepted
  if (raw.includes('群组邀请')) return t.noticeRoomInvite
  if (raw.includes('私信通道已建立')) return t.noticeChannelAccepted
  if (raw.includes('联邦投递失败')) return t.noticeDeliveryFailed
  if (raw.includes('技能已自动淘汰')) {
    return fill(t.noticeSkillPruned, {
      name: metaString(notification, 'skill_id') || name,
    })
  }
  if (raw.includes('技能已自动改进')) {
    return fill(t.noticeSkillImproved, {
      name: metaString(notification, 'skill_id') || name,
    })
  }
  if (/feed failed repeatedly/i.test(raw)) {
    return fill(t.noticeBrewSourceFailed, { name: name || 'RSS' })
  }
  if (/auto-refresh failed/i.test(raw)) {
    return fill(t.noticePlatformSyncFailed, { name: name || 'Steam' })
  }
  if (/scheduled task failed|a scheduled task failed/i.test(raw)) {
    return t.noticeScheduleFailed
  }
  if (/system update failed/i.test(raw)) return t.noticeUpdaterFailed
  if (/system update needs/i.test(raw)) return t.noticeUpdaterNeedsManual
  if (!raw || isUselessErrorText(raw)) return t.noticeTapp
  const mapped = userFacingError(raw, t.noticeTapp)
  return mapped === raw ? raw : mapped
}

export function notificationFacingBody(notification: AppNotification): string {
  const t = currentCopy().errors
  const eventKey =
    typeof notification.metadata?.event_key === 'string'
      ? notification.metadata.event_key
      : ''
  const actor = metaString(notification, 'actor_label')
  const room = metaString(notification, 'room_name')
  if (eventKey === 'federation.domain_revoked') {
    const count = notification.metadata?.cancelled_deliveries
    return fill(t.noticeFederationRevokedBody, {
      name: metaString(notification, 'target_domain') || 'remote',
      count: typeof count === 'number' ? count : Number(count ?? 0),
    })
  }
  if (eventKey === 'federation.new_follower') {
    return fill(t.noticeNewFollowerBody, { name: actor || 'someone' })
  }
  if (eventKey === 'federation.follow_accepted') {
    return fill(t.noticeFollowAcceptedBody, { name: actor || 'someone' })
  }
  if (eventKey === 'federation.channel_invite') {
    return fill(t.noticeChannelInviteBody, { name: actor || 'someone' })
  }
  if (eventKey === 'federation.room_invite') {
    return room
      ? fill(t.noticeRoomInviteNamedBody, { name: actor || 'someone', room })
      : fill(t.noticeRoomInviteBody, { name: actor || 'someone' })
  }
  if (eventKey === 'federation.room_invite_accepted') {
    return room
      ? fill(t.noticeRoomInviteAcceptedNamedBody, {
          name: actor || 'someone',
          room,
        })
      : fill(t.noticeRoomInviteAcceptedBody, { name: actor || 'someone' })
  }
  if (eventKey === 'federation.channel_accepted') {
    return fill(t.noticeChannelAcceptedBody, { name: actor || 'someone' })
  }
  if (eventKey === 'federation.delivery_failed') {
    return fill(t.noticeDeliveryFailedBody, {
      name: metaString(notification, 'target_domain') || 'remote',
    })
  }
  if (eventKey === 'skill.improved') {
    return t.noticeSkillImprovedBody
  }
  if (eventKey === 'skill.pruned') {
    return fill(t.noticeSkillPrunedBody, {
      name: metaString(notification, 'skill_id') || 'skill',
    })
  }
  if (
    eventKey === 'brew.new_items' &&
    (!notification.body ||
      /^发现 \d+ 篇新内容$/.test(notification.body) ||
      /^\d+ new items found$/i.test(notification.body))
  ) {
    const n = notification.metadata?.new_count
    return fill(t.noticeBrewNewItemsBody, {
      n: typeof n === 'number' ? n : 0,
    })
  }
  const messageType = metaString(notification, 'message_type')
  if (messageType === 'image' || /^📷 图片$|^Photo$/.test(notification.body)) {
    return t.noticePreviewPhoto
  }
  if (
    messageType === 'file' ||
    messageType === 'file-meta' ||
    /^📎 文件$|^File$/.test(notification.body)
  ) {
    return t.noticePreviewFile
  }
  if (messageType === 'system' || /^系统消息$|^System message$/.test(notification.body)) {
    return t.noticePreviewSystem
  }
  if (
    /^🔒 加密消息$|^Encrypted message$/.test(notification.body)
  ) {
    return t.noticePreviewEncrypted
  }
  if (/^新消息$|^New message$/.test(notification.body)) {
    return t.noticePreviewNew
  }
  const leftoverBrewBody = notification.body.match(/^发现 (\d+) 篇新内容$/)
  if (leftoverBrewBody) {
    return fill(t.noticeBrewNewItemsBody, { n: Number(leftoverBrewBody[1]) })
  }
  return userFacingError(notification.body, notification.body)
}
