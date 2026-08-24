import { currentCopy } from '../i18n/localeCopy'
import { ApiError } from '../services/api'

export function httpStatusMessage(status: number): string {
  const t = currentCopy().errors
  if (status === 401) return t.unauthorized
  if (status === 403) return t.forbidden
  if (status === 404) return t.notFound
  if (status === 408) return t.timeout
  if (status === 429) return t.rateLimited
  if (status >= 500) {
    return t.serverError.replace('{status}', String(status))
  }
  if (status > 0) {
    return t.httpStatus.replace('{status}', String(status))
  }
  return t.networkError
}

export function statusFromErrorText(text: string): number {
  const m =
    text.match(/\bHTTP\s+(\d{3})\b/i) ||
    text.match(/^API Error:\s*(\d{3})$/i) ||
    text.match(/\((?:HTTP\s*)?(\d{3})\)$/)
  const status = m ? Number(m[1]) : 0
  return status >= 400 && status <= 599 ? status : 0
}

export function isUselessErrorText(text: string): boolean {
  const detail = text.replace(/\s+/g, ' ').trim()
  if (!detail) return true
  if (/^API Error:\s*\d+$/i.test(detail)) return true
  if (/^HTTP(\s+error!)?(\s*status:?)?\s*\d+(\s*:.*)?$/i.test(detail)) {
    return true
  }
  if (/^failed to [a-z ]+:\s*\d+$/i.test(detail)) return true
  if (/install failed(:\s*\d+)?$/i.test(detail)) return true
  if (/csrf token (unavailable|refresh failed)/i.test(detail)) return true
  if (/^\{[\s\S]*\}$/.test(detail)) return true
  if (/failed to fetch|networkerror|load failed/i.test(detail)) return true
  if (/^unknown error$/i.test(detail)) return true
  if (/^request timeout$/i.test(detail)) return true
  if (/^internal (server )?error$/i.test(detail)) return true
  if (/^operation failed$/i.test(detail)) return true
  if (/^failed$/i.test(detail)) return true
  if (/^ai generation failed$/i.test(detail)) return true
  if (/^ai error:/i.test(detail)) return true
  if (
    /^failed to (save|load|get|publish|rotate|compose|process|verify|create|update|set|read|refresh|fetch|parse|start|decode|clear|collect|restore|seal) /i.test(
      detail,
    )
  ) {
    return true
  }
  if (/^no library data available$/i.test(detail)) return true
  if (/^action failed$/i.test(detail)) return true
  if (/^discovery failed$|^import failed$|^failed to add$/i.test(detail)) {
    return true
  }
  if (/^database error$/i.test(detail)) return true
  if (/\((?:HTTP\s*)?\d{3}\)$/i.test(detail)) {
    const inner = detail.replace(/\s*\((?:HTTP\s*)?\d{3}\)\s*$/i, '').trim()
    if (
      !inner ||
      /^could not [a-z ]+$/i.test(inner) ||
      /^failed to [a-z ]+$/i.test(inner)
    ) {
      return true
    }
  }
  return false
}

function readHint(reason: unknown): string {
  if (
    reason &&
    typeof reason === 'object' &&
    'hint' in reason &&
    typeof (reason as { hint: unknown }).hint === 'string'
  ) {
    return (reason as { hint: string }).hint.trim()
  }
  return ''
}

function readStatus(reason: unknown): number {
  if (
    reason &&
    typeof reason === 'object' &&
    'status' in reason &&
    typeof (reason as { status: unknown }).status === 'number'
  ) {
    return (reason as { status: number }).status
  }
  return 0
}

function readCode(reason: unknown): string {
  if (
    reason &&
    typeof reason === 'object' &&
    'code' in reason &&
    typeof (reason as { code: unknown }).code === 'string'
  ) {
    return (reason as { code: string }).code.trim()
  }
  return ''
}

function joinParts(...parts: string[]): string {
  return parts.filter(Boolean).join(' · ')
}

function clip(text: string): string {
  return text.length > 180 ? `${text.slice(0, 179)}…` : text
}

