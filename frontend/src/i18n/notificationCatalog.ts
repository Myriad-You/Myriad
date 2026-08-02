/**
 * Notification settings copy — locale catalogs next to i18n for review.
 *
 * Why not flat TranslationKeys?
 * Event keys are domain enums (`NotificationEventKey`, dotted ids like
 * `agent.task_progress`). Keeping a typed Record map is clearer than
 * hundreds of flattened keys and still ships with zh/en/ja in one place.
 *
 * Public API: `getNotificationCopy(locale)` (also re-exported from `./index`).
 */

import type {
  NotificationEventKey,
  NotificationSourceKey,
} from "../services/notificationPreferencesApi"
import type { Locale } from "./index"

export interface NotificationSourceCopy {
  title: string
  description: string
}

export interface NotificationUiCopy {
  master: string
  masterDesc: string
  delivery: string
  island: string
  islandDesc: string
  toast: string
  toastDesc: string
  browser: string
  browserDesc: string
  sourceEnabled: string
  sources: string
  locations: string
  locationsDesc: string
  events: string
  panelLocation: string
  toastLocation: string
  islandLocation: string
  browserLocation: string
}

export interface NotificationCopy {
  sources: Record<NotificationSourceKey, NotificationSourceCopy>
  events: Record<NotificationEventKey, string>
  ui: NotificationUiCopy
}

const SOURCE_TEXT: Record<
  Locale,
  Record<NotificationSourceKey, NotificationSourceCopy>
> = {
  'zh-CN': {
    agent: {
      title: 'Arael 任务',
      description: '任务进度、结果、取消与澄清请求',
    },
    heartbeat: {
      title: 'Heartbeat',
      description: 'Arael 后台定时任务执行结果',
    },
    mcp: { title: 'MCP 服务', description: '工具服务器连接和断开状态' },
    brew: { title: 'Brew', description: '订阅源新内容与连续抓取错误' },
    tapp: { title: 'Tapp', description: 'Tapp 主动消息、警告和任务错误' },
    updater: { title: '系统更新', description: '更新、回滚及人工恢复状态' },
    federation: { title: '联邦', description: '联邦消息、关注和邀请' },
    system: { title: '系统', description: 'Myriad 自身的重要信息' },
  },
  'en-US': {
    agent: {
      title: 'Arael Tasks',
      description: 'Task progress, results, cancellation and clarification',
    },
    heartbeat: {
      title: 'Heartbeat',
      description: 'Results from Arael background schedules',
    },
    mcp: { title: 'MCP Services', description: 'Tool server connection state' },
    brew: {
      title: 'Brew',
      description: 'New feed items and repeated fetch errors',
    },
    tapp: {
      title: 'Tapp',
      description: 'App messages, warnings and task errors',
    },
    updater: {
      title: 'System Update',
      description: 'Update, rollback and recovery status',
    },
    federation: {
      title: 'Federation',
      description: 'Federated messages, follows and invitations',
    },
    system: {
      title: 'System',
      description: 'Important information from Myriad',
    },
  },
  'ja-JP': {
    agent: {
      title: 'Arael タスク',
      description: '進行状況、結果、キャンセル、確認要求',
    },
    heartbeat: {
      title: 'Heartbeat',
      description: 'Arael バックグラウンド定期処理の結果',
    },
    mcp: { title: 'MCP サービス', description: 'ツールサーバーの接続状態' },
    brew: { title: 'Brew', description: '新着フィードと連続取得エラー' },
    tapp: { title: 'Tapp', description: 'アプリの通知、警告、タスクエラー' },
    updater: {
      title: 'システム更新',
      description: '更新、ロールバック、復旧状態',
    },
    federation: { title: '連合', description: '連合メッセージ、フォロー、招待' },
    system: { title: 'システム', description: 'Myriad からの重要なお知らせ' },
  },
}

