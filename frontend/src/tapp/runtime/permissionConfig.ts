/**
 * Tapp 权限配置
 *
 * 集中管理 API action → 权限映射和权限级别定义，
 * 便于维护和审计权限变更。
 *
 * Host-proxied domains (speech / brewList / federation) must stay in lockstep
 * with:
 * - `docs/development/tapp/fixtures/action_permissions.json` (this map)
 * - `docs/development/tapp/fixtures/host_route_permissions.json` (backend
 *   host_attribution route → permission)
 * - backend `TappPermission` / this file's `PERMISSION_LEVELS`
 *
 * Edit the fixtures first, then update this file and host_attribution.
 * Enforced by `permissionMapConsistency.test.ts` and Rust host_attribution tests.
 */

import type { PermissionLevel, TappPermission } from '../types'

type TappPermissionLevel = Exclude<PermissionLevel, 'public'>

/**
 * 权限级别映射（与 TappPermission.ts 保持同步）
 */
export const PERMISSION_LEVELS: Record<TappPermission, TappPermissionLevel> = {
  'widget:register': 'privileged',
  'platform:read': 'basic',
  'platform:write': 'privileged',
  'platform:register': 'privileged',
  'analytics:read': 'basic',
  'ai:generate': 'elevated',
  'ai:analyze': 'elevated',
  'ai:chat': 'elevated',
  'ai:image': 'elevated',
  'report:read': 'basic',
  'report:write': 'privileged',
  'storage:read': 'basic',
  'storage:write': 'elevated',
  'ui:notification': 'basic',
  'ui:fullscreen': 'basic',
  'ui:theme': 'basic',
  'ui:confirm': 'basic',
  'ui:openUrl': 'basic',
  'network:fetch': 'elevated',
  'media:playback': 'basic',
  'media:volume': 'basic',
  'media:queue': 'basic',
  'media:read': 'basic',
  'media:audio': 'basic',
  'component:theme': 'elevated',
  'component:agent': 'privileged',
  'shortcut:register': 'elevated',
  'event:publish': 'elevated',
  'event:subscribe': 'basic',
  'scheduler:register': 'elevated',
  'speech:tts': 'elevated',
  'speech:asr': 'elevated',
  'tappList:read': 'basic',
  'tappList:manage': 'privileged',
  'brew:read': 'basic',
  'brew:write': 'basic',
  'brew:comment': 'basic',
  'brew:manage': 'privileged',
  'federation:read': 'basic',
  'federation:write': 'basic',
  'federation:message': 'basic',
  'federation:trust': 'privileged',
  'federation:files': 'basic',
}

/**
 * `media.control` action → 最窄动作域权限。
 *
 * 与后端 `media_control_permission`（backend/src/api/tapp_runtime/media.rs）
 * 镜像；两侧表驱动测试锁定同一份映射。旧的粗权限 `media:control` 已移除，
 * 这里不存在任何兼容别名：播放状态=media:playback，音量=media:volume，
 * 队列/模式=media:queue。
 */
export const MEDIA_ACTION_PERMISSIONS: Record<
  | 'play'
  | 'pause'
  | 'next'
  | 'prev'
  | 'seek'
  | 'volume'
  | 'mode'
  | 'mute'
  | 'unmute',
  TappPermission
> = {
  play: 'media:playback',
  pause: 'media:playback',
  next: 'media:playback',
  prev: 'media:playback',
  seek: 'media:playback',
  volume: 'media:volume',
  mute: 'media:volume',
  unmute: 'media:volume',
  mode: 'media:queue',
}

/**
 * API action → 所需权限的静态映射表
 *
 * 使用 Map 以获得 O(1) 查找性能。
 * 'public' 表示无需权限即可调用。
 */