/** Localized, diagnosable copy for anything that can land in the UI. */
export function userFacingError(reason: unknown, fallback?: string): string {
  const t = currentCopy().errors
  const fallbackText = fallback?.trim() || t.unknown
  const raw =
    reason instanceof Error
      ? reason.message.trim()
      : typeof reason === 'string'
        ? reason.trim()
        : ''
  const status = readStatus(reason) || statusFromErrorText(raw)
  const code = readCode(reason)
  const hint = readHint(reason)

  if (code === 'TIMEOUT' || status === 408) {
    return joinParts(t.timeout, usefulExtra(hint, t.timeout))
  }
  if (code === 'NETWORK_ERROR' || (status === 0 && reason instanceof ApiError)) {
    return joinParts(t.networkError, usefulExtra(hint, t.networkError))
  }
  if (code === 'CSRF' || /csrf token/i.test(raw)) {
    return joinParts(t.csrfUnavailable, usefulExtra(hint, t.csrfUnavailable))
  }
  if (
    code === 'database_error' ||
    code === 'DATABASE_ERROR' ||
    /^database (error|query failed|not connected|is not connected)$/i.test(raw) ||
    /数据库连接未初始化|数据库未连接/.test(raw)
  ) {
    return joinParts(t.database, usefulExtra(hint, t.database))
  }
  if (code === 'password_failed') {
    return joinParts(t.passwordFailed, usefulExtra(hint, t.passwordFailed))
  }
  if (code === 'session_failed') {
    return joinParts(t.sessionFailed, usefulExtra(hint, t.sessionFailed))
  }
  if (code === 'account_create_failed') {
    return joinParts(
      currentCopy().auth.registerFailed,
      usefulExtra(hint, currentCopy().auth.registerFailed),
    )
  }
  if (
    code === 'config_file_permission' ||
    /无法创建配置文件|无法读取配置文件|无法保存配置文件/.test(raw)
  ) {
    return joinParts(t.configFilePermission, usefulExtra(hint, t.configFilePermission))
  }
  if (
    code === 'ai_response_invalid' ||
    /^failed to parse ai response$/i.test(raw)
  ) {
    return joinParts(t.aiResponseInvalid, usefulExtra(hint, t.aiResponseInvalid))
  }
  if (
    code === 'ai_generation_failed' ||
    /^ai error:/i.test(raw) ||
    /^ai generation failed$/i.test(raw) ||
    /^gemini api /i.test(raw)
  ) {
    return joinParts(t.aiGenerationFailed, usefulExtra(hint, t.aiGenerationFailed))
  }
  if (code === 'settings_backup_failed') {
    return joinParts(
      t.settingsBackupRestoreFailed,
      usefulExtra(hint, t.settingsBackupRestoreFailed),
    )
  }
  if (code === 'youtube_upstream_failed' || /^youtube upstream failed$/i.test(raw)) {
    return joinParts(t.serverError.replace('{status}', String(status || 502)))
  }
  if (code === 'e2e_key_failed' || /^failed to seal e2e key/i.test(raw)) {
    return joinParts(t.operationFailed, usefulExtra(hint, t.operationFailed))
  }
  if (code === 'config_save_failed') {
    return joinParts(t.operationFailed, usefulExtra(hint, t.operationFailed))
  }
  const brew = currentCopy().brew
  if (code === 'notion_fetch_failed' || /^failed to fetch notion/i.test(raw)) {
    return brew.errorNotionFetch
  }
  if (
    code === 'feed_parse_failed' ||
    /^failed to parse feed/i.test(raw)
  ) {
    return brew.errorFeedNeedName
  }
  if (
    code === 'feed_discover_failed' ||
    /^unable to discover rss/i.test(raw)
  ) {
    return brew.errorDiscoverFailed
  }
  if (code === 'mcp_config_invalid' || /^invalid mcp config/i.test(raw)) {
    return currentCopy().config.mcpInvalidConfig
  }
  if (
    /^invalid audio data$/i.test(raw) ||
    /^please (provide|upload) audio/i.test(raw) ||
    /无效的Base64|必须提供 audio_data|请上传音频|无效的音频数据/.test(raw)
  ) {
    return t.asrInvalidAudio
  }
  if (
    /speech service is not configured|语音服务未配置|TTS 服务未配置/i.test(raw)
  ) {
    return t.speechNotConfigured
  }
  if (
    /official speech requires openai|speech tts openai|转写已配置|官方播报请选 OpenAI|OpenRouter 目前没有官方/i.test(
      raw,
    )
  ) {
    return t.speechTtsOpenAiRequired
  }
  if (
    /speech service returned no audio|tts服务未返回音频|TTS 未返回音频/i.test(raw)
  ) {
    return t.speechTtsNoAudio
  }
  if (/speech text is too long|文本过长/.test(raw)) {
    return t.speechTextTooLong
  }
  if (
    /speech text is empty|dialogue list is empty|对话列表不能为空|文本不能为空/i.test(
      raw,
    )
  ) {
    return raw.toLowerCase().includes('list') || /对话列表/.test(raw)
      ? t.speechBatchEmpty
      : t.emptyDialogueText
  }
  if (/too many dialogues|对话数量超过限制/i.test(raw)) {
    return t.speechBatchTooMany
  }
  if (
    /speech service is unreachable|speech service request failed/i.test(raw)
  ) {
    return t.speechUpstreamFailed
  }
  if (
    code === 'domain_invalid' ||
    /^invalid origin$/i.test(raw) ||
    /^invalid url:/i.test(raw) ||
    /^origin must /i.test(raw) ||
    /wildcard origins are not allowed/i.test(raw) ||
    /http is only allowed for localhost/i.test(raw) ||
    /unsupported scheme/i.test(raw) ||
    /cors_origins would be empty/i.test(raw)
  ) {
    return t.domainInvalid
  }
  if (
    code === 'oauth_authorize_failed' ||
    /^failed to start (discord )?authorization/i.test(raw)
  ) {
    return t.oauthStartFailed
  }
  if (
    code === 'ROOM_MATERIALIZE_FAILED' ||
    /failed to (join|materialize) room/i.test(raw)
  ) {
    return t.roomJoinFailed
  }
  if (code === 'oauth_slug_required' || /^provider slug is required$/i.test(raw)) {
    return t.oauthSlugRequired
  }
  if (code === 'oauth_slug_invalid' || /invalid slug /i.test(raw)) {
    return t.oauthSlugInvalid
  }
  if (code === 'oauth_slug_duplicate' || /duplicate provider slug/i.test(raw)) {
    return t.oauthSlugDuplicate
  }
  if (
    code === 'oauth_client_id_required' ||
    /requires client_id/i.test(raw)
  ) {
    return t.oauthClientIdRequired
  }
  if (
    code === 'oauth_client_secret_required' ||
    /requires client_secret/i.test(raw)
  ) {
    return t.oauthClientSecretRequired
  }
  if (
    code === 'oauth_discovery_required' ||
    /requires discovery_url/i.test(raw)
  ) {
    return t.oauthDiscoveryRequired
  }
  if (
    code === 'oauth_kind_unsupported' ||
    /unsupported provider kind/i.test(raw)
  ) {
    return t.oauthKindUnsupported
  }
  if (
    code === 'remote_actor_unresolved' ||
    /cannot resolve (remote actor|actor|peer)/i.test(raw)
  ) {
    return t.remoteActorUnresolved
  }
  if (
    code === 'webfinger_failed' ||
    code === 'webfinger_not_found' ||
    code === 'webfinger_unavailable' ||
    /webfinger/i.test(raw)
  ) {
    return t.webfingerFailed
  }
  if (
    code === 'steam_not_configured' ||
    /steam api key 或 steam id 未配置|steam is not configured/i.test(raw)
  ) {
    return t.steamNotConfigured
  }
  if (
    code === 'platform_disabled' ||
    /平台未启用|is not enabled/i.test(raw)
  ) {
    return t.platformDisabled
  }
  if (
    code === 'fetch_failed' ||
    /failed to fetch data|获取失败|获取 .+失败|验证失败|解析响应失败|请求失败/i.test(
      raw,
    )
  ) {
    return t.platformFetchFailed
  }
  if (/^game not found$|未找到游戏信息/i.test(raw)) {
    return t.notFound
  }
  if (/^username is required$|username 为必填/i.test(raw)) {
    return t.usernameRequired
  }
  if (
    code === 'agent_processing_failed' ||
    /^processing failed$/i.test(raw) ||
    /^处理失败/.test(raw) ||
    /抱歉，这次没能完成你的请求|抱歉，执行时遇到了问题|没能执行成功|执行过程中遇到问题/.test(
      raw,
    )
  ) {
    return t.agentProcessingFailed
  }
  if (
    /^the scheduled task failed$|^the task failed$|^failed$|前端任务执行失败|^任务执行失败$|^任务未完成$|^任务失败$|^未知错误$|^失败$/.test(
      raw,
    )
  ) {
    return t.agentProcessingFailed
  }
  if (
    /^the failed step was skipped$|用户选择跳过错误步骤/.test(raw)
  ) {
    return t.agentProcessingFailed
  }
  if (
    /^the failed step will be retried$|用户选择重试失败步骤/.test(raw)
  ) {
    return t.agentProcessingFailed
  }
  if (/^confirmation failed$/i.test(raw) || /^确认执行失败/.test(raw)) {
    return t.agentProcessingFailed
  }
  if (
    /this action is not supported|that platform is not supported|不支持此操作|不支持的平台名称/i.test(
      raw,
    )
  ) {
    return t.agentUnsupported
  }
  if (
    /^this confirmation expired$|确认请求已过期/i.test(raw)
  ) {
    return t.agentConfirmExpired
  }
  if (
    /this confirmation is no longer available|确认请求不存在/i.test(raw)
  ) {
    return t.agentConfirmMissing
  }
  if (
    /^the task was cancelled$|任务已被取消|任务已被用户取消|操作已取消/i.test(
      raw,
    )
  ) {
    return t.agentTaskCancelled
  }
  if (
    /^the task was interrupted$|任务因服务重启/i.test(raw)
  ) {
    return t.agentTaskInterrupted
  }
  if (
    /^the step timed out$|执行超时/i.test(raw)
  ) {
    return t.agentStepTimeout
  }
  if (/^input is empty$|^输入不能为空$/.test(raw)) {
    return t.agentInputEmpty
  }
  if (/^input is too long$|输入过长/.test(raw)) {
    return t.agentInputTooLong
  }
  if (
    /could not subscribe to any|尝试了 .* 个源都无法订阅/i.test(raw)
  ) {
    return t.subscribeAllFailed
  }
  if (
    /this address is not allowed|不允许访问内网|不允许的 url scheme/i.test(
      raw,
    )
  ) {
    return t.privateNetworkBlocked
  }
  if (
    /this url is missing a host|url 缺少 host/i.test(raw)
  ) {
    return t.invalidUrl
  }
  if (
    /^missing url or feeds$|^no feed url to try$|缺少 url 或 feeds|没有可用的订阅源/i.test(
      raw,
    )
  ) {
    return t.subscribeAllFailed
  }
  if (
    /^too many items to write at once$|单次最多写入/i.test(raw)
  ) {
    return t.writeItemsOverCap
  }
  if (
    /^missing text for speech$|缺少 text 参数，无法进行文字转语音/.test(
      raw,
    )
  ) {
    return t.emptyDialogueText
  }
  if (/^missing music action$|缺少 action 参数/.test(raw)) {
    return t.agentInputEmpty
  }
  if (
    /^this article cannot be changed$|无权操作该文章/.test(raw)
  ) {
    return t.forbidden
  }
  if (
    /^feed not found$|^no feeds are available$|^that feed or author was not found$|^that was not found in subscribed feeds$|^no matching rss feed was found$|未找到匹配|系统中暂无订阅源|未在已订阅源中找到|未能找到匹配的 RSS|未找到名为/.test(
      raw,
    )
  ) {
    return t.feedNotFound
  }
  if (
    /^no matching articles were found$|未找到符合条件的文章/.test(raw)
  ) {
    return t.notFound
  }
  if (
    /^no matching playlist was found$|没有找到相关歌单/.test(raw)
  ) {
    return t.notFound
  }
  if (/^article not found$|未找到文章/.test(raw)) {
    return t.notFound
  }
  if (/^task not found$|任务不存在或无权访问/.test(raw)) {
    return t.notFound
  }
  if (
    /could not set up rsshub|could not read rsshub|no rsshub instances|初始化 RSSHub|读取 RSSHub|RSSHub 实例表为空/i.test(
      raw,
    )
  ) {
    return t.rsshubUnavailable
  }
  if (
    /^too many pipeline steps$|管道步骤数不能超过/.test(raw)
  ) {
    return t.pipelineTooManySteps
  }
  if (
    /^heartbeat admin required$|Heartbeat 管理需要管理员/.test(raw)
  ) {
    return t.heartbeatAdminRequired
  }
  if (
    /^this step needs confirmation first$|需要人工确认|未经确认的高风险/.test(
      raw,
    )
  ) {
    return t.stepNeedsConfirm
  }
  if (
    /^missing user intent$|Missing userIntent|请描述你想要执行的操作/.test(
      raw,
    )
  ) {
    return t.agentInputEmpty
  }
  if (
    /^missing url or query$|需要提供 url 或 query/.test(raw)
  ) {
    return t.agentInputEmpty
  }
  if (
    /^this url is invalid$|输入不是有效的 URL/.test(raw)
  ) {
    return t.invalidUrl
  }
  if (
    /^this step cannot be called directly$|不应被直接调用/.test(raw)
  ) {
    return t.agentUnsupported
  }
  if (/^the system is shutting down$|系统正在关闭/.test(raw)) {
    return t.agentTaskInterrupted
  }
  if (
    /^agent is admin only$|^agent chat is not enabled|Agent 仅管理员|未对普通用户开放/.test(
      raw,
    )
  ) {
    return t.forbidden
  }
  if (
    /^this service is not configured$|API Key 未配置|图片生成完成，但无法提取/i.test(
      raw,
    )
  ) {
    return /TTS|语音|Speech/.test(raw) ? t.speechNotConfigured : t.agentProcessingFailed
  }
  if (
    /^invalid tappid$/i.test(raw) ||
    /无效的 tappId/.test(raw)
  ) {
    return currentCopy().tapp.invalidId
  }
  if (
    code === 'media_action_invalid' ||
    code === 'media_mode_invalid' ||
    /^invalid (action|mode)$/i.test(raw)
  ) {
    return t.operationFailed
  }
  if (
    /^invalid url$/i.test(raw) ||
    /^无效的 URL/.test(raw)
  ) {
    return t.invalidUrl
  }
  if (
    code === 'federation_move_failed' ||
    /failed to move federation identity|shared keys \(G\)|local rewrite \(E\)/i.test(
      raw,
    )
  ) {
    return t.federationMoveFailed
  }
  if (
    /failed to initialize federation identity|failed to read user id/i.test(
      raw,
    )
  ) {
    return t.operationFailed
  }
  if (
    /^(channel|room|ring|transfer|activity|object|user) not found$/i.test(
      raw,
    )
  ) {
    return t.notFound
  }
  if (
    /transfer is (not ready|already )|channel already closed|channel must be closed/i.test(
      raw,
    )
  ) {
    return t.channelNotReady
  }
  if (
    /unsupported attachment (mime|type)|unsupported content type|invalid attachment url|attachment url/i.test(
      raw,
    )
  ) {
    return t.invalidUrl
  }
  if (
    / is required$| are required$|key rotation requires confirm/i.test(raw)
  ) {
    return t.agentInputEmpty
  }
  if (
    code === 'notion_url_invalid' ||
    /invalid notion url|unknown resource type/i.test(raw)
  ) {
    return t.notionUrlInvalid
  }
  if (
    code === 'channel_not_ready' ||
    /channel is .+, cannot (send|transfer)/i.test(raw)
  ) {
    return t.channelNotReady
  }
  if (
    code === 'invite_invalid_status' ||
    /cannot accept (invite|this invite)|channel is .+, cannot accept/i.test(raw)
  ) {
    return t.inviteInvalid
  }
  if (
    /^feed name is required$|订阅源名称不能为空/.test(raw)
  ) {
    return t.feedNameRequired
  }
  if (
    /unable to reach or parse this rss|无法访问或解析此 RSS|no rsshub instance|rsshub instance not found|没有配置的 RSSHub|RSSHub 实例不存在/i.test(
      raw,
    )
  ) {
    return currentCopy().brew.errorDiscoverFailed
  }
  if (
    /notion is not configured|notion api key 未配置/i.test(raw)
  ) {
    return currentCopy().brew.errorNotionFetch
  }
  if (/^failed to submit refresh$|^提交失败/.test(raw)) {
    return t.taskSubmitFailed
  }
  const arael = currentCopy().arael
  if (code === 'preset_title_too_long' || /标题过长|title is too long/i.test(raw)) {
    return arael.presetTitleTooLong
  }
  if (code === 'preset_summary_too_long' || /摘要过长|summary is too long/i.test(raw)) {
    return arael.presetSummaryTooLong
  }
  if (code === 'preset_steps_too_large' || /解析步骤数据过大|parsed steps are too large/i.test(raw)) {
    return arael.presetStepsTooLarge
  }
  if (
    code === 'preset_history_too_long' ||
    /对话历史过长|conversation history is too long/i.test(raw)
  ) {
    return arael.presetHistoryTooLong
  }
  if (
    code === 'notification_unavailable' ||
    /^notification system not initialized$/i.test(raw)
  ) {
    return t.notificationActionFailed
  }
  const setup = currentCopy().setup
  if (
    code === 'db_migration_failed' ||
    /^database migration failed$/i.test(raw) ||
    /数据库迁移失败/.test(raw)
  ) {
    return setup.dbMigrationFailed
  }
  if (code === 'schema_ensure_failed' || /^schema ensure failed$/i.test(raw)) {
    return setup.schemaEnsureFailed
  }
  if (
    code === 'setup_cleanup_failed' ||
    /^setup window cleanup failed$/i.test(raw) ||
    /无法持久化安装关闭/.test(raw)
  ) {
    return setup.cleanupFailed
  }
  if (
    code === 'setup_claim_failed' ||
    /failed to write setup claim marker/i.test(raw) ||
    /无法写入安装认领标记/.test(raw)
  ) {
    return setup.claimFailed
  }
  if (code === 'config_mode_required' || /只能在配置模式下修改/.test(raw)) {
    return setup.configModeRequired
  }
  if (
    code === 'schedule_invalid' ||
    /^invalid schedule (config|type)/i.test(raw) ||
    /^invalid (execution target|missed policy|scope)/i.test(raw)
  ) {
    return currentCopy().tapp.unknownError
  }
  if (/^task ['"]?[^'"]+['"]? not found$/i.test(raw)) {
    return t.notFound
  }
  if (
    /stored rig is invalid|rig character asset|rig compilation failed|rig manifest migration|invalid rig (import|source|atlas|analysis)|invalid portrait/i.test(
      raw,
    )
  ) {
    return currentCopy().merope.visualFailed
  }
  if (
    code === 'GAME_CONFIG_INVALID' ||
    /invalid game (tapp id|protocol)|game\.max_players|game\.max_message_bytes/i.test(
      raw,
    )
  ) {
    return t.gameConfigInvalid
  }
  if (
    code === 'GAME_MESSAGE_INVALID' ||
    /game message_type|game session messages|not a game session|game payload too large/i.test(
      raw,
    )
  ) {
    return t.gameMessageInvalid
  }
  if (/^failed to fetch feed$/i.test(raw)) {
    return currentCopy().brew.errorDiscoverFailed
  }

  const byStatus = status > 0 ? httpStatusMessage(status) : ''
  const useful = isUselessErrorText(raw) ? '' : clip(raw)
  const extraHint = usefulExtra(hint, byStatus, useful, fallbackText)

  if (byStatus && useful && useful !== byStatus) {
    return joinParts(byStatus, useful, extraHint)
  }
  if (useful) return joinParts(useful, extraHint)
  if (byStatus) return joinParts(byStatus, extraHint)
  return joinParts(fallbackText, extraHint)
}

function usefulExtra(text: string, ...known: string[]): string {
  if (!text || isUselessErrorText(text)) return ''
  if (known.some((item) => item && text === item)) return ''
  return clip(text)
}