const EVENT_TEXT: Record<Locale, Record<NotificationEventKey, string>> = {
  'zh-CN': {
    'agent.task_progress': '任务进度',
    'agent.task_completed': '任务完成',
    'agent.task_failed': '任务失败',
    'agent.task_cancelled': '任务取消',
    'agent.clarification': '等待回答',
    'heartbeat.succeeded': '定时任务成功',
    'heartbeat.failed': '定时任务失败',
    'mcp.connected': 'MCP 已连接',
    'mcp.disconnected': 'MCP 断开或失败',
    'brew.new_items': '发现新内容',
    'brew.source_error': '订阅源连续失败',
    'tapp.message': '普通消息',
    'tapp.warning': '警告',
    'tapp.error': '错误与任务失败',
    'updater.submitted': '任务已提交',
    'updater.running': '任务执行中',
    'updater.succeeded': '更新成功',
    'updater.failed': '更新失败',
    'updater.needs_manual': '需要人工处理',
    'updater.unknown': '状态无法确认',
    'federation.channel_message': '私信新消息',
    'federation.room_message': '群聊新消息',
    'federation.new_follower': '新的关注者',
    'federation.follow_accepted': '关注已通过',
    'federation.channel_invite': '私信邀请',
    'federation.room_invite': '群组邀请',
    'federation.channel_accepted': '私信通道已建立',
    'federation.room_invite_accepted': '群组邀请已接受',
    'system.info': '系统信息',
    'skill.pruned': '技能自动淘汰',
    'skill.improved': '技能自动改进',
    'skill.changed': '技能变更',
  },
  'en-US': {
    'agent.task_progress': 'Task progress',
    'agent.task_completed': 'Task completed',
    'agent.task_failed': 'Task failed',
    'agent.task_cancelled': 'Task cancelled',
    'agent.clarification': 'Waiting for an answer',
    'heartbeat.succeeded': 'Scheduled task succeeded',
    'heartbeat.failed': 'Scheduled task failed',
    'mcp.connected': 'MCP connected',
    'mcp.disconnected': 'MCP disconnected or failed',
    'brew.new_items': 'New content found',
    'brew.source_error': 'Feed repeatedly failed',
    'tapp.message': 'Message',
    'tapp.warning': 'Warning',
    'tapp.error': 'Error or task failure',
    'updater.submitted': 'Job submitted',
    'updater.running': 'Job running',
    'updater.succeeded': 'Update succeeded',
    'updater.failed': 'Update failed',
    'updater.needs_manual': 'Manual recovery required',
    'updater.unknown': 'Status cannot be confirmed',
    'federation.channel_message': 'Direct message',
    'federation.room_message': 'Room message',
    'federation.new_follower': 'New follower',
    'federation.follow_accepted': 'Follow accepted',
    'federation.channel_invite': 'Direct-message invitation',
    'federation.room_invite': 'Room invitation',
    'federation.channel_accepted': 'Direct channel established',
    'federation.room_invite_accepted': 'Room invite accepted',
    'system.info': 'System information',
    'skill.pruned': 'Skill auto-pruned',
    'skill.improved': 'Skill auto-improved',
    'skill.changed': 'Skill changed',
  },
  'ja-JP': {
    'agent.task_progress': 'タスクの進行状況',
    'agent.task_completed': 'タスク完了',
    'agent.task_failed': 'タスク失敗',
    'agent.task_cancelled': 'タスクキャンセル',
    'agent.clarification': '回答待ち',
    'heartbeat.succeeded': '定期タスク成功',
    'heartbeat.failed': '定期タスク失敗',
    'mcp.connected': 'MCP 接続',
    'mcp.disconnected': 'MCP 切断または失敗',
    'brew.new_items': '新着コンテンツ',
    'brew.source_error': 'フィードの連続失敗',
    'tapp.message': 'メッセージ',
    'tapp.warning': '警告',
    'tapp.error': 'エラーまたはタスク失敗',
    'updater.submitted': 'ジョブ送信済み',
    'updater.running': 'ジョブ実行中',
    'updater.succeeded': '更新成功',
    'updater.failed': '更新失敗',
    'updater.needs_manual': '手動対応が必要',
    'updater.unknown': '状態を確認できません',
    'federation.channel_message': 'ダイレクトメッセージ',
    'federation.room_message': 'ルームメッセージ',
    'federation.new_follower': '新しいフォロワー',
    'federation.follow_accepted': 'フォロー承認',
    'federation.channel_invite': 'DM 招待',
    'federation.room_invite': 'ルーム招待',
    'federation.channel_accepted': 'DM チャンネル確立',
    'federation.room_invite_accepted': 'ルーム招待が承認されました',
    'system.info': 'システム情報',
    'skill.pruned': 'スキル自動淘汰',
    'skill.improved': 'スキル自動改善',
    'skill.changed': 'スキル変更',
  },
}

const UI_TEXT: Record<Locale, NotificationUiCopy> = {
  'zh-CN': {
    master: '启用通知中心',
    masterDesc: '关闭后不再创建新的通知，已有历史仍会保留',
    delivery: '展示方式',
    island: '智能岛轮播',
    islandDesc: '新通知到达时在顶部控制岛展示',
    toast: 'Toast 通知',
    toastDesc: '新通知到达时使用全局提示',
    browser: '浏览器系统通知',
    browserDesc: '页面位于后台且浏览器已授权时展示',
    sourceEnabled: '允许此来源',
    sources: '通知来源',
    locations: '显示位置',
    locationsDesc: '与上方全局展示开关共同生效',
    events: '通知事件',
    panelLocation: '通知面板',
    toastLocation: 'Toast',
    islandLocation: '智能岛',
    browserLocation: '系统通知',
  },
  'en-US': {
    master: 'Enable notification center',
    masterDesc:
      'No new notifications are created when disabled; history is kept',
    delivery: 'Presentation',
    island: 'Control-island carousel',
    islandDesc: 'Show new notifications in the top control island',
    toast: 'Toast notifications',
    toastDesc: 'Use global alerts when new notifications arrive',
    browser: 'Browser system notifications',
    browserDesc: 'Show while the page is in the background when permitted',
    sourceEnabled: 'Allow this source',
    sources: 'Notification sources',
    locations: 'Display locations',
    locationsDesc: 'Combined with the global presentation switches above',
    events: 'Notification events',
    panelLocation: 'Notification panel',
    toastLocation: 'Toast',
    islandLocation: 'Control island',
    browserLocation: 'System notification',
  },
  'ja-JP': {
    master: '通知センターを有効にする',
    masterDesc: '無効時は新規通知を作成せず、履歴は保持します',
    delivery: '表示方法',
    island: 'コントロールアイランド',
    islandDesc: '新着通知を上部のコントロールアイランドに表示',
    toast: 'Toast 通知',
    toastDesc: '新着通知をグローバル表示',
    browser: 'ブラウザー通知',
    browserDesc: '許可済みでページがバックグラウンドの時に表示',
    sourceEnabled: 'この送信元を許可',
    sources: '通知の送信元',
    locations: '表示場所',
    locationsDesc: '上部のグローバル表示設定と組み合わせて適用します',
    events: '通知イベント',
    panelLocation: '通知パネル',
    toastLocation: 'Toast',
    islandLocation: 'コントロールアイランド',
    browserLocation: 'システム通知',
  },
}

export function getNotificationCopy(locale: Locale): NotificationCopy {
  return {
    sources: SOURCE_TEXT[locale],
    events: EVENT_TEXT[locale],
    ui: UI_TEXT[locale],
  }
}
