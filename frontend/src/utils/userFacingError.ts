import { currentCopy, formatCurrent } from '../i18n/localeCopy'
import { ApiError } from '../services/api'
import { resolveErrorCode } from './errorCodes'
import { httpStatusMessage, statusFromErrorText } from './httpStatus'
import { isInternalDump, isUselessErrorText } from './uselessErrorText'

export { httpStatusMessage, isUselessErrorText, statusFromErrorText }

function fill(
  template: string,
  params: Record<string, string | number> = {},
): string {
  return formatCurrent(template, params)
}

function readHint(reason: unknown): string {
  if (
    reason &&
    typeof reason === 'object' &&
    Object.hasOwn(reason, 'hint') &&
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
    Object.hasOwn(reason, 'status') &&
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
    Object.hasOwn(reason, 'code') &&
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

/** Category label plus any leftover that still helps the user locate the fault. */
function classified(label: string, raw: string, hint = ''): string {
  const colon = raw.indexOf(':')
  const rest = colon >= 0 ? raw.slice(colon + 1).trim() : ''
  const keep =
    rest && !isInternalDump(rest) && !isUselessErrorText(rest) ? clip(rest) : ''
  const size = raw.match(/\d+\s*MB/i)?.[0] || ''
  const png = /\bPNG\b/i.test(raw) && /must be/i.test(raw) ? 'PNG' : ''
  return joinParts(
    label,
    keep,
    size && !keep.includes(size) ? size : '',
    png && !keep.toUpperCase().includes('PNG') ? png : '',
    usefulExtra(hint, label, keep),
  )
}

/** Localized, diagnosable copy. New faults need a machine `code`; leftover regex is last-resort. */
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
  const code = resolveErrorCode(readCode(reason), raw)
  const hint = readHint(reason)

  if (code === 'unauthorized' || code === 'UNAUTHORIZED') {
    return joinParts(t.unauthorized, usefulExtra(hint, t.unauthorized))
  }
  if (code === 'forbidden' || code === 'FORBIDDEN' || code === 'no_admin') {
    return joinParts(t.forbidden, usefulExtra(hint, t.forbidden))
  }
  if (
    code === 'federation_disabled_region' ||
    /federation is not supported in this region/i.test(raw) ||
    /federation is disabled because this server['’]s egress location is mainland china/i.test(
      raw,
    ) ||
    /^federation disabled in this region$/i.test(raw) ||
    /^federation is disabled on this instance$/i.test(raw) ||
    /federation apps cannot be downloaded or installed/i.test(raw)
  ) {
    return t.federationDisabledRegion
  }
  if (code === 'not_found' || code === 'NOT_FOUND') {
    return joinParts(t.notFound, usefulExtra(hint, t.notFound))
  }
  if (code === 'locale_invalid') {
    return joinParts(t.localeInvalid, usefulExtra(hint, t.localeInvalid))
  }
  if (code === 'locale_save_failed') {
    return classified(t.operationFailed, raw, hint)
  }
  if (code === 'config_load_failed') {
    return classified(currentCopy().config.loadConfigFailed, raw, hint)
  }
  if (code === 'permissions_save_failed') {
    return classified(currentCopy().config.permissionsSaveFailed, raw, hint)
  }
  if (code === 'tapp_save_failed') {
    return classified(t.tappSaveFailed, raw, hint)
  }
  if (code === 'account_update_failed') {
    return classified(currentCopy().config.usersUpdateFailed, raw, hint)
  }
  if (code === 'users_load_failed') {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(
      joinParts(currentCopy().config.usersLoadError, action),
      raw,
      hint,
    )
  }
  if (code === 'account_delete_failed') {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(
      joinParts(currentCopy().config.usersDeleteFailed, action),
      raw,
      hint,
    )
  }
  if (code === 'identity_unlink_failed') {
    return classified(currentCopy().config.usersUnlinkFailed, raw, hint)
  }
  if (code === 'bad_request' || code === 'BAD_REQUEST') {
    const label = httpStatusMessage(400)
    return joinParts(label, usefulExtra(hint, label))
  }
  if (code === 'conflict' || code === 'CONFLICT') {
    const label = httpStatusMessage(409)
    return joinParts(label, usefulExtra(hint, label))
  }
  if (code === 'internal_error' || code === 'INTERNAL_ERROR') {
    const label = fill(t.serverError, { status: 500 })
    return joinParts(label, usefulExtra(hint, label))
  }
  if (code === 'service_unavailable' || code === 'SERVICE_UNAVAILABLE') {
    return joinParts(
      t.serviceUnavailable,
      usefulExtra(hint, t.serviceUnavailable),
    )
  }
  if (code === 'configuration_mode') {
    return joinParts(
      t.configurationMode,
      usefulExtra(hint, t.configurationMode),
    )
  }
  if (code === 'setup_completed') {
    return joinParts(t.setupCompleted, usefulExtra(hint, t.setupCompleted))
  }
  if (code === 'username_required') {
    return joinParts(t.usernameRequired, usefulExtra(hint, t.usernameRequired))
  }
  if (code === 'uid_required') {
    return joinParts(t.uidRequired, usefulExtra(hint, t.uidRequired))
  }
  if (code === 'invalid_uid') {
    return joinParts(t.invalidUid, usefulExtra(hint, t.invalidUid))
  }
  if (code === 'steam_credentials_required') {
    return joinParts(t.steamNotConfigured, usefulExtra(hint, t.steamNotConfigured))
  }
  if (code === 'youtube_credentials_required') {
    return joinParts(
      t.youtubeCredentialsRequired,
      usefulExtra(hint, t.youtubeCredentialsRequired),
    )
  }
  if (code === 'youtube_api_key_required') {
    return joinParts(
      t.youtubeApiKeyRequired,
      usefulExtra(hint, t.youtubeApiKeyRequired),
    )
  }
  if (code === 'user_id_required') {
    return joinParts(t.userIdRequired, usefulExtra(hint, t.userIdRequired))
  }
  if (code === 'invalid_user_id') {
    return joinParts(t.invalidUserId, usefulExtra(hint, t.invalidUserId))
  }
  if (
    code === 'bangumi_credentials_required'
  ) {
    return joinParts(
      t.bangumiCredentialsRequired,
      usefulExtra(hint, t.bangumiCredentialsRequired),
    )
  }
  if (code === 'discord_token_required') {
    return joinParts(
      t.discordTokenRequired,
      usefulExtra(hint, t.discordTokenRequired),
    )
  }
  if (code === 'bearer_token_required') {
    return joinParts(
      t.bearerTokenRequired,
      usefulExtra(hint, t.bearerTokenRequired),
    )
  }
  if (code === 'gamertag_required') {
    return joinParts(t.gamertagRequired, usefulExtra(hint, t.gamertagRequired))
  }
  if (code === 'xbox_api_key_required') {
    return joinParts(t.xboxApiKeyRequired, usefulExtra(hint, t.xboxApiKeyRequired))
  }
  if (code === 'online_id_required') {
    return joinParts(t.onlineIdRequired, usefulExtra(hint, t.onlineIdRequired))
  }
  if (code === 'npsso_required') {
    return joinParts(t.npssoRequired, usefulExtra(hint, t.npssoRequired))
  }
  if (code === 'platform_test_unimplemented') {
    return joinParts(
      t.platformTestUnimplemented,
      usefulExtra(hint, t.platformTestUnimplemented),
    )
  }
  if (
    code === 'site_owner_missing'
  ) {
    return joinParts(t.siteOwnerMissing, usefulExtra(hint, t.siteOwnerMissing))
  }
  if (
    code === 'x_bearer_not_configured' ||
    code === 'bearer_token_required'
  ) {
    return joinParts(
      t.bearerTokenRequired,
      usefulExtra(hint, t.bearerTokenRequired),
    )
  }
  if (code === 'x_username_required') {
    return joinParts(t.usernameRequired, usefulExtra(hint, t.usernameRequired))
  }
  if (code === 'discord_token_not_configured') {
    return joinParts(
      t.discordTokenRequired,
      usefulExtra(hint, t.discordTokenRequired),
    )
  }
  if (code === 'discord_app_not_configured') {
    return joinParts(
      currentCopy().config.discordOAuthAppMissing,
      usefulExtra(hint, currentCopy().config.discordOAuthAppMissing),
    )
  }
  if (
    code === 'bearer_token_not_allowed' ||
    code === 'access_token_not_allowed'
  ) {
    return joinParts(
      t.queryTokenNotAllowed,
      usefulExtra(hint, t.queryTokenNotAllowed),
    )
  }
  if (code === 'share_text_empty') {
    return joinParts(t.shareTextEmpty, usefulExtra(hint, t.shareTextEmpty))
  }
  if (
    code === 'module_visibility_save_failed'
  ) {
    return joinParts(
      t.moduleVisibilitySaveFailed,
      usefulExtra(hint, t.moduleVisibilitySaveFailed),
    )
  }
  if (code === 'invalid_platform') {
    return classified(t.invalidPlatform, raw, hint)
  }
  if (code === 'hitokoto_save_failed') {
    return joinParts(
      currentCopy().config.hitokotoSaveFailed,
      usefulExtra(hint, currentCopy().config.hitokotoSaveFailed),
    )
  }
  if (
    code === 'report_settings_save_failed'
  ) {
    return joinParts(
      currentCopy().config.reportSettingsSaveFailed,
      usefulExtra(hint, currentCopy().config.reportSettingsSaveFailed),
    )
  }
  if (
    code === 'no_permission_settings'
  ) {
    return joinParts(
      t.noPermissionSettings,
      usefulExtra(hint, t.noPermissionSettings),
    )
  }
  if (code === 'no_valid_report') {
    return joinParts(t.noValidReport, usefulExtra(hint, t.noValidReport))
  }
  if (code === 'file_too_large' || /^file size must be between/i.test(raw)) {
    return classified(t.fileTooLarge, raw, hint)
  }
  if (
    code === 'payload_too_large' ||
    /^message payload too large/i.test(raw)
  ) {
    return classified(t.payloadTooLarge, raw, hint)
  }
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
  if (code === 'password_too_short') return t.passwordMinLength
  if (code === 'password_too_long') return currentCopy().auth.passwordLengthError
  if (code === 'password_needs_letter_and_digit') {
    return currentCopy().userModal.passwordNeedsLetterAndDigit
  }
  if (code === 'password_failed') {
    return joinParts(t.passwordFailed, usefulExtra(hint, t.passwordFailed))
  }
  if (code === 'session_failed') {
    return joinParts(t.sessionFailed, usefulExtra(hint, t.sessionFailed))
  }
  if (
    /^failed to (list sessions|find session|load session messages)/i.test(raw)
  ) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.agentSessionLoadFailed, action), raw, hint)
  }
  if (
    /^failed to (create session|update session|save user message|save assistant message)/i.test(
      raw,
    )
  ) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.agentSessionSaveFailed, action), raw, hint)
  }
  if (/^failed to archive session/i.test(raw)) {
    return classified(t.agentSessionArchiveFailed, raw, hint)
  }
  if (
    /^failed to (count persona reports|load persona)/i.test(
      raw,
    )
  ) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.personaLoadFailed, action), raw, hint)
  }
  if (
    /^failed to (begin persona save|save persona|update persona portrait|commit persona save)/i.test(
      raw,
    )
  ) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.personaSaveFailed, action), raw, hint)
  }
  if (
    /^failed to (begin persona delete|delete persona|clear persona portrait|commit persona delete)/i.test(
      raw,
    )
  ) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.personaDeleteFailed, action), raw, hint)
  }
  if (/^failed to load addressee/i.test(raw)) {
    return classified(t.addresseeLoadFailed, raw, hint)
  }
  if (/^failed to (save addressee|save quiet-hours)/i.test(raw)) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.addresseeSaveFailed, action), raw, hint)
  }
  if (
    code === 'preset_fetch_failed' ||
    /^failed to (fetch favorites|fetch history|find preset|check existing preset)/i.test(
      raw,
    )
  ) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.presetLoadFailed, action), raw, hint)
  }
  if (
    code === 'preset_update_failed' ||
    /^failed to (create preset|update preset|toggle favorite)/i.test(raw)
  ) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.presetSaveFailed, action), raw, hint)
  }
  if (/^failed to delete preset/i.test(raw)) {
    return classified(t.presetDeleteFailed, raw, hint)
  }
  if (
    code === 'account_load_failed' ||
    /^failed to (look up account|load current user|check existing admin|check username|read installation claim|read user data)/i.test(
      raw,
    )
  ) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.accountLoadFailed, action), raw, hint)
  }
  if (
    /^failed to (begin admin setup|lock admin setup|create admin account)/i.test(
      raw,
    )
  ) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.accountSaveFailed, action), raw, hint)
  }
  if (/^failed to (change password|set password)/i.test(raw)) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.passwordChangeFailed, action), raw, hint)
  }
  if (/^failed to update local login/i.test(raw)) {
    return classified(t.localLoginSaveFailed, raw, hint)
  }
  if (
    /^failed to (check owner|list users|list user identities|find user|list user apps|count admins|count identities|load identity)/i.test(
      raw,
    )
  ) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(
      joinParts(currentCopy().config.usersLoadError, action),
      raw,
      hint,
    )
  }
  if (/^failed to update user/i.test(raw)) {
    return classified(currentCopy().config.usersUpdateFailed, raw, hint)
  }
  if (/^failed to unlink identity/i.test(raw)) {
    return classified(currentCopy().config.usersUnlinkFailed, raw, hint)
  }
  if (
    /^failed to (begin user delete|cleanup user data|delete user|rollback user delete|commit user delete)/i.test(
      raw,
    )
  ) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(
      joinParts(currentCopy().config.usersDeleteFailed, action),
      raw,
      hint,
    )
  }
  if (code === 'account_create_failed') {
    return joinParts(
      currentCopy().auth.registerFailed,
      usefulExtra(hint, currentCopy().auth.registerFailed),
    )
  }
  if (
    code === 'config_file_read_failed'
  ) {
    return classified(t.configFileReadFailed, raw, hint)
  }
  if (
    code === 'config_file_permission'
  ) {
    return classified(t.configFilePermission, raw, hint)
  }
  if (
    code === 'ai_response_invalid' ||
    /^failed to parse ai response$/i.test(raw)
  ) {
    return joinParts(t.aiResponseInvalid, usefulExtra(hint, t.aiResponseInvalid))
  }
  if (
    /^(annotation generation|podcast script generation|smart filter|content comparison|prompt generation|translation|code explanation|ai abstraction|skill ai generation|skill ai planning|ui analysis) failed/i.test(
      raw,
    ) ||
    /^failed to parse skill ai json/i.test(raw)
  ) {
    const action =
      raw.match(
        /^(annotation generation|podcast script generation|smart filter|content comparison|prompt generation|translation|code explanation|ai abstraction|skill ai generation|skill ai planning|ui analysis)/i,
      )?.[1] ||
      (/parse skill ai json/i.test(raw) ? 'skill AI JSON' : '')
    const status = raw.match(/\bHTTP\s+(\d{3})\b/i)
    const colon = raw.indexOf(':')
    const rest = colon >= 0 ? raw.slice(colon + 1).trim() : ''
    const keep =
      rest && !isInternalDump(rest) && !isUselessErrorText(rest) ? clip(rest) : ''
    const http = status ? `HTTP ${status[1]}` : ''
    return joinParts(
      t.aiStepFailed,
      action,
      keep && keep !== http ? keep : '',
      http,
      usefulExtra(hint, t.aiStepFailed, action, keep, http),
    )
  }
  if (
    code === 'ai_generation_failed' ||
    /^ai error:/i.test(raw) ||
    /^ai generation failed$/i.test(raw) ||
    /^ai (analysis|search) failed$/i.test(raw) ||
    /^gemini api /i.test(raw) ||
    /invalid gemini json/i.test(raw)
  ) {
    return joinParts(t.aiGenerationFailed, usefulExtra(hint, t.aiGenerationFailed))
  }
  if (/skill improvement on cooldown/i.test(raw)) {
    const wait = raw.match(/(\d+) seconds remaining/i)?.[1]
    return joinParts(
      t.skillCooldown,
      wait ? `${wait}s` : '',
      usefulExtra(hint, t.skillCooldown),
    )
  }
  if (/^invalid skill file format/i.test(raw)) {
    return t.skillFileInvalid
  }
  if (
    /^failed to (backup|read|write|replace) skill/i.test(raw) ||
    /^failed to (create skill trash directory|move skill to trash)/i.test(raw) ||
    /^skill file missing/i.test(raw)
  ) {
    return classified(t.skillFileFailed, raw, hint)
  }
  if (code === 'settings_backup_failed') {
    return joinParts(
      t.settingsBackupRestoreFailed,
      usefulExtra(hint, t.settingsBackupRestoreFailed),
    )
  }
  if (
    /^export_hash_failed$|^export_integrity_failed$|^integrity_seal_failed/i.test(
      raw,
    )
  ) {
    return currentCopy().config.analytics.exportFailed
  }
  if (
    /^invalid tapp archive/i.test(raw) ||
    /^invalid manifest(\.json)?/i.test(raw) ||
    /invalid \.tapp file/i.test(raw) ||
    /manifest\.json (not found|uses the pre-layer format)/i.test(raw) ||
    /invalid agent schema/i.test(raw) ||
    /install package is missing/i.test(raw) ||
    /missing content for declared/i.test(raw)
  ) {
    return currentCopy().tapp.installFailed
  }
  if (code === 'steering_unavailable') {
    return classified(t.agentSteeringFailed, raw, hint)
  }
  if (code === 'dnd_schedule_invalid' || /invalid do-not-disturb/i.test(raw)) {
    return t.dndScheduleInvalid
  }
  if (
    code === 'dnd_schedule_incomplete' ||
    /set both start and end, or clear both/i.test(raw)
  ) {
    return t.dndScheduleIncomplete
  }
  if (code === 'merope_disabled') {
    return currentCopy().agentPanel.agentPersonaOff
  }
  if (
    code === 'consent_required'
  ) {
    return t.stepNeedsConfirm
  }
  if (
    code === 'GUEST_LAYOUT_READONLY' ||
    code === 'TAPP_PERMISSION_NOT_GRANTED' ||
    code === 'site_owner_required' ||
    code === 'admin_required' ||
    /guests cannot /i.test(raw)
  ) {
    return t.forbidden
  }
  if (code === 'login_required') {
    return t.unauthorized
  }
  if (code === 'lyrics_fetch_failed') {
    return fill(t.lyricsFailed, { status: status || 502 })
  }
  if (code === 'playlist_fetch_failed') {
    return classified(currentCopy().music.loadPlaylistFailed, raw, hint)
  }
  if (code === 'song_fetch_failed') {
    return classified(currentCopy().music.loadSongFailed, raw, hint)
  }
  if (code === 'hitokoto_fetch_failed' || /^hitokoto api failed$/i.test(raw)) {
    return currentCopy().config.hitokotoLoadFailed
  }
  if (
    /failed to fetch (hitokoto|bilibili|bangumi|steam|weather|netease|game details)/i.test(
      raw,
    ) ||
    /^(bilibili|bangumi|weather|netease|steam) api error/i.test(raw) ||
    /获取\s*(Steam|Bilibili|Bangumi|Hitokoto|天气|网易)/i.test(raw)
  ) {
    const name = /hitokoto/i.test(raw)
      ? 'Hitokoto'
      : /bilibili/i.test(raw)
        ? 'Bilibili'
        : /bangumi/i.test(raw)
          ? 'Bangumi'
          : /steam|game details/i.test(raw)
            ? 'Steam'
            : /weather|天气/i.test(raw)
              ? 'Weather'
              : /netease|网易/i.test(raw)
                ? 'Netease'
                : 'Platform'
    const label = fill(t.platformNamedFetchFailed, { name })
    const status = raw.match(/\bHTTP\s+(\d{3})\b/i)
    const colon = raw.indexOf(':')
    const rest = colon >= 0 ? raw.slice(colon + 1).trim() : ''
    const keep =
      rest && !isInternalDump(rest) && !isUselessErrorText(rest) ? clip(rest) : ''
    const http = status ? `HTTP ${status[1]}` : ''
    return joinParts(
      label,
      keep && keep !== http ? keep : '',
      http,
      usefulExtra(hint, label, keep, http),
    )
  }
  if (
    /^no cached \w+ data/i.test(raw) ||
    /^no data available for platform:/i.test(raw) ||
    /^platform data not found$/i.test(raw)
  ) {
    const found =
      raw.match(/^no cached (\w+) data/i)?.[1] ||
      raw.match(/platform:\s*(\w+)/i)?.[1] ||
      'platform'
    const name = `${found.charAt(0).toUpperCase()}${found.slice(1)}`
    return joinParts(
      fill(t.platformCacheMissing, { name }),
      usefulExtra(hint, t.platformCacheMissing),
    )
  }
  if (
    /^failed to (read|parse) \w+ data/i.test(raw) ||
    /^failed to (read|parse) cache/i.test(raw)
  ) {
    const found = raw.match(/^failed to (?:read|parse) (\w+) data/i)?.[1] || 'platform'
    const name = `${found.charAt(0).toUpperCase()}${found.slice(1)}`
    return classified(
      fill(t.platformNamedFetchFailed, { name }),
      raw,
      hint,
    )
  }
  if (
    /^http request failed/i.test(raw) ||
    /^request failed(:|$)/i.test(raw) ||
    /^fetch failed/i.test(raw) ||
    /^read failed/i.test(raw) ||
    /^http client error/i.test(raw) ||
    /^invalid outbound proxy/i.test(raw)
  ) {
    return classified(t.networkError, raw, hint)
  }
  if (/^dns resolution failed/i.test(raw)) {
    return classified(t.dnsFailed, raw, hint)
  }
  if (
    /^failed to parse json/i.test(raw) ||
    /^json parse failed/i.test(raw) ||
    /^invalid (ai chat messages|json from upstream)/i.test(raw) ||
    /^failed to serialize json body/i.test(raw)
  ) {
    return classified(t.aiResponseInvalid, raw, hint)
  }
  if (/^failed to (consume|persist|load) confirmation/i.test(raw)) {
    return classified(t.agentConfirmMissing, raw, hint)
  }
  if (
    /persist tapp interaction wait/i.test(raw) ||
    /failed to (create tapp staging|activate staged)/i.test(raw) ||
    (/tapp/i.test(raw) &&
      /storage is not writable|not enough disk space/i.test(raw))
  ) {
    return classified(t.tappSaveFailed, raw, hint)
  }
  if (/^failed to serialize manifest/i.test(raw)) {
    return classified(currentCopy().tapp.installFailed, raw, hint)
  }
  if (/^tapp generation failed/i.test(raw)) {
    return classified(t.tappGenerateFailed, raw, hint)
  }
  if (/^failed to fetch tapps/i.test(raw)) {
    return classified(currentCopy().tapp.loadAppFailed, raw, hint)
  }
  if (
    /^failed to (load|list) (the )?app list/i.test(raw) ||
    /failed to load site tapp catalog/i.test(raw)
  ) {
    return joinParts(
      currentCopy().tapp.listLoadFailed,
      status ? `HTTP ${status}` : '',
      usefulExtra(hint, currentCopy().tapp.listLoadFailed),
    )
  }
  if (/tapp \S+ is already installed/i.test(raw)) {
    return currentCopy().tapp.alreadyInstalled
  }
  if (
    /tapp \S+ is not installed/i.test(raw) ||
    /tapp \S+ is already being uninstalled/i.test(raw)
  ) {
    return currentCopy().tapp.appNotExist
  }
  if (/tapp \S+ requires permission reauthorization/i.test(raw)) {
    return currentCopy().tapp.reauthorizationMessage
  }
  if (
    /^failed to save (to cloud|window schemes)/i.test(raw)
  ) {
    return joinParts(
      currentCopy().tapp.schemeSaveFailed,
      status ? `HTTP ${status}` : '',
      usefulExtra(hint, currentCopy().tapp.schemeSaveFailed),
    )
  }
  if (/^failed to save dashboard layout/i.test(raw)) {
    return joinParts(
      t.dashboardLayoutSaveFailed,
      status ? `HTTP ${status}` : '',
      usefulExtra(hint, t.dashboardLayoutSaveFailed),
    )
  }
  if (/^failed to save dashboard title/i.test(raw)) {
    return joinParts(
      t.dashboardTitleSaveFailed,
      status ? `HTTP ${status}` : '',
      usefulExtra(hint, t.dashboardTitleSaveFailed),
    )
  }
  if (/^failed to save custom platforms/i.test(raw)) {
    return joinParts(
      t.customPlatformsSaveFailed,
      status ? `HTTP ${status}` : '',
      usefulExtra(hint, t.customPlatformsSaveFailed),
    )
  }
  if (/^failed to save control panel/i.test(raw)) {
    return joinParts(
      t.controlPanelSaveFailed,
      status ? `HTTP ${status}` : '',
      usefulExtra(hint, t.controlPanelSaveFailed),
    )
  }
  if (/^failed to save title style/i.test(raw)) {
    return joinParts(
      t.titleStyleSaveFailed,
      status ? `HTTP ${status}` : '',
      usefulExtra(hint, t.titleStyleSaveFailed),
    )
  }
  if (/^failed to save widget theme/i.test(raw)) {
    return joinParts(
      t.widgetThemeSaveFailed,
      status ? `HTTP ${status}` : '',
      usefulExtra(hint, t.widgetThemeSaveFailed),
    )
  }
  if (/^failed to (load|save) tapp settings/i.test(raw)) {
    const label = /load/i.test(raw)
      ? currentCopy().tapp.settingsLoadFailed
      : currentCopy().tapp.settingSaveFailed
    return joinParts(label, status ? `HTTP ${status}` : '', usefulExtra(hint, label))
  }
  if (
    /^failed to list agent reports/i.test(raw) ||
    /^failed to (load|fetch) reports?/i.test(raw)
  ) {
    return classified(t.reportLoadFailed, raw, hint)
  }
  if (/^failed to (create|update|delete) report/i.test(raw)) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.reportSaveFailed, action), raw, hint)
  }
  if (
    code === 'tapp_access_check_failed' ||
    /^failed to verify tapp access/i.test(raw)
  ) {
    return classified(t.tappAccessCheckFailed, raw, hint)
  }
  if (code === 'tapp_not_found') {
    return classified(t.tappFindFailed, raw, hint)
  }
  if (/^failed to check tapp install permission/i.test(raw)) {
    return classified(t.tappInstallCheckFailed, raw, hint)
  }
  if (
    code === 'TAPP_CREDENTIAL_LOAD_FAILED' ||
    /^failed to load (shortcuts|components|tapp credentials)/i.test(raw)
  ) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.tappResourceLoadFailed, action), raw, hint)
  }
  if (/^image too large/i.test(raw)) {
    return classified(t.imageTooLarge, raw, hint)
  }
  if (
    /storage preflight|create storage directory|write storage probe|list tapp owner directories|failed to inspect tapp resources/i.test(
      raw,
    )
  ) {
    return classified(t.storageNotWritable, raw, hint)
  }
  if (
    /failed to (create|write|flush|publish) cache|failed to remove generated image|failed to download image|failed to read image data/i.test(
      raw,
    )
  ) {
    return classified(t.imageCacheFailed, raw, hint)
  }
  if (/^mcp server timeout/i.test(raw)) {
    const method = raw.match(/method ['"]([^'"]+)['"]/i)?.[1] || ''
    return joinParts(t.mcpTimeout, method, usefulExtra(hint, t.mcpTimeout, method))
  }
  if (
    /^invalid (json-rpc|mcp initialize|mcp tools\/list|mcp tools\/call) response/i.test(
      raw,
    ) ||
    /^invalid (initialize|tools\/list|tools\/call) response/i.test(raw)
  ) {
    return classified(t.mcpResponseInvalid, raw, hint)
  }
  if (/^mcp (error|tool (error|failed))/i.test(raw)) {
    return classified(t.mcpToolFailed, raw, hint)
  }
  if (/^mcp server ['"][^'"]+['"] is not ready/i.test(raw)) {
    const id = raw.match(/mcp server ['"]([^'"]+)['"]/i)?.[1] || ''
    const state = raw.match(/not ready \(([^)]+)\)/i)?.[1] || ''
    return joinParts(t.mcpTalkFailed, id, state, usefulExtra(hint, t.mcpTalkFailed, id, state))
  }
  if (
    /^failed to (read from mcp|drain mcp|write to mcp|flush mcp|serialize mcp|write mcp)/i.test(
      raw,
    ) ||
    /^mcp (line is not valid utf-8|message exceeds|request exceeds|notification exceeds|server closed)/i.test(
      raw,
    ) ||
    /^json serialize error/i.test(raw) ||
    /^too many concurrent mcp/i.test(raw)
  ) {
    return classified(t.mcpTalkFailed, raw, hint)
  }
  if (/^upstream (request failed|http)/i.test(raw)) {
    return classified(
      fill(t.serverError, { status: status || 502 }),
      raw,
      hint,
    )
  }
  if (
    /^failed to save report/i.test(raw) ||
    /serialize report|insert report|report persist/i.test(raw)
  ) {
    return classified(t.reportSaveFailed, raw, hint)
  }
  if (/^failed to save reminder/i.test(raw)) {
    return classified(t.reminderSaveFailed, raw, hint)
  }
  if (/^failed to save note/i.test(raw)) {
    return classified(t.noteSaveFailed, raw, hint)
  }
  if (/^failed to save bookmark/i.test(raw)) {
    return classified(t.bookmarkSaveFailed, raw, hint)
  }
  if (
    /failed to (save|load|resolve) profile text|failed to list identities/i.test(
      raw,
    )
  ) {
    const label = /save/i.test(raw)
      ? t.profileTextSaveFailed
      : t.profileTextLoadFailed
    return classified(label, raw, hint)
  }
  if (/failed to (load|save) avatar source/i.test(raw)) {
    const label = /save/i.test(raw)
      ? t.avatarSourceSaveFailed
      : t.avatarSourceLoadFailed
    return classified(label, raw, hint)
  }
  if (/^failed to (load|save) avatar/i.test(raw)) {
    return classified(currentCopy().merope.loadFailed, raw, hint)
  }
  if (
    code === 'storage_read_failed' ||
    code === 'storage_save_failed' ||
    /^failed to (read|save) storage/i.test(raw) ||
    /^failed to (update|delete) tapp storage/i.test(raw)
  ) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.tappStorageFailed, action), raw, hint)
  }
  if (
    code === 'cache_clear_failed' ||
    /^failed to resolve site owner/i.test(raw) ||
    /^ai[_ ]task[_ ]registry/i.test(raw)
  ) {
    return classified(t.database, raw, hint)
  }
  if (
    code === 'update_failed' ||
    /^updater transport|^decode json failed|^updater upstream/i.test(raw)
  ) {
    return classified(t.noticeUpdaterFailed, raw, hint)
  }
  if (
    /^tripo |could not (load|create|query) tripo|invalid (glb json|3d model|tripo )|failed to store 3d/i.test(
      raw,
    ) ||
    code === 'TRIPO_ERROR'
  ) {
    return classified(t.model3dFailed, raw, hint)
  }
  if (/invalid credential binding/i.test(raw)) {
    return classified(t.unauthorized, raw, hint)
  }
  if (/output schema validation|AI_OUTPUT_SCHEMA_MISMATCH/i.test(raw)) {
    return classified(t.schemaMismatch, raw, hint)
  }
  if (
    /invalid backend action|invalid schedule config/i.test(raw) ||
    /^scheduled (tapp is no longer|task failed)/i.test(raw) ||
    /^all \d+ retries failed/i.test(raw) ||
    /no active tapp runtime callback/i.test(raw) ||
    /^(query|insert|delete|update) failed/i.test(raw) ||
    /^failed to .*(due tasks|due task|execution|scheduled task|scheduler|frontend dispatch|frontend pending|frontend stats|task stats|missed stats|audience|enqueue task|user role|delete storage|list connections)/i.test(
      raw,
    )
  ) {
    const action =
      raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() ||
      raw.match(/^(query|insert|delete|update) failed/i)?.[1]?.toLowerCase() ||
      ''
    return classified(
      joinParts(t.noticeScheduleFailed, action),
      raw,
      hint,
    )
  }
  if (/^player not found$/i.test(raw)) {
    return t.notFound
  }
  if (/^rate limited by upstream/i.test(raw)) {
    return t.rateLimited
  }
  if (code === 'youtube_upstream_failed') {
    return joinParts(
      t.youtubeUpstreamFailed,
      status ? `HTTP ${status}` : '',
      usefulExtra(hint, t.youtubeUpstreamFailed),
    )
  }
  if (
    code === 'e2e_key_failed' ||
    /^failed to seal e2e key/i.test(raw) ||
    /payload serialize|envelope (serialize|parse)|plaintext json parse|e2e (seal|unseal)|invalid remote e2e|e2e key wrap/i.test(
      raw,
    )
  ) {
    return classified(t.e2eKeyFailed, raw, hint)
  }
  if (code === 'config_save_failed') {
    return classified(t.configSaveFailed, raw, hint)
  }
  if (/^failed to reload configuration/i.test(raw)) {
    return classified(t.configReloadFailed, raw, hint)
  }
  if (
    /^failed to fetch public config/i.test(raw) ||
    /public config does not contain platforms/i.test(raw)
  ) {
    return joinParts(
      t.configFileReadFailed,
      status ? `HTTP ${status}` : '',
      usefulExtra(hint, t.configFileReadFailed),
    )
  }
  if (
    /failed to download |failed to fetch asset |store index is missing download path/i.test(
      raw,
    )
  ) {
    const name =
      raw.match(/download ([^(]+)/i)?.[1]?.trim() ||
      raw.match(/asset (\S+)/i)?.[1] ||
      raw.match(/required (\S+)/i)?.[1] ||
      ''
    const http = raw.match(/HTTP\s+(\d{3})/i)
    const label = fill(currentCopy().tapp.storeDownloadFailed, {
      name: name || 'asset',
    })
    return joinParts(
      label,
      http ? `HTTP ${http[1]}` : '',
      usefulExtra(hint, label),
    )
  }
  if (/store package version mismatch/i.test(raw)) {
    const catalog = raw.match(/catalog lists (\S+)/i)?.[1] || '?'
    const packed = raw.match(/manifest\.json is (\S+)/i)?.[1] || '?'
    return fill(currentCopy().tapp.storeVersionMismatch, {
      catalog,
      manifest: packed,
    })
  }
  const phantasi = currentCopy().phantasi
  if (
    code === 'notion_fetch_failed' ||
    /^failed to fetch notion/i.test(raw) ||
    /notion api error|failed to reach notion|failed to parse notion|failed to fetch page content/i.test(
      raw,
    )
  ) {
    const status = raw.match(/\bHTTP\s+(\d{3})\b/i)
    const colon = raw.indexOf(':')
    const rest = colon >= 0 ? raw.slice(colon + 1).trim() : ''
    const phrase = rest.replaceAll(/^HTTP\s+\d{3}\s*:?\s*/gi, '').trim()
    const keep =
      phrase && !isInternalDump(phrase) && !isUselessErrorText(phrase)
        ? clip(phrase)
        : ''
    const http = status ? `HTTP ${status[1]}` : ''
    return joinParts(
      phantasi.errorNotionFetch,
      keep && keep !== http ? keep : '',
      http,
      usefulExtra(hint, phantasi.errorNotionFetch, keep, http),
    )
  }
  if (
    code === 'feed_parse_failed' ||
    /^failed to parse feed\. please provide a name/i.test(raw)
  ) {
    return phantasi.errorFeedNeedName
  }
  if (/^failed to parse feed/i.test(raw)) {
    return classified(t.phantasiParseFailed, raw, hint)
  }
  if (
    code === 'feed_discover_failed' ||
    /^unable to discover rss/i.test(raw)
  ) {
    return classified(phantasi.errorDiscoverFailed, raw, hint)
  }
  if (/^invalid feed url/i.test(raw)) {
    return classified(t.phantasiInvalidUrl, raw, hint)
  }
  if (
    /^failed to load config(uration)?$/i.test(raw)
  ) {
    return classified(currentCopy().config.loadConfigFailed, raw, hint)
  }
  if (/^failed to (update|save) permissions/i.test(raw)) {
    return classified(currentCopy().config.permissionsSaveFailed, raw, hint)
  }
  if (/^failed to persist tapp/i.test(raw)) {
    return classified(t.tappSaveFailed, raw, hint)
  }
  if (
    code === 'mcp_config_save_failed' ||
    /^failed to save mcp config/i.test(raw) ||
    /serialize mcp config|create mcp config|write mcp config|replace mcp config/i.test(
      raw,
    )
  ) {
    return classified(currentCopy().config.mcpSaveFailed, raw, hint)
  }
  if (code === 'mcp_config_invalid') {
    return classified(currentCopy().config.mcpInvalidConfig, raw, hint)
  }
  if (
    /^invalid audio data$/i.test(raw) ||
    /^please (provide|upload) audio/i.test(raw) ||
    /无效的Base64|必须提供 audio_data|请上传音频|无效的音频数据/.test(raw)
  ) {
    return t.asrInvalidAudio
  }
  if (code === 'ai_not_configured' || code === 'AI_NOT_CONFIGURED' || /^(?:No AI provider configured|AI analyzer not configured)/i.test(raw)) {
    return t.aiNotConfigured
  }
  if (
    code === 'speech_not_configured' ||
    /speech service is not configured|语音服务未配置|TTS 服务未配置/i.test(raw)
  ) {
    return t.speechNotConfigured
  }
  if (
    code === 'realtime_session_unavailable'
  ) {
    return t.realtimeSessionUnavailable
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
    return raw.toLowerCase().includes('list') || raw.includes('对话列表')
      ? t.speechBatchEmpty
      : t.emptyDialogueText
  }
  if (/too many dialogues|对话数量超过限制/i.test(raw)) {
    return t.speechBatchTooMany
  }
  if (
    code === 'speech_upstream_failed' ||
    /speech service is unreachable|speech service request failed|invalid transcription json/i.test(
      raw,
    )
  ) {
    return t.speechUpstreamFailed
  }
  if (
    code === 'domain_invalid' ||
    /^invalid origin$/i.test(raw) ||
    /^invalid url:/i.test(raw) ||
    /^unsafe or invalid url/i.test(raw) ||
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
    /^failed to start (discord )?authorization/i.test(raw) ||
    /^state serialize/i.test(raw) ||
    /^HMAC key error/i.test(raw) ||
    /github (token|api|\/user)/i.test(raw) ||
    /^OIDC /i.test(raw) ||
    /^token (request|endpoint|JSON parse)/i.test(raw)
  ) {
    return t.oauthStartFailed
  }
  if (
    code === 'ROOM_MATERIALIZE_FAILED' ||
    /failed to (join|materialize) room/i.test(raw)
  ) {
    return t.roomJoinFailed
  }
  if (code === 'oauth_slug_required') {
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
    /psn npsso is empty|npsso exchange failed|cookie may be expired|psn_npsso not configured/i.test(
      raw,
    )
  ) {
    const statusMatch = raw.match(/\b(?:status\s+|HTTP\s+)(\d{3})\b/i)
    return joinParts(
      t.psnNpssoExpired,
      statusMatch ? `HTTP ${statusMatch[1]}` : '',
      usefulExtra(hint, t.psnNpssoExpired),
    )
  }
  if (/^psn (authorize|token)/i.test(raw) || /psn credential budget/i.test(raw)) {
    return classified(t.psnRequestFailed, raw, hint)
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
  if (code === 'API_NOT_FOUND' || code === 'api_not_found') {
    return joinParts(t.notFound, usefulExtra(hint, t.notFound))
  }
  if (code === 'INVALID_USER' || code === 'invalid_user') {
    return joinParts(t.unauthorized, usefulExtra(hint, t.unauthorized))
  }
  if (
    code === 'fetch_failed' &&
    /failed to fetch report/i.test(raw)
  ) {
    return joinParts(t.reportLoadFailed, usefulExtra(hint, t.reportLoadFailed))
  }
  if (
    code === 'fetch_failed' ||
    /failed to fetch data|获取失败|获取 .+失败|验证失败|解析响应失败|请求失败/i.test(
      raw,
    )
  ) {
    return classified(t.platformFetchFailed, raw, hint)
  }
  if (
    code === 'platform_refresh_reconcile_failed' ||
    /platform auto-refresh/i.test(raw) ||
    /failed to (load core platform tasks|disable stale core task|(update|create|disable) \w+ core task)/i.test(
      raw,
    )
  ) {
    const plat = raw.match(/failed to (?:update|create|disable) (\w+) core task/i)
    const name = plat?.[1]
      ? `${plat[1].charAt(0).toUpperCase()}${plat[1].slice(1)}`
      : 'Platform'
    return classified(
      fill(t.noticePlatformSyncFailed, { name }),
      raw,
      hint,
    )
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
    raw.startsWith('处理失败')
  ) {
    return classified(t.agentProcessingFailed, raw, hint)
  }
  if (
    /抱歉，这次没能完成你的请求|抱歉，执行时遇到了问题|没能执行成功|执行过程中遇到问题/.test(
      raw,
    )
  ) {
    return t.agentProcessingFailed
  }
  if (
    /^the scheduled task failed$|前端任务执行失败|^任务执行失败$|^任务未完成$|^任务失败$/.test(
      raw,
    )
  ) {
    return classified(t.noticeScheduleFailed, raw, hint)
  }
  if (/^the task failed$|^failed$|^未知错误$|^失败$/.test(raw)) {
    return t.agentProcessingFailed
  }
  if (
    /^the failed step was skipped$|用户选择跳过错误步骤/.test(raw)
  ) {
    return t.agentStepSkipped
  }
  if (
    /^the failed step will be retried$|用户选择重试失败步骤/.test(raw)
  ) {
    return t.agentStepRetrying
  }
  if (/^confirmation failed$/i.test(raw) || raw.startsWith('确认执行失败')) {
    return t.agentConfirmFailed
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
    code === 'task_cancelled' ||
    /^the task was cancelled$|^任务已取消$|任务已被取消|任务已被用户取消|操作已取消|用户取消了任务/i.test(
      raw,
    )
  ) {
    return t.agentTaskCancelled
  }
  if (
    code === 'wait_channel_closed' ||
    /任务等待通道已断开|^the wait channel closed$/i.test(raw)
  ) {
    return t.waitChannelClosed
  }
  if (
    code === 'wait_input_timeout' ||
    /等待用户输入已超时|任务等待用户输入超时|^waiting for input timed out|^Waiting for a reply timed out|^用户回答已过期|^The answer expired|^回答超时$|^The answer timed out$/i.test(
      raw,
    )
  ) {
    const hours = raw.match(/（(\d+)小时）|\((\d+) hours\)/)
    if (hours) {
      return fill(t.waitInputTimeoutHours, {
        hours: Number(hours[1] || hours[2] || '0'),
      })
    }
    return t.waitInputTimeout
  }
  if (
    /^API 速率限制，等待后重试$|^Rate limited; wait and retry$/i.test(raw)
  ) {
    return t.rateLimited
  }
  if (
    /^服务暂时不可用，等待后重试$|^Service temporarily unavailable; wait and retry$/i.test(
      raw,
    )
  ) {
    return t.serviceUnavailable
  }
  if (
    /^API 响应解析失败|^Failed to parse the API response; retrying may help$/i.test(
      raw,
    )
  ) {
    return t.aiResponseInvalid
  }
  if (
    /^权限不足，需要用户授权$|^Permission denied; user authorization is required$/i.test(
      raw,
    )
  ) {
    return t.forbidden
  }
  if (
    /^请求的资源不存在$|^The requested resource does not exist$/i.test(raw)
  ) {
    return t.notFound
  }
  if (
    /^内容策略违规|^Content policy violation; retry after removing sensitive content$/i.test(
      raw,
    )
  ) {
    return t.contentPolicyRetry
  }
  if (/^配置缺失|^Configuration missing \(not retryable\)/i.test(raw)) {
    return classified(t.serviceNotConfigured, raw, hint)
  }
  if (/^参数缺失:|^Missing parameter:/i.test(raw)) {
    return classified(t.missingParameter, raw, hint)
  }
  if (/^未知错误:|^Unknown error:/i.test(raw)) {
    return classified(t.unknown, raw, hint)
  }
  if (
    /^恢复执行超出步骤上限$|^Resume exceeded the step cap$/i.test(raw)
  ) {
    return t.agentResumeOverCap
  }
  if (
    code === 'task_unavailable' ||
    /任务状态已不可用|^the task is no longer available$/i.test(raw)
  ) {
    return t.taskUnavailable
  }
  if (code === 'analytics_unavailable') {
    return t.analyticsUnavailable
  }
  const queued = raw.match(
    /排队中（前方约\s*(\d+)\s*个任务）|Queued \(about (\d+) ahead\)/i,
  )
  if (queued) {
    return fill(t.agentQueued, {
      n: Number(queued[1] || queued[2] || '0'),
    })
  }
  const queueWait = raw.match(
    /系统繁忙，排队超过\s*(\d+)\s*秒|Waited more than (\d+) seconds/i,
  )
  if (queueWait) {
    return fill(t.agentQueueTimeout, {
      sec: Number(queueWait[1] || queueWait[2] || '0'),
    })
  }
  if (/^现在没在放歌|^Nothing is playing/i.test(raw)) {
    return currentCopy().music.noPlaying
  }
  if (
    /^任务已提交，等待执行$|^Task submitted, waiting to run$/i.test(raw)
  ) {
    return t.agentSubmitted
  }
  const planFailed = raw.match(
    /^I understood the request, but planning failed:\s*(\S.*?)\.\s*Please describe what you want more specifically\.?$/i,
  )
  if (planFailed) {
    const detail = (planFailed[1] || '').trim()
    return detail
      ? fill(t.agentPlanningFailed, { detail })
      : t.agentPlanningFailedBare
  }
  if (raw.startsWith('我理解了你的请求，但生成执行计划时出现问题')) {
    const detail = raw
      .replaceAll(/^我理解了你的请求，但生成执行计划时出现问题[：:.\s]*/g, '')
      .replaceAll(/请更具体地描述你想要什么。?$/g, '')
      .replaceAll(/[。．.]+$/g, '')
      .trim()
    return detail
      ? fill(t.agentPlanningFailed, { detail })
      : t.agentPlanningFailedBare
  }
  const music = currentCopy().music
  if (/^正在播放音乐$|^Playing music$/i.test(raw)) return music.playingNow
  if (/^已暂停播放$|^Paused$/i.test(raw)) return music.pausedPlayback
  if (/^切换播放状态$|^Toggled playback$/i.test(raw)) return music.toggledPlayback
  if (/^切换到下一首$|^Skipped to next track$/i.test(raw)) return music.skippedNext
  if (/^切换到上一首$|^Skipped to previous track$/i.test(raw)) {
    return music.skippedPrevious
  }
  if (/^已调节音量$|^Volume adjusted$/i.test(raw)) return music.volumeAdjusted
  if (/^已静音$|^Muted$/i.test(raw)) return music.muted
  if (/^已取消静音$|^Unmuted$/i.test(raw)) return music.unmuted
  if (/^已跳转播放位置$|^Seeked$/i.test(raw)) return music.seeked
  if (/^获取 B 站数据$|^Loading Bilibili data$/i.test(raw)) {
    return fill(t.loadingNamedData, { name: 'Bilibili' })
  }
  if (/^获取 Steam 数据$|^Loading Steam data$/i.test(raw)) {
    return fill(t.loadingNamedData, { name: 'Steam' })
  }
  if (/^获取 GitHub 数据$|^Loading GitHub data$/i.test(raw)) {
    return fill(t.loadingNamedData, { name: 'GitHub' })
  }
  if (/^获取网易云数据$|^Loading NetEase data$/i.test(raw)) {
    return fill(t.loadingNamedData, { name: 'NetEase' })
  }
  if (/^获取 Bangumi 数据$|^Loading Bangumi data$/i.test(raw)) {
    return fill(t.loadingNamedData, { name: 'Bangumi' })
  }
  if (/^获取 X 数据$|^Loading X data$/i.test(raw)) {
    return fill(t.loadingNamedData, { name: 'X' })
  }
  if (/^获取 Discord 数据$|^Loading Discord data$/i.test(raw)) {
    return fill(t.loadingNamedData, { name: 'Discord' })
  }
  if (/^获取 MyAnimeList 数据$|^Loading MyAnimeList data$/i.test(raw)) {
    return fill(t.loadingNamedData, { name: 'MyAnimeList' })
  }
  if (/^AI 总结$|^Summarizing$/i.test(raw)) return t.agentSummarizing
  if (/^AI 分析$|^Analyzing$/i.test(raw)) return t.agentAnalyzing
  if (/^网络搜索$|^Searching the web$/i.test(raw)) return t.agentWebSearch
  if (/^发现 RSS 源$|^Discovering feeds$/i.test(raw)) return t.agentDiscoverFeeds
  if (/^订阅 RSS 源$|^Subscribing to a feed$/i.test(raw)) {
    return t.agentSubscribeFeed
  }
  if (/^获取平台数据$|^Loading platform data$/i.test(raw)) {
    return fill(t.loadingNamedData, { name: 'platform' })
  }
  if (/^AI 对话$|^Chatting$/i.test(raw)) return t.agentChat
  if (/^生成图片$|^Generating an image$/i.test(raw)) return t.agentGenerateImage
  if (/^生成提示词$|^Generating a prompt$/i.test(raw)) return t.agentGeneratePrompt
  if (/^内容对比$|^Comparing content$/i.test(raw)) return t.agentCompareContent
  if (/^文字转语音$|^Reading aloud$/i.test(raw)) return t.agentReadingAloud
  if (/^全局搜索$|^Searching$/i.test(raw)) return t.agentSearching
  if (/^生成报告$|^Generating a report$/i.test(raw)) return t.agentGenerateReport
  if (/^清除缓存$|^Clearing cache$/i.test(raw)) return t.agentClearCache
  if (/^获取 Tapp 列表$|^Listing apps$/i.test(raw)) return t.agentListingApps
  if (/^打开 Tapp$|^Opening an app$/i.test(raw)) return t.agentOpeningApp
  if (/^获取文章列表$|^Loading articles$/i.test(raw)) return t.agentLoadingArticles
  if (/^获取文章内容$|^Loading article$/i.test(raw)) return t.agentLoadingArticle
  if (/^获取订阅源$|^Loading feeds$/i.test(raw)) return t.agentLoadingFeeds
  if (/^获取订阅内容$|^Loading feed content$/i.test(raw)) {
    return t.agentLoadingFeedContent
  }
  if (/^阅读统计$|^Reading stats$/i.test(raw)) return t.agentReadingStats
  if (/^执行技能$|^Running skill$/i.test(raw)) return t.agentRunningSkill
  if (/^调用外部工具$|^Calling a tool$/i.test(raw)) return t.agentCallingTool
  if (/^Tapp 应用$|^Tapp apps$/i.test(raw)) return currentCopy().tapp.apps
  if (/^组件列表$/.test(raw)) return t.tappWidgets
  if (/^存储数据$/.test(raw)) return t.tappStorage
  if (/^定时任务$|^Scheduled tasks$/i.test(raw)) return t.tappScheduledTasks
  if (/^执行记录$/.test(raw)) return t.tappExecutions
  if (/^未知应用$|^Unknown app$/i.test(raw)) return t.unknownApp
  if (/^未命名报告$|^Untitled report$/i.test(raw)) return t.unnamedReport
  if (/^未知标题$/.test(raw)) return t.unknownTitle
  if (/^未命名内容$/.test(raw)) return t.untitledContent
  if (/^未知用户$/.test(raw)) return currentCopy().userModal.unknownUser
  if (/^未分类$/.test(raw)) return currentCopy().phantasi.uncategorized
  if (/^最新文章$|^Latest articles$/i.test(raw)) {
    return currentCopy().phantasi.latestArticles
  }
  if (/^标题不能为空$|^A title is required$/i.test(raw)) {
    return currentCopy().phantasi.noteTitleRequired
  }
  const titleTooLong = raw.match(
    /^标题最多 (\d+) 字，现在有 (\d+) 字$|^Titles can be at most (\d+) characters \(this one is (\d+)\)$/i,
  )
  if (titleTooLong) {
    return fill(currentCopy().phantasi.noteTitleTooLong, {
      max: titleTooLong[1] || titleTooLong[3] || '',
      chars: titleTooLong[2] || titleTooLong[4] || '',
    })
  }
  const bodyTooLong = raw.match(
    /^正文最多 (\d+) 字，现在有 (\d+) 字$|^Notes can be at most (\d+) characters \(this one is (\d+)\)$/i,
  )
  if (bodyTooLong) {
    return fill(currentCopy().phantasi.noteBodyTooLong, {
      max: bodyTooLong[1] || bodyTooLong[3] || '',
      chars: bodyTooLong[2] || bodyTooLong[4] || '',
    })
  }
  if (/^Note draft was updated elsewhere$/i.test(raw)) {
    return currentCopy().phantasi.noteRevisionConflict
  }
  if (/^That time has already passed$/i.test(raw)) {
    return currentCopy().phantasi.noteSchedulePast
  }
  if (/^A schedule time is required$/i.test(raw)) {
    return currentCopy().phantasi.noteScheduleNeedTime
  }
  if (/^游客$/.test(raw)) return t.guestLabel
  const userNumber = raw.match(/^用户#(\d+)$/)
  if (userNumber) return fill(t.userNumber, { id: userNumber[1] })
  if (/^Xbox 玩家$/.test(raw)) return currentCopy().reportCardWidget.xboxGamerDefault
  if (/^PSN 玩家$/.test(raw)) return t.psnPlayer
  if (/^Steam 玩家$/.test(raw)) return t.steamPlayer
  if (/^等待 Tapp 完成交互$/.test(raw)) return t.waitTappInteraction
  if (/^动态技能$/.test(raw)) return t.capDynamicSkills
  if (/^MCP 工具$/.test(raw)) return t.capMcpTools
  if (
    raw.startsWith('我现在心情很低，不想接新的事情') ||
    raw.startsWith('I\'m in a very low mood and don\'t want to take on anything new')
  ) {
    return t.agentRefuseLowMood
  }
  if (raw.startsWith('我对这个请求的理解置信度较低')) return t.agentNeedClarification
  if (/^重新执行这个步骤$/.test(raw)) return t.retryStepDesc
  if (/^跳过这个步骤继续执行$/.test(raw)) return t.skipStepDesc
  if (/^取消整个任务$/.test(raw)) return t.cancelTaskDesc
  if (/^重试$/.test(raw)) return t.retryStep
  if (/^跳过$/.test(raw)) return t.skipStep
  if (/^联网搜索结果$/.test(raw)) return t.webSearchResult
  if (raw.includes('试试搜索你已有数据')) return t.searchLocalHint
  const dbMissing = raw.match(/^(\S+) 数据库文件不存在（(.+)）/)
  if (dbMissing) {
    return fill(t.databaseFileMissing, {
      name: dbMissing[1],
      path: dbMissing[2],
    })
  }
  if (/^请尝试其他关键词$/.test(raw)) return t.tryOtherKeyword
  if (/^检查拼写是否正确$/.test(raw)) return t.checkSpelling
  if (raw.includes('page.content 读取 Tapp')) return t.pageContentNeedsTapp
  if (raw.includes('page.content 读取平台')) return t.pageContentNeedsPlatform
  if (/^AI 联网搜索发现$/.test(raw)) return t.webSearchResult
  if (raw.includes('AI 已根据近期失败原因改写')) return t.noticeSkillImprovedBody
  const prunedSkill = raw.match(/^自动技能「(.+)」因失败率过高被淘汰/)
  if (prunedSkill) {
    return fill(t.noticeSkillPrunedBody, { name: prunedSkill[1] })
  }
  if (raw.startsWith('我的理解是：')) return t.agentNeedClarification
  if (/^网易云音乐用户$/.test(raw)) return t.neteaseMusicUser
  if (/^Bangumi 用户$/.test(raw)) return t.bangumiUser
  if (/^MyAnimeList 用户$/.test(raw)) return t.malUser
  if (/^智能阅读列表$/.test(raw)) return currentCopy().phantasi.smartReadingList
  if (/^订阅源$/.test(raw)) return currentCopy().phantasi.boardFeeds
  if (/^已读$/.test(raw)) return t.phantasiMarkRead
  if (/^未读$/.test(raw)) return t.phantasiMarkUnread
  if (/^已收藏$/.test(raw)) return t.phantasiMarkStarred
  if (/^取消收藏$/.test(raw)) return t.phantasiMarkUnstarred
  if (/^稍后阅读$/.test(raw)) return t.phantasiMarkLater
  if (/^请提供更多信息$|^Please provide more information\.?$/i.test(raw)) {
    return t.agentNeedMoreInfo
  }
  const webSearchNamed = raw.match(/^网络搜索\s*[—\-]\s*(\S.*)$/)
  if (webSearchNamed) {
    return fill(t.webSearchNamed, { name: webSearchNamed[1] })
  }
  const readingListNamed = raw.match(/^阅读列表\s*[—\-]\s*(\S.*)$/)
  if (readingListNamed) {
    return fill(t.readingListNamed, { name: readingListNamed[1] })
  }
  const mcpTool = raw.match(
    /^将调用外部 MCP 服务 '(.+)' 的工具 '(.+)'$|^This will call tool '(.+)' on MCP server '(.+)'$/i,
  )
  if (mcpTool) {
    const server = mcpTool[1] || mcpTool[4] || ''
    const tool = mcpTool[2] || mcpTool[3] || ''
    return fill(t.confirmMcpTool, { server, tool })
  }
  const mcpToolsLoaded = raw.match(/^已加载 (\d+) 个工具$|^Loaded (\d+) tools$/i)
  if (mcpToolsLoaded) {
    return fill(t.noticeMcpToolsLoaded, {
      n: Number(mcpToolsLoaded[1] || mcpToolsLoaded[2] || '0'),
    })
  }
  if (
    /^维护重试成功$|^Maintenance retry succeeded$/i.test(raw)
  ) {
    return t.noticeMcpMaintenanceRetry
  }
  if (/^自动重启成功$|^Auto-restart succeeded$/i.test(raw)) {
    return t.noticeMcpAutoRestart
  }
  if (
    raw.includes('状态监控超时') ||
    /Status watch timed out/i.test(raw)
  ) {
    return t.noticeUpdaterWatchTimeout
  }
  if (/^未知艺术家$/.test(raw)) return currentCopy().library.unknownArtist
  const autoRefreshNamed = raw.match(
    /^自动刷新 (.+) 数据$|^Auto-refresh (.+) data$/i,
  )
  if (autoRefreshNamed) {
    return fill(t.autoRefreshNamed, {
      name: autoRefreshNamed[1] || autoRefreshNamed[2] || '',
    })
  }
  const leftoverHeartbeatTask = raw.match(
    /^定时任务:\s*(\S.*)$|^Scheduled task:\s*(\S.*)$/i,
  )
  if (leftoverHeartbeatTask) {
    return fill(t.noticeHeartbeatTask, {
      name: leftoverHeartbeatTask[1] || leftoverHeartbeatTask[2] || '',
    })
  }
  if (
    /^即将添加新的 RSS|^This will add a new RSS/i.test(raw)
  ) {
    return t.confirmAddFeed
  }
  if (
    /^即将控制 Phantasi|^This will start, stop, or refresh the Phantasi scheduler/i.test(
      raw,
    )
  ) {
    return t.confirmPhantasiSchedule
  }
  if (
    /^即将向外部 URL|^This will send an HTTP request to an external URL/i.test(
      raw,
    )
  ) {
    return t.confirmHttpFetch
  }
  if (
    /^即将创建 Tapp 定时任务|^This will create a scheduled Tapp task/i.test(
      raw,
    )
  ) {
    return t.confirmCreateTappTask
  }
  if (
    /^即将立即触发 Tapp 定时任务|^This will run a scheduled Tapp task now/i.test(
      raw,
    )
  ) {
    return t.confirmTriggerTappTask
  }
  if (
    /^即将提交后台平台数据处理任务|^This will submit a background platform data job/i.test(
      raw,
    )
  ) {
    return t.confirmPlatformJob
  }
  if (
    /^AI 将分析 Tapp UI|^AI will analyze the Tapp UI/i.test(raw)
  ) {
    return t.confirmAnalyzeTappUi
  }
  if (
    /^即将向 Tapp 发起声明式交互|^This will send a declared interaction to the Tapp/i.test(
      raw,
    )
  ) {
    return t.confirmTappInteract
  }
  if (
    /^即将与页面元素交互|^This will interact with a page element/i.test(raw)
  ) {
    return t.confirmPageInteract
  }
  if (
    /^AI 将分析页面|^AI will analyze the page and may run actions/i.test(raw)
  ) {
    return t.confirmAnalyzePage
  }
  if (
    /^即将写入内容到目标|^This will write content to the target/i.test(raw)
  ) {
    return t.confirmWriteTarget
  }
  const playlistLoad = raw.match(
    /^正在加载(网易云|QQ音乐)歌单|^Loading (NetEase|QQ Music) playlist/i,
  )
  if (playlistLoad) {
    const name =
      playlistLoad[1] === '网易云' || playlistLoad[2] === 'NetEase'
        ? 'NetEase'
        : 'QQ Music'
    return fill(t.loadingNamedPlaylist, { name })
  }
  if (/^好了，都处理完|^All done\.?$/i.test(raw)) return t.agentAllDone
  if (/^你好！有什么我可以帮你的吗？$|^Hi! How can I help\?$/i.test(raw)) {
    return t.agentGreeting
  }
  if (/^正在理解你的请求|^Understanding your request/i.test(raw)) {
    return t.agentUnderstanding
  }
  if (/^正在规划执行步骤|^Planning steps/i.test(raw)) return t.agentPlanning
  if (
    /^需要你补充一些信息|^我需要更多信息来理解你的请求$|^I need more information to understand that\.?$/i.test(
      raw,
    )
  ) {
    return t.agentNeedClarification
  }
  const willRun = raw.match(/^This will run (.+)$|^此操作将执行\s*(\S.*)$/)
  if (willRun) {
    return fill(t.willExecute, { name: willRun[1] || willRun[2] || '' })
  }
  if (raw.startsWith('此操作将')) return t.stepNeedsConfirm
  if (/^数据读取$|^Data$/.test(raw)) return t.capCategoryData
  if (/^数据写入$|^Write$/.test(raw)) return t.capCategoryWrite
  if (/^AI处理$|^AI$/.test(raw)) return t.capCategoryAi
  if (/^资源创建$|^Create$/.test(raw)) return t.capCategoryCreate
  if (/^系统操作$|^System$/.test(raw)) return t.capCategorySystem
  if (/^外部集成$|^External$/.test(raw)) return t.capCategoryExternal
  if (/^界面控制$|^Interface$/.test(raw)) return t.capCategoryInterface
  if (/^需要更多信息$|^More information is needed\.?$/i.test(raw)) {
    return t.agentNeedMoreInfo
  }
  if (/^任务已进入更新队列$|^Update queued$/i.test(raw)) {
    return t.noticeUpdaterSubmitted
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
  if (/^comment not found$/i.test(raw)) {
    return t.notFound
  }
  if (/^failed to load comment replies/i.test(raw)) {
    return classified(t.commentRepliesLoadFailed, raw, hint)
  }
  if (/^failed to (load comments|find comment)/i.test(raw)) {
    return classified(t.commentLoadFailed, raw, hint)
  }
  if (/^failed to (save|update) comment/i.test(raw)) {
    return classified(t.commentSaveFailed, raw, hint)
  }
  if (/^failed to delete comment/i.test(raw)) {
    return classified(t.commentDeleteFailed, raw, hint)
  }
  if (/^failed to load (source|articles)(?:\s|:|$)/i.test(raw)) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.phantasiLoadFailed, action), raw, hint)
  }
  if (/^failed to (find|load) article(?:\s|:|$)/i.test(raw)) {
    return classified(t.articleLoadFailed, raw, hint)
  }
  if (
    /^failed to (create icons directory|create icon file|write icon file|read icon bytes)/i.test(
      raw,
    )
  ) {
    return classified(t.iconSaveFailed, raw, hint)
  }
  if (
    /^failed to (check existing phantasi source|find phantasi source)/i.test(raw)
  ) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.phantasiLoadFailed, action), raw, hint)
  }
  if (/^failed to (find|update|create) reading state/i.test(raw)) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.readingStateFailed, action), raw, hint)
  }
  if (/^failed to save content/i.test(raw)) {
    return classified(t.contentSaveFailed, raw, hint)
  }
  if (/^failed to delete tapp storage/i.test(raw)) {
    return classified(t.tappStorageFailed, raw, hint)
  }
  if (/^task not found$|任务不存在或无权访问/.test(raw)) {
    return t.notFound
  }
  if (
    /^only admins can /i.test(raw) ||
    /^cannot delete the default global instance$/i.test(raw)
  ) {
    return joinParts(t.forbidden, clip(raw), usefulExtra(hint, t.forbidden))
  }
  if (/^instance not found$/i.test(raw)) {
    return t.notFound
  }
  if (
    /^failed to (fetch|check) rsshub instances/i.test(raw) ||
    /^failed to (fetch global instances|fetch user instances|check existing instances)/i.test(
      raw,
    )
  ) {
    return classified(t.rsshubLoadFailed, raw, hint)
  }
  if (
    /^failed to (create|update|delete|reset|find) rsshub instance/i.test(raw) ||
    /^failed to (insert|find|update|delete|reset) instance/i.test(raw)
  ) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.rsshubSaveFailed, action), raw, hint)
  }
  if (
    /could not set up rsshub|could not read rsshub|no rsshub instances|unsafe rsshub url|初始化 RSSHub|读取 RSSHub|RSSHub 实例表为空/i.test(
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
    /^this service is not configured$|API Key 未配置/i.test(raw)
  ) {
    return /TTS|语音|Speech/.test(raw) ? t.speechNotConfigured : t.serviceNotConfigured
  }
  if (raw.includes('图片生成完成，但无法提取')) {
    return currentCopy().agentPersona.onboarding.imageProviderInvalidResponse
  }
  if (
    /^invalid tappid$/i.test(raw) ||
    raw.includes('无效的 tappId')
  ) {
    return currentCopy().tapp.invalidId
  }
  if (code === 'media_action_invalid') {
    return classified(t.mediaActionInvalid, raw, hint)
  }
  if (code === 'media_mode_invalid') {
    return classified(t.mediaModeInvalid, raw, hint)
  }
  if (
    /^invalid url$/i.test(raw) ||
    raw.startsWith('无效的 URL')
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
  if (/failed to initialize federation identity/i.test(raw)) {
    return classified(t.federationInitFailed, raw, hint)
  }
  if (/^failed to rotate federation keys/i.test(raw)) {
    return classified(t.federationKeyRotateFailed, raw, hint)
  }
  if (
    /^failed to (list following|list followers|list follows|load timeline|load delivery stats|list delivery)/i.test(
      raw,
    )
  ) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.federationDataFailed, action), raw, hint)
  }
  if (/failed to read user id/i.test(raw)) {
    return classified(t.database, raw, hint)
  }
  if (/activity not ready/i.test(raw)) {
    return classified(t.inboxNotReady, raw, hint)
  }
  if (
    /claim inbound receipt|finish inbound receipt/i.test(raw)
  ) {
    return classified(t.database, raw, hint)
  }
  if (
    /inbox processing failed|activity was permanently rejected/i.test(raw) ||
    /queue (claim|dead-letter|delivery|remote failure)/i.test(raw) ||
    /^permanent http/i.test(raw)
  ) {
    return classified(t.inboxFailed, raw, hint)
  }
  if (/^HTTP\s+\d{3}:\s*\{/i.test(raw)) {
    return classified(
      httpStatusMessage(status || statusFromErrorText(raw) || 502),
      raw,
      hint,
    )
  }
  if (
    /object ownership check failed|rejected by trust policy|object attributedto |not same-origin with signing actor/i.test(
      raw,
    )
  ) {
    return t.forbidden
  }
  if (
    /invalid (request header encoding|`.+` header encoding)|ambiguous request: (a signed header|header)/i.test(
      raw,
    )
  ) {
    return t.requestRejected
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
    return currentCopy().phantasi.errorDiscoverFailed
  }
  if (
    /notion is not configured|notion api key 未配置/i.test(raw)
  ) {
    return currentCopy().phantasi.errorNotionFetch
  }
  if (/^failed to submit refresh$|^提交失败/.test(raw)) {
    return t.taskSubmitFailed
  }
  const agentPanel = currentCopy().agentPanel
  if (code === 'preset_title_too_long' || /标题过长|title is too long/i.test(raw)) {
    return agentPanel.presetTitleTooLong
  }
  if (code === 'preset_summary_too_long' || /摘要过长|summary is too long/i.test(raw)) {
    return agentPanel.presetSummaryTooLong
  }
  if (code === 'preset_steps_too_large' || /解析步骤数据过大|parsed steps are too large/i.test(raw)) {
    return agentPanel.presetStepsTooLarge
  }
  if (
    code === 'preset_history_too_long' ||
    /对话历史过长|conversation history is too long/i.test(raw)
  ) {
    return agentPanel.presetHistoryTooLong
  }
  if (
    code === 'notification_unavailable'
  ) {
    return t.notificationUnavailable
  }
  const setup = currentCopy().setup
  if (
    code === 'db_migration_failed'
  ) {
    return setup.dbMigrationFailed
  }
  if (code === 'schema_ensure_failed') {
    return setup.schemaEnsureFailed
  }
  if (
    code === 'setup_cleanup_failed'
  ) {
    return setup.cleanupFailed
  }
  if (
    code === 'setup_claim_failed'
  ) {
    return setup.claimFailed
  }
  if (code === 'config_mode_required') {
    return setup.configModeRequired
  }
  if (
    code === 'setup_window_closed'
  ) {
    return setup.claimedRepairDesc
  }
  if (
    code === 'setup_secret_mismatch'
  ) {
    return setup.secretMismatch
  }
  if (
    code === 'schedule_invalid' ||
    /^invalid schedule (config|type)/i.test(raw) ||
    /^invalid (execution target|missed policy|scope)/i.test(raw)
  ) {
    return classified(t.scheduleInvalid, raw, hint)
  }
  if (/^task ['"]?[^'"]+['"]? not found$/i.test(raw)) {
    return t.notFound
  }
  if (/^no library data available$/i.test(raw)) {
    return currentCopy().library.loadFailed
  }
  if (/unable to load analytics/i.test(raw)) {
    return joinParts(
      currentCopy().config.analytics.loadFailed,
      status ? `HTTP ${status}` : '',
      usefulExtra(hint, currentCopy().config.analytics.loadFailed),
    )
  }
  if (/unable to load ai usage/i.test(raw)) {
    return joinParts(
      currentCopy().config.analytics.aiUsageLoadFailed,
      status ? `HTTP ${status}` : '',
      usefulExtra(hint, currentCopy().config.analytics.aiUsageLoadFailed),
    )
  }
  if (/unable to load platform data preview/i.test(raw)) {
    return joinParts(
      currentCopy().dataManagement.previewLoadFailed,
      status ? `HTTP ${status}` : '',
      usefulExtra(hint, currentCopy().dataManagement.previewLoadFailed),
    )
  }
  if (/unable to load platform previews/i.test(raw)) {
    return joinParts(
      currentCopy().dataManagement.previewLoadFailed,
      status ? `HTTP ${status}` : '',
      usefulExtra(hint, currentCopy().dataManagement.previewLoadFailed),
    )
  }
  if (/unable to load platform data status/i.test(raw)) {
    return joinParts(
      currentCopy().dataManagement.statusUnavailable,
      status ? `HTTP ${status}` : '',
      usefulExtra(hint, currentCopy().dataManagement.statusUnavailable),
    )
  }
  if (/unable to load visitor stats|visitor card unavailable/i.test(raw)) {
    return joinParts(
      currentCopy().visitorStats.loadFailed,
      status ? `HTTP ${status}` : '',
      usefulExtra(hint, currentCopy().visitorStats.loadFailed),
    )
  }
  if (/missing (room_id|channel_id)/i.test(raw)) {
    return t.inviteInvalid
  }
  if (/runtime event stream/i.test(raw)) {
    return joinParts(
      t.streamUnreadable,
      status ? `HTTP ${status}` : '',
      usefulExtra(hint, t.streamUnreadable),
    )
  }
  if (/failed to save to cloud/i.test(raw)) {
    return joinParts(
      currentCopy().tapp.schemeSaveFailed,
      status ? `HTTP ${status}` : '',
      usefulExtra(hint, currentCopy().tapp.schemeSaveFailed),
    )
  }
  if (/hitokoto response missing text field/i.test(raw)) {
    return currentCopy().config.hitokotoLoadFailed
  }
  if (/identity not found/i.test(raw)) {
    return t.notFound
  }
  if (
    /^media upload failed/i.test(raw) ||
    /failed to upload (federation )?media/i.test(raw)
  ) {
    return classified(t.federationMediaUploadFailed, raw, hint)
  }
  if (
    code === 'MEDIA_IN_USE' ||
    /still in use and cannot be deleted/i.test(raw)
  ) {
    return t.mediaInUse
  }
  const merope = currentCopy().merope
  if (
    code === 'see_through_token_required' ||
    /hugging face api token is not configured/i.test(raw)
  ) {
    return merope.motionSeeThroughTokenRequired
  }
  if (
    code === 'see_through_busy' ||
    /see-through decomposition is already running/i.test(raw)
  ) {
    return merope.motionSeeThroughBusy
  }
  if (
    code === 'see_through_auth_failed' ||
    /hugging face rejected the (configured )?api token/i.test(raw)
  ) {
    return merope.motionSeeThroughAuthFailed
  }
  if (
    code === 'see_through_quota_unavailable' ||
    /zerogpu (quota is exhausted|is unavailable)/i.test(raw)
  ) {
    return merope.motionSeeThroughQuota
  }
  if (
    code === 'see_through_timeout' ||
    /see-through inference timed out/i.test(raw)
  ) {
    return merope.motionSeeThroughTimeout
  }
  if (
    code === 'see_through_upstream_failed' ||
    code === 'see_through_invalid_input' ||
    /see-through (returned|event stream)/i.test(raw) ||
    /hugging face token must be a valid/i.test(raw)
  ) {
    return classified(merope.motionSeeThroughUpstream, raw, hint)
  }
  if (
    /stored rig is invalid|active rig is missing|active rig atlas is missing|invalid rig asset id/i.test(
      raw,
    )
  ) {
    return classified(merope.rigStoredInvalid, raw, hint)
  }
  if (
    /rig compilation failed|rig character asset|rig manifest migration|merope_rig_failed/i.test(
      raw,
    )
  ) {
    return classified(merope.rigCompileFailed, raw, hint)
  }
  if (/rig atlas|invalid rig atlas/i.test(raw)) {
    return classified(merope.rigAtlasFailed, raw, hint)
  }
  if (
    /invalid rig (import|source|analysis)|rig import|rig source exceeds|rig analysis reference|rig preview is missing/i.test(
      raw,
    )
  ) {
    return classified(merope.rigImportFailed, raw, hint)
  }
  if (
    /master portrait is not available|the current master portrait/i.test(raw)
  ) {
    return merope.portraitUnavailable
  }
  if (
    /invalid portrait|portrait image exceeds|portrait upload|portrait is missing image|could not upload portrait/i.test(
      raw,
    )
  ) {
    return classified(merope.portraitUploadFailed, raw, hint)
  }
  if (/could not generate site portrait/i.test(raw)) {
    return joinParts(
      merope.visualFailed,
      status ? `HTTP ${status}` : '',
      usefulExtra(hint, merope.visualFailed),
    )
  }
  if (/could not load site face/i.test(raw)) {
    return joinParts(
      merope.loadFailed,
      status ? `HTTP ${status}` : '',
      usefulExtra(hint, merope.loadFailed),
    )
  }
  if (/could not load see-through status/i.test(raw)) {
    return joinParts(
      merope.seeThroughStatusFailed,
      status ? `HTTP ${status}` : '',
      usefulExtra(hint, merope.seeThroughStatusFailed),
    )
  }
  if (/could not save hugging face token/i.test(raw)) {
    return classified(merope.motionSeeThroughTokenFailed, raw, hint)
  }
  if (/see-through decomposition failed/i.test(raw)) {
    return classified(merope.motionSeeThroughUpstream, raw, hint)
  }
  if (/could not commit persona rig/i.test(raw)) {
    return classified(merope.rigCommitFailed, raw, hint)
  }
  if (/could not (preview|import|diagnose) persona rig/i.test(raw)) {
    return classified(merope.rigImportFailed, raw, hint)
  }
  if (/persona rig .+ manifest is invalid/i.test(raw)) {
    return classified(merope.rigCompileFailed, raw, hint)
  }
  if (/webgl2 is required/i.test(raw)) {
    return merope.anime25dWebglFailed
  }
  if (/anime2\.5drig playback missing/i.test(raw)) {
    const role = raw.match(/missing (\S+)/i)?.[1] || ''
    return fill(merope.anime25dMissingLayer, { role: role || '?' })
  }
  if (
    /anime2\.5drig (mesh buffers|layer crop|layer texture|program|shader|link)/i.test(
      raw,
    ) ||
    /^missing uniform /i.test(raw)
  ) {
    return merope.anime25dPlaybackFailed
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
    /game_message_invalid|invalid game message|game message_type|game session messages|not a game session|game payload too large/i.test(
      raw,
    )
  ) {
    return t.gameMessageInvalid
  }
  if (/^failed to fetch feed/i.test(raw)) {
    return classified(t.phantasiRefreshFailed, raw, hint)
  }
  if (
    /^failed to (fetch|count) phantasi /i.test(raw) ||
    /^failed to count (starred|read) items/i.test(raw) ||
    /^failed to (list sources|find source|list categories|find category|list articles|export sources)/i.test(
      raw,
    )
  ) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.phantasiLoadFailed, action), raw, hint)
  }
  if (
    /^failed to (save|update) source/i.test(raw) ||
    /^failed to import sources/i.test(raw) ||
    /^failed to (check existing items|update source counts|query sources|batch insert items)/i.test(
      raw,
    )
  ) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(
      joinParts(t.phantasiSourceSaveFailed, action),
      raw,
      hint,
    )
  }
  if (/^failed to delete source/i.test(raw)) {
    return classified(t.phantasiSourceDeleteFailed, raw, hint)
  }
  if (/^failed to (save|update) category/i.test(raw)) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.phantasiCategorySaveFailed, action), raw, hint)
  }
  if (/^failed to delete category/i.test(raw)) {
    return classified(t.phantasiCategoryDeleteFailed, raw, hint)
  }

  const byStatus = status > 0 ? httpStatusMessage(status) : ''
  const useful = isUselessErrorText(raw) ? '' : clip(raw)
  const extraHint = usefulExtra(hint, byStatus, useful, fallbackText)

  if (code === 'unmapped') {
    if (status >= 400) return joinParts(byStatus, extraHint)
    return joinParts(t.operationFailed, extraHint)
  }
  if (useful && /^failed to\b/i.test(useful)) {
    if (status >= 400) return joinParts(byStatus, extraHint)
    return joinParts(t.operationFailed, extraHint)
  }
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
