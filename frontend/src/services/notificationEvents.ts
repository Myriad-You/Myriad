/**
 * Notification event catalog — the frontend mirror of the backend
 * `NotificationEventKey` enum. `notificationEvents.test.ts` checks this table
 * against `shared/notification_events.json`, which a backend test checks
 * against the enum. Never compare `metadata.event_key` / `metadata.action`
 * with a string literal; go through the constants below.
 */

export const NOTIFICATION_SOURCE_KEYS = [
  'agent',
  'heartbeat',
  'mcp',
  'phantasi',
  'tapp',
  'updater',
  'federation',
  'system',
] as const

export type NotificationSourceKey = (typeof NOTIFICATION_SOURCE_KEYS)[number]

/** Named event keys. Order matches the settings UI and the shared contract. */
export const NotificationEvent = {
  agentTaskProgress: 'agent.task_progress',
  agentTaskCompleted: 'agent.task_completed',
  agentTaskFailed: 'agent.task_failed',
  agentTaskCancelled: 'agent.task_cancelled',
  agentClarification: 'agent.clarification',
  meropePlatformActivity: 'agent.merope.platform_activity',
  meropeReportReady: 'agent.merope.report_ready',
  meropeReachOut: 'agent.merope.reach_out',
  heartbeatSucceeded: 'heartbeat.succeeded',
  heartbeatFailed: 'heartbeat.failed',
  heartbeatSeoReview: 'heartbeat.seo_review',
  mcpConnected: 'mcp.connected',
  mcpDisconnected: 'mcp.disconnected',
  phantasiNewItems: 'phantasi.new_items',
  phantasiSourceError: 'phantasi.source_error',
  platformSyncFailed: 'platform.sync.failed',
  tappMessage: 'tapp.message',
  tappWarning: 'tapp.warning',
  tappError: 'tapp.error',
  updaterSubmitted: 'updater.submitted',
  updaterRunning: 'updater.running',
  updaterSucceeded: 'updater.succeeded',
  updaterFailed: 'updater.failed',
  updaterNeedsManual: 'updater.needs_manual',
  updaterUnknown: 'updater.unknown',
  federationChannelMessage: 'federation.channel_message',
  federationRoomMessage: 'federation.room_message',
  federationNewFollower: 'federation.new_follower',
  federationFollowAccepted: 'federation.follow_accepted',
  federationChannelInvite: 'federation.channel_invite',
  federationRoomInvite: 'federation.room_invite',
  federationChannelAccepted: 'federation.channel_accepted',
  federationRoomInviteAccepted: 'federation.room_invite_accepted',
  federationDeliveryFailed: 'federation.delivery_failed',
  federationDomainRevoked: 'federation.domain_revoked',
  systemInfo: 'system.info',
  skillPruned: 'skill.pruned',
  skillImproved: 'skill.improved',
  skillChanged: 'skill.changed',
} as const

export type NotificationEventKey =
  (typeof NotificationEvent)[keyof typeof NotificationEvent]

/**
 * Preference source per event. Not derivable from the key prefix:
 * `platform.sync.failed` belongs to `system`, `skill.*` to `agent`.
 */
export const NOTIFICATION_EVENT_SOURCES: Readonly<
  Record<NotificationEventKey, NotificationSourceKey>
> = {
  'agent.task_progress': 'agent',
  'agent.task_completed': 'agent',
  'agent.task_failed': 'agent',
  'agent.task_cancelled': 'agent',
  'agent.clarification': 'agent',
  'agent.merope.platform_activity': 'agent',
  'agent.merope.report_ready': 'agent',
  'agent.merope.reach_out': 'agent',
  'heartbeat.succeeded': 'heartbeat',
  'heartbeat.failed': 'heartbeat',
  'heartbeat.seo_review': 'heartbeat',
  'mcp.connected': 'mcp',
  'mcp.disconnected': 'mcp',
  'phantasi.new_items': 'phantasi',
  'phantasi.source_error': 'phantasi',
  'platform.sync.failed': 'system',
  'tapp.message': 'tapp',
  'tapp.warning': 'tapp',
  'tapp.error': 'tapp',
  'updater.submitted': 'updater',
  'updater.running': 'updater',
  'updater.succeeded': 'updater',
  'updater.failed': 'updater',
  'updater.needs_manual': 'updater',
  'updater.unknown': 'updater',
  'federation.channel_message': 'federation',
  'federation.room_message': 'federation',
  'federation.new_follower': 'federation',
  'federation.follow_accepted': 'federation',
  'federation.channel_invite': 'federation',
  'federation.room_invite': 'federation',
  'federation.channel_accepted': 'federation',
  'federation.room_invite_accepted': 'federation',
  'federation.delivery_failed': 'federation',
  'federation.domain_revoked': 'federation',
  'system.info': 'system',
  'skill.pruned': 'agent',
  'skill.improved': 'agent',
  'skill.changed': 'agent',
}

export const NOTIFICATION_EVENT_KEYS: readonly NotificationEventKey[] =
  Object.values(NotificationEvent)

export function isNotificationEventKey(
  value: unknown,
): value is NotificationEventKey {
  return typeof value === 'string' && Object.hasOwn(NOTIFICATION_EVENT_SOURCES, value)
}

/** Read `metadata.event_key` only when it is a catalogued key. */
export function notificationEventKeyOf(
  metadata: Record<string, unknown> | null | undefined,
): NotificationEventKey | undefined {
  const value = metadata?.event_key
  return isNotificationEventKey(value) ? value : undefined
}

/** `metadata.action` values: where a click on the notification lands. */
export const NotificationAction = {
  openAgent: 'open_agent',
  openAgentManage: 'open_agent_manage',
} as const

export type NotificationActionKey =
  (typeof NotificationAction)[keyof typeof NotificationAction]

/** Every catalogued Merope event lives under this prefix. */
export const MEROPE_EVENT_PREFIX = 'agent.merope.'

/**
 * Persona speech topic for a touch reaction. Rides on live speech, never on a
 * stored notification, so it is deliberately not in the catalog.
 */
export const MEROPE_TOUCH_TOPIC = `${MEROPE_EVENT_PREFIX}touch`