export const PERMISSION_MAP: ReadonlyMap<string, TappPermission | 'public'> =
  new Map([
    // 公开 API（无需权限）
    ['lifecycle.ready', 'public'],
    ['lifecycle.error', 'public'],
    ['ui.getTheme', 'public'],
    ['ui.getPrimaryColor', 'public'],
    ['ui.getLocale', 'public'],
    ['ui.setTitle', 'public'],
    ['context.getApp', 'public'],
    ['context.getUser', 'public'],
    ['context.getPlayer', 'public'],
    ['context.getNavigation', 'public'],
    ['context.getSystem', 'public'],
    ['context.getGeo', 'public'],
    ['user.getRole', 'public'],
    ['user.isAdmin', 'public'],
    ['user.isGuest', 'public'],
    ['user.isLoggedIn', 'public'],
    ['user.getAllowedPermissionLevels', 'public'],
    ['user.canUsePermissionLevel', 'public'],
    ['component.list', 'public'],
    ['shortcut.list', 'public'],
    ['background.list', 'public'],
    ['background.has', 'public'],
    ['animation.getLevel', 'public'],
    ['animation.shouldAnimate', 'public'],
    ['animation.getConfig', 'public'],
    ['animation.getStaggerDelay', 'public'],
    ['dynamicContent.get', 'public'],

    // Tapp API 声明系统 - 权限由后端检查
    ['api.execute', 'public'],
    ['api.list', 'public'],

    // 跨 Tapp 数据访问由双方 manifest + Runtime Grant + 每次宿主授权共同控制，
    // 不使用可长期授予的静态权限。
    ['dataExchange.registerProvider', 'public'],
    ['dataExchange.unregisterProvider', 'public'],
    ['dataExchange.request', 'public'],
    ['dataExchange.respond', 'public'],

    // 小组件权限
    ['widget.register', 'widget:register'],
    ['widget.unregister', 'widget:register'],
    ['widget.listRegistered', 'widget:register'],
    ['widget.updateConfig', 'widget:register'],
    ['widget.instanceSettings.update', 'public'],
    ['widget.invalidate', 'public'],

    // 内容列表权限 — Tapp
    ['tappList.list', 'tappList:read'],
    ['tappList.get', 'tappList:read'],
    ['tappList.getRecent', 'tappList:read'],
    ['tappList.getInstallPackage', 'tappList:read'],
    ['tappList.resolveStoreSource', 'tappList:read'],
    ['tappList.install', 'tappList:manage'],
    ['tappList.uninstall', 'tappList:manage'],
    ['tappList.start', 'tappList:manage'],
    ['tappList.stop', 'tappList:manage'],
    ['tappList.export', 'tappList:manage'],

    // 内容列表权限 — Brew 读取
    ['brewList.list', 'brew:read'],
    ['brewList.get', 'brew:read'],
    ['brewList.sources', 'brew:read'],
    ['brewList.categories', 'brew:read'],
    ['brewList.stats', 'brew:read'],
    ['brewList.discover', 'brew:manage'],
    ['brewList.exportOpml', 'brew:read'],

    // 内容列表权限 — Brew 写入
    ['brewList.markRead', 'brew:write'],
    ['brewList.markUnread', 'brew:write'],
    ['brewList.star', 'brew:write'],
    ['brewList.unstar', 'brew:write'],
    ['brewList.markAllRead', 'brew:write'],

    // 内容列表权限 — Brew 评论
    ['brewList.getComments', 'brew:comment'],
    ['brewList.createComment', 'brew:comment'],
    ['brewList.updateComment', 'brew:comment'],
    ['brewList.deleteComment', 'brew:comment'],
    ['brewList.getReplies', 'brew:comment'],
    ['brewList.createReply', 'brew:comment'],

    // 内容列表权限 — Brew 管理
    ['brewList.addSource', 'brew:manage'],
    ['brewList.updateSource', 'brew:manage'],
    ['brewList.deleteSource', 'brew:manage'],
    ['brewList.refreshSource', 'brew:manage'],
    ['brewList.importOpml', 'brew:manage'],
    ['brewList.createCategory', 'brew:manage'],
    ['brewList.deleteCategory', 'brew:manage'],

    // 平台数据权限
    ['platform.listEnabled', 'platform:read'],
    ['platform.getData', 'platform:read'],
    ['platform.getStats', 'platform:read'],
    ['platform.getDistribution', 'platform:read'],
    ['platform.addItem', 'platform:write'],
    ['platform.addItems', 'platform:write'],
    ['platform.registerPlatform', 'platform:register'],

    // 站点访问统计（聚合）
    ['analytics.getSummary', 'analytics:read'],
    ['analytics.getVisitorCard', 'analytics:read'],
    // Input/output permissions depend on the request shape and are enforced by
    // the backend Runtime Grant (`inline` needs none; platform/storage are dynamic).
    ['data.transform', 'public'],

    // AI 权限
    // AI Task operation/context permissions are resolved dynamically by backend.
    ['ai.tasks.create', 'public'],
    ['ai.tasks.get', 'public'],
    ['ai.tasks.cancel', 'public'],
    ['ai.tasks.usage', 'public'],
    ['ai.tasks.subscribe', 'public'],
    ['ai.tasks.unsubscribe', 'public'],

    // 报告权限
    ['report.listReports', 'report:read'],
    ['report.getReport', 'report:read'],
    ['report.getPlatformReport', 'report:read'],
    ['report.create', 'report:write'],
    ['report.list', 'report:read'],
    ['report.get', 'report:read'],
    ['report.update', 'report:write'],
    ['report.delete', 'report:write'],

    // 存储权限
    ['storage.get', 'storage:read'],
    ['storage.set', 'storage:write'],
    ['storage.remove', 'storage:write'],
    ['storage.keys', 'storage:read'],
    ['storage.getAll', 'storage:read'],
    ['storage.clear', 'storage:write'],
    ['storage.usage', 'storage:read'],
    ['settings.get', 'storage:read'],
    ['settings.set', 'storage:write'],
    ['settings.getAll', 'storage:read'],

    // UI 权限
    ['ui.showNotification', 'ui:notification'],
    ['ui.confirm', 'ui:confirm'],
    ['ui.openUrl', 'ui:openUrl'],
    ['ui.listOpenUrls', 'ui:openUrl'],
    ['ui.requestFullscreen', 'ui:fullscreen'],
    ['ui.exitFullscreen', 'ui:fullscreen'],
    ['ui.toggleFullscreen', 'ui:fullscreen'],
    ['ui.isFullscreen', 'ui:fullscreen'],

    // 媒体权限
    // media.control 的 action 分域权限在沙箱 handler 内按
    // MEDIA_ACTION_PERMISSIONS 逐 action 校验，并在后端
    // POST /api/tapp/media/control 再强制；此处标记 public 仅放行到 handler。
    ['media.control', 'public'],
    ['media.getStatus', 'media:read'],
    ['media.getPlaylist', 'media:read'],
    ['media.playTrack', 'media:playback'],
    ['media.jumpToIndex', 'media:playback'],
    ['media.getSpectrum', 'media:read'],
    // 推流版的 getSpectrum：由 tapp 订阅后由宿主按帧推送，读到的是同一份频谱数据，
    // 沙箱 handler 自身也是校验 media:read
    ['media.spectrumStream', 'media:read'],
    ['media.getLyrics', 'media:read'],
    ['media.getBeatGrid', 'media:read'],
    ['media.loadNeteasePlaylist', 'media:queue'],
    ['media.getSkipVip', 'media:read'],
    ['media.setSkipVip', 'media:queue'],

    // 组件权限
    ['component.registerTheme', 'component:theme'],
    ['component.registerAgent', 'component:agent'],
    // 类型相关权限由后端根据 theme/agent 动态校验。
    ['component.unregister', 'public'],

    // 快捷键权限
    ['shortcut.register', 'shortcut:register'],
    ['shortcut.unregister', 'shortcut:register'],

    // 事件权限
    ['event.publish', 'event:publish'],

    // Agent Interaction is governed by Manifest declaration, interaction state, schema,
    // accepting runtime identity, and host intent confirmation on the backend.
    ['agent.v2.accept', 'public'],
    ['agent.v2.result', 'public'],
    ['agent.v2.reject', 'public'],
    ['agent.v2.intent', 'public'],

    // 后台权限
    ['background.require', 'event:subscribe'],
    ['background.release', 'event:subscribe'],

    // 动态内容权限
    ['dynamicContent.set', 'ui:notification'],
    ['dynamicContent.update', 'ui:notification'],
    ['dynamicContent.remove', 'ui:notification'],

    // 文件操作权限
    ['file.download', 'storage:read'],

    // 包内静态资源（安装包声明内容，可读即可运行的 Tapp 已可见）
    ['assets.get', 'public'],
    ['assets.list', 'public'],

    // 定时任务权限
    ['scheduler.register', 'scheduler:register'],
    ['scheduler.unregister', 'scheduler:register'],
    ['scheduler.list', 'scheduler:register'],
    ['scheduler.get', 'scheduler:register'],
    ['scheduler.enable', 'scheduler:register'],
    ['scheduler.disable', 'scheduler:register'],
    ['scheduler.trigger', 'scheduler:register'],
    ['scheduler.subscribe', 'scheduler:register'],
    ['scheduler.unsubscribe', 'scheduler:register'],
    ['scheduler.complete', 'scheduler:register'],

    // 语音服务权限
    ['speech.tts', 'speech:tts'],
    ['speech.getVoices', 'speech:tts'],
    ['speech.getStatus', 'speech:tts'],
    ['speech.asr', 'speech:asr'],

    // 联邦权限
    ['federation.getIdentity', 'federation:read'],
    // Explicit signing-key rotation (POST /api/federation/keys/rotate)
    ['federation.rotateKeys', 'federation:write'],
    ['federation.getFeed', 'federation:read'],
    ['federation.getTimeline', 'federation:read'],
    ['federation.getObject', 'federation:read'],
    ['federation.getFollowing', 'federation:read'],
    ['federation.getFollowers', 'federation:read'],
    ['federation.getPublished', 'federation:read'],
    ['federation.getChannels', 'federation:read'],
    ['federation.getChannel', 'federation:read'],
    ['federation.getMessages', 'federation:read'],
    ['federation.getRooms', 'federation:read'],
    ['federation.getRoom', 'federation:read'],
    ['federation.getRoomMembers', 'federation:read'],
    ['federation.getRoomMessages', 'federation:read'],
    ['federation.getRings', 'federation:read'],
    ['federation.getRing', 'federation:read'],
    ['federation.getRingPeers', 'federation:read'],
    ['federation.follow', 'federation:write'],
    ['federation.unfollow', 'federation:write'],
    ['federation.publish', 'federation:write'],
    ['federation.createNote', 'federation:write'],
    ['federation.like', 'federation:write'],
    ['federation.unlike', 'federation:write'],
    ['federation.bookmark', 'federation:write'],
    ['federation.unbookmark', 'federation:write'],
    ['federation.getBookmarks', 'federation:read'],
    // External share intent (compose + status only; never server-side post)
    ['federation.getExternalShareStatus', 'federation:read'],
    ['federation.composeExternalShare', 'federation:read'],
    ['federation.announce', 'federation:write'],
    ['federation.unannounce', 'federation:write'],
    ['federation.uploadMedia', 'federation:write'],
    ['federation.unpublish', 'federation:write'],
    ['federation.createChannel', 'federation:write'],
    ['federation.acceptChannel', 'federation:write'],
    ['federation.closeChannel', 'federation:write'],
    ['federation.deleteChannel', 'federation:write'],
    ['federation.createRoom', 'federation:write'],
    ['federation.updateRoom', 'federation:write'],
    ['federation.deleteRoom', 'federation:write'],
    ['federation.inviteMember', 'federation:write'],
    ['federation.acceptRoomInvite', 'federation:write'],
    ['federation.rejectRoomInvite', 'federation:write'],
    ['federation.removeMember', 'federation:write'],
    ['federation.setMemberRole', 'federation:write'],
    ['federation.leaveRoom', 'federation:write'],
    ['federation.transferRoomOwnership', 'federation:write'],
    ['federation.initiateChannelE2e', 'federation:write'],
    ['federation.initiateRoomE2e', 'federation:write'],
    ['federation.addRoomSticker', 'federation:write'],
    ['federation.removeRoomSticker', 'federation:write'],
    ['federation.pinRoomMessage', 'federation:write'],
    ['federation.createRing', 'federation:write'],
    ['federation.leaveRing', 'federation:write'],
    ['federation.addPeer', 'federation:write'],
    ['federation.removePeer', 'federation:write'],
    ['federation.triggerSync', 'federation:write'],
    ['federation.sendMessage', 'federation:message'],
    ['federation.sendRoomMessage', 'federation:message'],
    ['federation.subscribeChannel', 'federation:message'],
    ['federation.unsubscribeChannel', 'federation:message'],
    ['federation.subscribeRoom', 'federation:message'],
    ['federation.unsubscribeRoom', 'federation:message'],
    ['federation.getTrustPolicy', 'federation:trust'],
    ['federation.updateTrustPolicy', 'federation:trust'],
    ['federation.getInstances', 'federation:trust'],
    ['federation.getDeliveryStats', 'federation:read'],
    ['federation.listDelivery', 'federation:read'],
    ['federation.retryDelivery', 'federation:write'],
    ['federation.cancelDelivery', 'federation:write'],
    ['federation.retryAllDeadDelivery', 'federation:write'],
    ['federation.cancelAllPendingDelivery', 'federation:write'],
    ['federation.dismissDelivery', 'federation:write'],
    ['federation.purgeDeadDelivery', 'federation:write'],
    ['federation.joinRoom', 'federation:write'],
    ['federation.updateInstanceTrust', 'federation:trust'],
    ['federation.toggleInstanceBlock', 'federation:trust'],
    ['federation.initiateTransfer', 'federation:files'],
    ['federation.listTransfers', 'federation:files'],
    ['federation.initiateRoomTransfer', 'federation:files'],
    ['federation.listRoomTransfers', 'federation:files'],
    ['federation.listRoomFiles', 'federation:files'],
    ['federation.getTransfer', 'federation:files'],
    ['federation.downloadTransfer', 'federation:files'],
    ['federation.uploadChunk', 'federation:files'],
    ['federation.cancelTransfer', 'federation:files'],
  ])
