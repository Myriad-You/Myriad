import { currentCopy, formatCurrent } from '../i18n/localeCopy'
import { ApiError } from '../services/api'
import { exactErrorCode, resolveErrorCode } from './errorCodes'
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

/**
 * The backend's text after `label:` is English. Next to translated copy keep
 * only short detail (a MIME type, a size, `HTTP 502`); drop English sentences.
 */
export function withoutForeignProse(copy: string, raw: string): string {
  if ([...copy].every((ch) => ch.charCodeAt(0) < 128)) return raw
  const colon = raw.indexOf(':')
  if (colon < 0) return raw
  const words = raw.slice(colon + 1).trim().split(/\s+/).filter(Boolean)
  return words.length > 2 ? raw.slice(0, colon) : raw
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
  // Code from the transport, or from matching a known fixed text exactly.
  const rawCode = exactErrorCode(raw)
  const is = (candidate: string) => code === candidate || rawCode === candidate

  // The code → copy table: a code with an entry needs no branch below.
  const byCode: Readonly<Record<string, string | undefined>> = t.byCode
  const tableCopy = (code && byCode[code]) || (rawCode && byCode[rawCode])
  if (tableCopy) return classified(tableCopy, withoutForeignProse(tableCopy, raw), hint)

  if (code === 'unauthorized') {
    return joinParts(t.unauthorized, usefulExtra(hint, t.unauthorized))
  }
  if (code === 'forbidden' || code === 'no_admin') {
    return joinParts(t.forbidden, usefulExtra(hint, t.forbidden))
  }
  if (
    is('federation_disabled_region') ||
    is('text_federation_disabled_in_this_region')
  ) {
    return t.federationDisabledRegion
  }
  if (code === 'not_found') {
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
  if (is('tapp_save_failed')) {
    return classified(t.tappSaveFailed, raw, hint)
  }
  if (is('account_update_failed')) {
    return classified(currentCopy().config.usersUpdateFailed, raw, hint)
  }
  if (is('users_load_failed')) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(
      joinParts(currentCopy().config.usersLoadError, action),
      raw,
      hint,
    )
  }
  if (is('account_delete_failed')) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(
      joinParts(currentCopy().config.usersDeleteFailed, action),
      raw,
      hint,
    )
  }
  if (is('identity_unlink_failed')) {
    return classified(currentCopy().config.usersUnlinkFailed, raw, hint)
  }
  if (code === 'bad_request') {
    const label = httpStatusMessage(400)
    return joinParts(label, usefulExtra(hint, label))
  }
  if (code === 'conflict') {
    const label = httpStatusMessage(409)
    return joinParts(label, usefulExtra(hint, label))
  }
  if (code === 'internal_error') {
    const label = fill(t.serverError, { status: 500 })
    return joinParts(label, usefulExtra(hint, label))
  }
  if (code === 'service_unavailable') {
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
  if (code === 'TIMEOUT' || status === 408) {
    return joinParts(t.timeout, usefulExtra(hint, t.timeout))
  }
  if (code === 'NETWORK_ERROR' || (status === 0 && reason instanceof ApiError)) {
    return joinParts(t.networkError, usefulExtra(hint, t.networkError))
  }
  if (code === 'CSRF' || is('csrf_failed')) {
    return joinParts(t.csrfUnavailable, usefulExtra(hint, t.csrfUnavailable))
  }
  if (code === 'database_error' || is('text_database_is_not_connected')) {
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
  if (is('agent_session_query_failed')) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.agentSessionLoadFailed, action), raw, hint)
  }
  if (is('agent_session_save_failed')) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.agentSessionSaveFailed, action), raw, hint)
  }
  if (is('agent_session_archive_failed')) {
    return classified(t.agentSessionArchiveFailed, raw, hint)
  }
  if (is('persona_load_failed')) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.personaLoadFailed, action), raw, hint)
  }
  if (is('persona_save_failed')) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.personaSaveFailed, action), raw, hint)
  }
  if (is('persona_delete_failed')) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.personaDeleteFailed, action), raw, hint)
  }
  if (is('addressee_load_failed')) {
    return classified(t.addresseeLoadFailed, raw, hint)
  }
  if (is('addressee_save_failed')) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.addresseeSaveFailed, action), raw, hint)
  }
  if (is('preset_fetch_failed')) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.presetLoadFailed, action), raw, hint)
  }
  if (is('preset_update_failed')) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.presetSaveFailed, action), raw, hint)
  }
  if (is('preset_delete_failed')) {
    return classified(t.presetDeleteFailed, raw, hint)
  }
  if (is('account_load_failed')) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.accountLoadFailed, action), raw, hint)
  }
  if (is('account_save_failed')) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.accountSaveFailed, action), raw, hint)
  }
  if (is('password_change_failed')) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.passwordChangeFailed, action), raw, hint)
  }
  if (is('local_login_save_failed')) {
    return classified(t.localLoginSaveFailed, raw, hint)
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
  if (is('ai_response_invalid')) {
    return joinParts(t.aiResponseInvalid, usefulExtra(hint, t.aiResponseInvalid))
  }
  // uncoded: agent step errors are plain strings with no code field
  // (ai_process.rs ai_step_failed, ui_control.rs / execute_step.rs classify_outbound_fetch).
  if (
    /^(annotation generation|podcast script generation|smart filter|content comparison|prompt generation|translation|code explanation|ai abstraction|skill ai planning|ui analysis) failed/i.test(
      raw,
    )
  ) {
    const action =
      raw.match(
        /^(annotation generation|podcast script generation|smart filter|content comparison|prompt generation|translation|code explanation|ai abstraction|skill ai planning|ui analysis)/i,
      )?.[1] || ''
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
    is('ai_generation_failed') ||
    // uncoded: Gemini client strings carrying the provider's status/body
    // (gemini_media.rs GeminiMediaError, analyzer/client.rs).
    /^gemini api /i.test(raw) ||
    /invalid gemini json/i.test(raw)
  ) {
    return joinParts(t.aiGenerationFailed, usefulExtra(hint, t.aiGenerationFailed))
  }
  if (code === 'settings_backup_failed') {
    return joinParts(
      t.settingsBackupRestoreFailed,
      usefulExtra(hint, t.settingsBackupRestoreFailed),
    )
  }
  if (
    // uncoded: package validation strings reach the client through the
    // ApiResponse envelope (myriad-tapp-rules prepared.rs, tapp_store
    // prepared_package.rs), plus TappRuntime.ts's own manifest check.
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
  if (code === 'dnd_schedule_invalid') {
    return t.dndScheduleInvalid
  }
  if (code === 'dnd_schedule_incomplete') {
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
    code === 'admin_required'
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
  if (is('hitokoto_fetch_failed')) {
    return currentCopy().config.hitokotoLoadFailed
  }
  // uncoded: agent step errors (handlers/external.rs reqwest_fetch_error /
  // display_fetch_error) and fetcher errors (fetcher/platforms_core.rs) that
  // embed the provider's reply.
  if (
    /failed to fetch (hitokoto|bilibili|bangumi|steam|weather|netease|game details)/i.test(
      raw,
    ) ||
    /^(bilibili|bangumi|weather|netease|steam) api error/i.test(raw)
  ) {
    const name = /hitokoto/i.test(raw)
      ? 'Hitokoto'
      : /bilibili/i.test(raw)
        ? 'Bilibili'
        : /bangumi/i.test(raw)
          ? 'Bangumi'
          : /steam|game details/i.test(raw)
            ? 'Steam'
            : /weather/i.test(raw)
              ? 'Weather'
              : /netease/i.test(raw)
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
  // uncoded: platform cache strings name the platform (platform_cache.rs
  // platform_cache_read_failed, data_read/platform.rs, ai_process.rs /
  // catalog.rs "No data available for platform").
  if (
    /^no cached \w+ data/i.test(raw) ||
    /^no data available for platform:/i.test(raw) ||
    is('text_platform_data_not_found')
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
  if (/^failed to (read|parse) \w+ data/i.test(raw)) {
    const found = raw.match(/^failed to (?:read|parse) (\w+) data/i)?.[1] || 'platform'
    const name = `${found.charAt(0).toUpperCase()}${found.slice(1)}`
    return classified(
      fill(t.platformNamedFetchFailed, { name }),
      raw,
      hint,
    )
  }
  // uncoded: outbound HTTP strings from agent steps (handlers/external.rs),
  // the Tapp declared-API proxy (tapp_api_service.rs) and myriad-outbound.
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
  if (code === 'tapp_already_installed') {
    return currentCopy().tapp.alreadyInstalled
  }
  if (code === 'tapp_not_installed') {
    return currentCopy().tapp.appNotExist
  }
  if (
    is('REPORT_CREATE_FAILED') ||
    is('REPORT_UPDATE_FAILED') ||
    is('REPORT_DELETE_FAILED')
  ) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.reportSaveFailed, action), raw, hint)
  }
  if (code === 'tapp_not_found') {
    return classified(t.tappFindFailed, raw, hint)
  }
  if (code === 'TAPP_CREDENTIAL_LOAD_FAILED') {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.tappResourceLoadFailed, action), raw, hint)
  }
  // uncoded: image cache strings travel inside media / AI task errors
  // (image_cache.rs cache_io_error, data_paths.rs storage_error).
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
  if (is('mcp_timeout')) {
    const method = raw.match(/method ['"]([^'"]+)['"]/i)?.[1] || ''
    return joinParts(t.mcpTimeout, method, usefulExtra(hint, t.mcpTimeout, method))
  }
  if (is('mcp_response_invalid')) {
    return classified(t.mcpResponseInvalid, raw, hint)
  }
  if (is('mcp_tool_failed')) {
    return classified(t.mcpToolFailed, raw, hint)
  }
  if (is('mcp_server_not_ready')) {
    const id = raw.match(/mcp server ['"]([^'"]+)['"]/i)?.[1] || ''
    const state = raw.match(/not ready \(([^)]+)\)/i)?.[1] || ''
    return joinParts(t.mcpTalkFailed, id, state, usefulExtra(hint, t.mcpTalkFailed, id, state))
  }
  if (is('mcp_io_failed')) {
    return classified(t.mcpTalkFailed, raw, hint)
  }
  if (is('upstream_request_failed')) {
    return classified(
      fill(t.serverError, { status: status || 502 }),
      raw,
      hint,
    )
  }
  if (is('report_save_failed')) {
    return classified(t.reportSaveFailed, raw, hint)
  }
  if (is('reminder_save_failed')) {
    return classified(t.reminderSaveFailed, raw, hint)
  }
  if (is('note_save_failed')) {
    return classified(t.noteSaveFailed, raw, hint)
  }
  if (is('bookmark_save_failed')) {
    return classified(t.bookmarkSaveFailed, raw, hint)
  }
  if (is('profile_text_source_save_failed')) {
    return classified(t.profileTextSaveFailed, raw, hint)
  }
  if (is('profile_text_source_load_failed')) {
    return classified(t.profileTextLoadFailed, raw, hint)
  }
  if (is('avatar_source_save_failed')) {
    return classified(t.avatarSourceSaveFailed, raw, hint)
  }
  if (is('avatar_source_load_failed')) {
    return classified(t.avatarSourceLoadFailed, raw, hint)
  }
  if (is('storage_read_failed') || is('storage_save_failed')) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(joinParts(t.tappStorageFailed, action), raw, hint)
  }
  if (code === 'cache_clear_failed' || is('AI_TASK_REGISTRY_UNAVAILABLE')) {
    return classified(t.database, raw, hint)
  }
  if (code === 'update_failed' || is('updater_upstream_failed')) {
    return classified(t.noticeUpdaterFailed, raw, hint)
  }
  if (is('TRIPO_ERROR')) {
    return classified(t.model3dFailed, raw, hint)
  }
  if (is('tapp_credential_binding_invalid')) {
    return classified(t.unauthorized, raw, hint)
  }
  if (is('AI_OUTPUT_SCHEMA_MISMATCH')) {
    return classified(t.schemaMismatch, raw, hint)
  }
  if (is('scheduler_failed')) {
    const action = raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
    return classified(
      joinParts(t.noticeScheduleFailed, action),
      raw,
      hint,
    )
  }
  if (is('text_player_not_found')) {
    return t.notFound
  }
  if (code === 'youtube_upstream_failed') {
    return joinParts(
      t.youtubeUpstreamFailed,
      status ? `HTTP ${status}` : '',
      usefulExtra(hint, t.youtubeUpstreamFailed),
    )
  }
  if (is('e2e_key_failed')) {
    return classified(t.e2eKeyFailed, raw, hint)
  }
  if (code === 'config_invalid') {
    return joinParts(
      t.configInvalid,
      usefulExtra(raw, t.configInvalid),
      usefulExtra(hint, t.configInvalid),
    )
  }
  if (code === 'config_save_failed') {
    return classified(t.configSaveFailed, raw, hint)
  }
  if (is('public_config_invalid')) {
    return joinParts(
      t.configFileReadFailed,
      status ? `HTTP ${status}` : '',
      usefulExtra(hint, t.configFileReadFailed),
    )
  }
  if (is('store_asset_fetch_failed')) {
    const name = raw.match(/asset ([^\s:]+)/i)?.[1] || ''
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
  const phantasi = currentCopy().phantasi
  if (is('notion_fetch_failed')) {
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
  if (is('feed_parse_failed')) {
    return phantasi.errorFeedNeedName
  }
  if (is('feed_unparseable')) {
    return classified(t.phantasiParseFailed, raw, hint)
  }
  if (code === 'feed_discover_failed') {
    return classified(phantasi.errorDiscoverFailed, raw, hint)
  }
  if (is('feed_url_invalid')) {
    return classified(t.phantasiInvalidUrl, raw, hint)
  }
  if (is('config_load_failed')) {
    return classified(currentCopy().config.loadConfigFailed, raw, hint)
  }
  if (is('permissions_save_failed')) {
    return classified(currentCopy().config.permissionsSaveFailed, raw, hint)
  }
  if (is('tapp_save_failed')) {
    return classified(t.tappSaveFailed, raw, hint)
  }
  if (is('mcp_config_save_failed')) {
    return classified(currentCopy().config.mcpSaveFailed, raw, hint)
  }
  if (code === 'mcp_config_invalid') {
    return classified(currentCopy().config.mcpInvalidConfig, raw, hint)
  }
  if (is('text_invalid_audio_data')) {
    return t.asrInvalidAudio
  }
  if (code === 'ai_not_configured' || code === 'AI_NOT_CONFIGURED') {
    return t.aiNotConfigured
  }
  if (code === 'speech_not_configured') {
    return t.speechNotConfigured
  }
  if (
    code === 'realtime_session_unavailable'
  ) {
    return t.realtimeSessionUnavailable
  }
  if (is('speech_tts_openai_required')) {
    return t.speechTtsOpenAiRequired
  }
  if (is('speech_tts_no_audio')) {
    return t.speechTtsNoAudio
  }
  if (is('speech_text_too_long')) {
    return t.speechTextTooLong
  }
  if (is('speech_batch_empty')) {
    return t.speechBatchEmpty
  }
  if (is('speech_text_empty')) {
    return t.emptyDialogueText
  }
  if (is('speech_batch_too_many')) {
    return t.speechBatchTooMany
  }
  if (code === 'speech_upstream_failed') {
    return t.speechUpstreamFailed
  }
  if (code === 'domain_invalid') {
    return t.domainInvalid
  }
  if (is('oauth_authorize_failed')) {
    return t.oauthStartFailed
  }
  if (is('ROOM_MATERIALIZE_FAILED')) {
    return t.roomJoinFailed
  }
  if (code === 'oauth_slug_required') {
    return t.oauthSlugRequired
  }
  if (code === 'oauth_slug_invalid') {
    return t.oauthSlugInvalid
  }
  if (code === 'oauth_slug_duplicate') {
    return t.oauthSlugDuplicate
  }
  if (code === 'oauth_client_id_required') {
    return t.oauthClientIdRequired
  }
  if (code === 'oauth_client_secret_required') {
    return t.oauthClientSecretRequired
  }
  if (code === 'oauth_discovery_required') {
    return t.oauthDiscoveryRequired
  }
  if (code === 'oauth_kind_unsupported') {
    return t.oauthKindUnsupported
  }
  if (is('remote_actor_unresolved')) {
    return t.remoteActorUnresolved
  }
  if (
    code === 'webfinger_failed' ||
    code === 'webfinger_not_found' ||
    code === 'webfinger_unavailable'
  ) {
    return t.webfingerFailed
  }
  if (is('psn_npsso_expired')) {
    const statusMatch = raw.match(/\b(?:status\s+|HTTP\s+)(\d{3})\b/i)
    return joinParts(
      t.psnNpssoExpired,
      statusMatch ? `HTTP ${statusMatch[1]}` : '',
      usefulExtra(hint, t.psnNpssoExpired),
    )
  }
  if (is('psn_request_failed')) {
    return classified(t.psnRequestFailed, raw, hint)
  }
  if (is('steam_not_configured')) {
    return t.steamNotConfigured
  }
  if (is('platform_disabled')) {
    return t.platformDisabled
  }
  if (code === 'api_not_found') {
    return joinParts(t.notFound, usefulExtra(hint, t.notFound))
  }
  if (code === 'invalid_user') {
    return joinParts(t.unauthorized, usefulExtra(hint, t.unauthorized))
  }
  // Tapp runtime contract: report reads fail with the shared `fetch_failed`
  // code (tapp_runtime/reports.rs:86), so only the label tells them apart.
  if (
    code === 'fetch_failed' &&
    /failed to fetch report/i.test(raw)
  ) {
    return joinParts(t.reportLoadFailed, usefulExtra(hint, t.reportLoadFailed))
  }
  if (is('fetch_failed')) {
    return classified(t.platformFetchFailed, raw, hint)
  }
  if (is('platform_refresh_reconcile_failed')) {
    return classified(
      fill(t.noticePlatformSyncFailed, { name: 'Platform' }),
      raw,
      hint,
    )
  }
  if (is('game_not_found')) {
    return t.notFound
  }
  if (is('username_required')) {
    return t.usernameRequired
  }
  if (
    code === 'agent_processing_failed' ||
    is('text_processing_failed')
  ) {
    return classified(t.agentProcessingFailed, raw, hint)
  }
  if (is('agent_apology_legacy') || is('agent_task_failed')) {
    return t.agentProcessingFailed
  }
  if (is('agent_step_skipped')) {
    return t.agentStepSkipped
  }
  if (is('agent_step_retrying')) {
    return t.agentStepRetrying
  }
  if (is('text_confirmation_failed')) {
    return t.agentConfirmFailed
  }
  if (is('agent_unsupported')) {
    return t.agentUnsupported
  }
  if (is('agent_confirm_expired')) {
    return t.agentConfirmExpired
  }
  if (is('agent_confirm_missing')) {
    return t.agentConfirmMissing
  }
  if (is('task_cancelled')) {
    return t.agentTaskCancelled
  }
  if (is('wait_channel_closed')) {
    return t.waitChannelClosed
  }
  if (is('wait_input_timeout')) {
    const hours = raw.match(/\((\d+) hours\)/)
    if (hours) {
      return fill(t.waitInputTimeoutHours, { hours: Number(hours[1]) })
    }
    return t.waitInputTimeout
  }
  if (is('rate_limited')) {
    return t.rateLimited
  }
  if (is('ai_response_invalid')) {
    return t.aiResponseInvalid
  }
  if (is('text_the_requested_resource_does_not_exist')) {
    return t.notFound
  }
  if (is('content_policy_retry')) {
    return t.contentPolicyRetry
  }
  if (is('service_not_configured')) {
    return classified(t.serviceNotConfigured, raw, hint)
  }
  if (is('missing_parameter')) {
    return classified(t.missingParameter, raw, hint)
  }
  if (is('unknown_error')) {
    return classified(t.unknown, raw, hint)
  }
  if (is('text_resume_exceeded_the_step_cap')) {
    return t.agentResumeOverCap
  }
  if (is('task_unavailable')) {
    return t.taskUnavailable
  }
  if (code === 'analytics_unavailable') {
    return t.analyticsUnavailable
  }
  const queued = raw.match(/Queued \(about (\d+) ahead\)/i)
  if (queued) return fill(t.agentQueued, { n: Number(queued[1]) })
  const queueWait = raw.match(/Waited more than (\d+) seconds/i)
  if (queueWait) return fill(t.agentQueueTimeout, { sec: Number(queueWait[1]) })
  if (is('music_not_playing')) {
    return currentCopy().music.noPlaying
  }
  if (is('text_task_submitted_waiting_to_run')) {
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
  const music = currentCopy().music
  if (is('text_playing_music')) return music.playingNow
  if (is('text_paused')) return music.pausedPlayback
  if (is('text_toggled_playback')) return music.toggledPlayback
  if (is('text_skipped_to_next_track')) return music.skippedNext
  if (is('text_skipped_to_previous_track')) {
    return music.skippedPrevious
  }
  if (is('text_volume_adjusted')) return music.volumeAdjusted
  if (is('text_muted')) return music.muted
  if (is('text_unmuted')) return music.unmuted
  if (is('text_seeked')) return music.seeked
  if (is('text_loading_bilibili_data')) {
    return fill(t.loadingNamedData, { name: 'Bilibili' })
  }
  if (is('text_loading_steam_data')) {
    return fill(t.loadingNamedData, { name: 'Steam' })
  }
  if (is('text_loading_github_data')) {
    return fill(t.loadingNamedData, { name: 'GitHub' })
  }
  if (is('text_loading_netease_data')) {
    return fill(t.loadingNamedData, { name: 'NetEase' })
  }
  if (is('text_loading_bangumi_data')) {
    return fill(t.loadingNamedData, { name: 'Bangumi' })
  }
  if (is('text_loading_x_data')) {
    return fill(t.loadingNamedData, { name: 'X' })
  }
  if (is('text_loading_discord_data')) {
    return fill(t.loadingNamedData, { name: 'Discord' })
  }
  if (is('text_loading_myanimelist_data')) {
    return fill(t.loadingNamedData, { name: 'MyAnimeList' })
  }
  if (is('text_summarizing')) return t.agentSummarizing
  if (is('text_analyzing')) return t.agentAnalyzing
  if (is('text_searching_the_web')) return t.agentWebSearch
  if (is('text_discovering_feeds')) return t.agentDiscoverFeeds
  if (is('text_subscribing_to_a_feed')) {
    return t.agentSubscribeFeed
  }
  if (is('text_loading_platform_data')) {
    return fill(t.loadingNamedData, { name: 'platform' })
  }
  if (is('text_chatting')) return t.agentChat
  if (is('text_generating_an_image')) return t.agentGenerateImage
  if (is('text_generating_a_prompt')) return t.agentGeneratePrompt
  if (is('text_comparing_content')) return t.agentCompareContent
  if (is('text_reading_aloud')) return t.agentReadingAloud
  if (is('text_searching')) return t.agentSearching
  if (is('text_generating_a_report')) return t.agentGenerateReport
  if (is('text_clearing_cache')) return t.agentClearCache
  if (is('text_listing_apps')) return t.agentListingApps
  if (is('text_opening_an_app')) return t.agentOpeningApp
  if (is('text_loading_articles')) return t.agentLoadingArticles
  if (is('text_loading_article')) return t.agentLoadingArticle
  if (is('text_loading_feeds')) return t.agentLoadingFeeds
  if (is('text_loading_feed_content')) {
    return t.agentLoadingFeedContent
  }
  if (is('text_reading_stats')) return t.agentReadingStats
  if (is('text_running_skill')) return t.agentRunningSkill
  if (is('text_calling_a_tool')) return t.agentCallingTool
  if (is('text_tapp_apps')) return currentCopy().tapp.apps
  if (is('text_tapp_widgets')) return t.tappWidgets
  if (is('text_tapp_storage')) return t.tappStorage
  if (is('text_scheduled_tasks')) return t.tappScheduledTasks
  if (is('text_tapp_executions')) return t.tappExecutions
  if (is('text_unknown_app')) return t.unknownApp
  if (is('text_untitled_report')) return t.unnamedReport
  if (is('text_unknown_title')) return t.unknownTitle
  if (is('text_untitled_content')) return t.untitledContent
  if (is('text_unknown_user')) return currentCopy().userModal.unknownUser
  if (is('text_uncategorized')) return currentCopy().phantasi.uncategorized
  if (is('text_latest_articles')) {
    return currentCopy().phantasi.latestArticles
  }
  if (is('text_a_title_is_required')) {
    return currentCopy().phantasi.noteTitleRequired
  }
  const titleTooLong = raw.match(
    /^Titles can be at most (\d+) characters \(this one is (\d+)\)$/i,
  )
  if (titleTooLong) {
    return fill(currentCopy().phantasi.noteTitleTooLong, {
      max: titleTooLong[1],
      chars: titleTooLong[2],
    })
  }
  const bodyTooLong = raw.match(
    /^Notes can be at most (\d+) characters \(this one is (\d+)\)$/i,
  )
  if (bodyTooLong) {
    return fill(currentCopy().phantasi.noteBodyTooLong, {
      max: bodyTooLong[1],
      chars: bodyTooLong[2],
    })
  }
  if (is('text_note_draft_was_updated_elsewhere')) {
    return currentCopy().phantasi.noteRevisionConflict
  }
  if (is('text_that_time_has_already_passed')) {
    return currentCopy().phantasi.noteSchedulePast
  }
  if (is('text_a_schedule_time_is_required')) {
    return currentCopy().phantasi.noteScheduleNeedTime
  }
  if (is('text_guest')) return t.guestLabel
  // Stored labels (author names, list names) keep the words they were saved
  // with; like the stored labels in error_codes.json, they are still read.
  const userNumber = raw.match(/^用户#(\d+)$/)
  if (userNumber) return fill(t.userNumber, { id: userNumber[1] })
  if (is('text_xbox_player')) return currentCopy().reportCardWidget.xboxGamerDefault
  if (is('text_psn_player')) return t.psnPlayer
  if (is('text_steam_player')) return t.steamPlayer
  if (is('text_wait_tapp_interaction')) return t.waitTappInteraction
  if (is('text_dynamic_skills')) return t.capDynamicSkills
  if (is('text_mcp_tools')) return t.capMcpTools
  if (raw.startsWith('I\'m in a very low mood and don\'t want to take on anything new')) {
    return t.agentRefuseLowMood
  }
  if (is('text_retry_step_desc')) return t.retryStepDesc
  if (is('text_skip_step_desc')) return t.skipStepDesc
  if (is('text_cancel_task_desc')) return t.cancelTaskDesc
  if (is('text_retry_step')) return t.retryStep
  if (is('text_skip_step')) return t.skipStep
  if (is('text_web_search_result')) return t.webSearchResult
  if (is('text_try_other_keyword')) return t.tryOtherKeyword
  if (is('text_check_spelling')) return t.checkSpelling
  if (is('text_web_search_result')) return t.webSearchResult
  if (is('text_netease_music_user')) return t.neteaseMusicUser
  if (is('text_bangumi_user')) return t.bangumiUser
  if (is('text_mal_user')) return t.malUser
  if (is('text_smart_reading_list')) return currentCopy().phantasi.smartReadingList
  if (is('text_board_feeds')) return currentCopy().phantasi.boardFeeds
  if (is('text_mark_read')) return t.phantasiMarkRead
  if (is('text_mark_unread')) return t.phantasiMarkUnread
  if (is('text_mark_starred')) return t.phantasiMarkStarred
  if (is('text_mark_unstarred')) return t.phantasiMarkUnstarred
  if (is('text_mark_later')) return t.phantasiMarkLater
  if (is('text_need_more_info')) {
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
  // Confirmation text with the tool and server names in it
  // (capability/mod.rs mcp_capability).
  const mcpTool = raw.match(/^This will call tool '(.+)' on MCP server '(.+)'$/i)
  if (mcpTool) {
    return fill(t.confirmMcpTool, { server: mcpTool[2], tool: mcpTool[1] })
  }
  const mcpToolsLoaded = raw.match(/^Loaded (\d+) tools$/i)
  if (mcpToolsLoaded) {
    return fill(t.noticeMcpToolsLoaded, { n: Number(mcpToolsLoaded[1]) })
  }
  if (is('text_maintenance_retry_succeeded')) {
    return t.noticeMcpMaintenanceRetry
  }
  if (is('text_auto_restart_succeeded')) {
    return t.noticeMcpAutoRestart
  }
  if (is('text_updater_watch_timeout')) return t.noticeUpdaterWatchTimeout
  if (is('text_unknown_artist')) return currentCopy().library.unknownArtist
  // Scheduled-task name stored in the database (platform_auto_refresh.rs).
  const autoRefreshNamed = raw.match(/^Auto-refresh (.+) data$/i)
  if (autoRefreshNamed) {
    return fill(t.autoRefreshNamed, { name: autoRefreshNamed[1] })
  }
  // Stored notification title (notification_producers.rs).
  const heartbeatTask = raw.match(/^Scheduled task:\s*(\S.*)$/i)
  if (heartbeatTask) {
    return fill(t.noticeHeartbeatTask, { name: heartbeatTask[1] })
  }
  if (is('text_confirm_add_feed')) return t.confirmAddFeed
  if (is('text_confirm_phantasi_schedule')) return t.confirmPhantasiSchedule
  if (is('text_confirm_http_fetch')) return t.confirmHttpFetch
  if (is('text_confirm_create_tapp_task')) return t.confirmCreateTappTask
  if (is('text_confirm_trigger_tapp_task')) return t.confirmTriggerTappTask
  if (is('text_confirm_platform_job')) return t.confirmPlatformJob
  if (is('text_confirm_analyze_tapp_ui')) return t.confirmAnalyzeTappUi
  if (is('text_confirm_tapp_interact')) return t.confirmTappInteract
  if (is('text_confirm_page_interact')) return t.confirmPageInteract
  if (is('text_confirm_analyze_page')) return t.confirmAnalyzePage
  if (is('text_confirm_write_target')) return t.confirmWriteTarget
  if (is('text_agent_all_done')) return t.agentAllDone
  if (is('text_hi_how_can_i_help')) {
    return t.agentGreeting
  }
  if (is('text_agent_understanding')) return t.agentUnderstanding
  if (is('text_agent_planning')) return t.agentPlanning
  if (is('text_agent_need_clarification')) return t.agentNeedClarification
  // Confirmation text naming the step (response_agent.rs).
  const willRun = raw.match(/^This will run (.+)$/)
  if (willRun) return fill(t.willExecute, { name: willRun[1] })
  if (is('text_cap_category_data')) return t.capCategoryData
  if (is('text_cap_category_write')) return t.capCategoryWrite
  if (is('text_cap_category_ai')) return t.capCategoryAi
  if (is('text_cap_category_create')) return t.capCategoryCreate
  if (is('text_cap_category_system')) return t.capCategorySystem
  if (is('text_cap_category_external')) return t.capCategoryExternal
  if (is('text_cap_category_interface')) return t.capCategoryInterface
  if (is('text_agent_need_more_info')) return t.agentNeedMoreInfo
  if (is('text_update_queued')) {
    return t.noticeUpdaterSubmitted
  }
  // Agent step errors: plain copy, since a retry appends
  // " (previous N attempts: …)" that the table's detail tail would echo.
  if (is('agent_task_interrupted')) return t.agentTaskInterrupted
  if (is('agent_step_timeout')) return t.agentStepTimeout
  if (is('agent_input_empty')) return t.agentInputEmpty
  if (is('agent_input_too_long')) return t.agentInputTooLong
  if (is('agent_subscribe_failed')) return t.subscribeAllFailed
  if (is('agent_write_items_over_cap')) return t.writeItemsOverCap
  if (is('agent_speech_text_missing')) return t.emptyDialogueText
  if (is('agent_feed_not_found')) return t.feedNotFound
  if (is('phantasi_load_failed')) {
    return classified(joinParts(t.phantasiLoadFailed, failedAction(raw)), raw, hint)
  }
  if (is('phantasi_reading_state_failed')) {
    return classified(joinParts(t.readingStateFailed, failedAction(raw)), raw, hint)
  }
  if (is('admin_only_action')) {
    return joinParts(t.forbidden, clip(raw), usefulExtra(hint, t.forbidden))
  }
  if (is('rsshub_save_failed')) {
    return classified(joinParts(t.rsshubSaveFailed, failedAction(raw)), raw, hint)
  }
  if (is('rsshub_unavailable')) return t.rsshubUnavailable
  if (is('agent_pipeline_too_many_steps')) return t.pipelineTooManySteps
  if (is('heartbeat_admin_required')) return t.heartbeatAdminRequired
  if (is('agent_step_needs_confirm')) return t.stepNeedsConfirm
  if (is('agent_service_not_configured')) return t.serviceNotConfigured
  if (is('agent_feed_name_required')) return t.feedNameRequired
  if (is('agent_task_submit_failed')) return t.taskSubmitFailed
  if (is('text_invalid_tappid')) {
    return currentCopy().tapp.invalidId
  }
  if (code === 'media_action_invalid') {
    return classified(t.mediaActionInvalid, raw, hint)
  }
  if (code === 'media_mode_invalid') {
    return classified(t.mediaModeInvalid, raw, hint)
  }
  if (is('text_invalid_url')) {
    return t.invalidUrl
  }
  // Both labels share `actor_unavailable`, the code remote servers see.
  if (/failed to initialize federation identity/i.test(raw)) {
    return classified(t.federationInitFailed, raw, hint)
  }
  if (is('federation_data_failed')) {
    return classified(joinParts(t.federationDataFailed, failedAction(raw)), raw, hint)
  }
  if (/failed to read user id/i.test(raw)) {
    return classified(t.database, raw, hint)
  }
  // external: remote federation peers answer with these texts, and delivery
  // records keep them inside "HTTP 5xx: {…}" / "PERMANENT HTTP …" strings.
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
  // external: legacy peer text; the MIME after the colon is not shown.
  if (/^unsupported attachment mime\b/i.test(raw)) {
    return t.byCode.attachment_type_unsupported
  }
  // Catch-all for code-less "X is required" text from agent handlers.
  if (
    / is required$| are required$|key rotation requires confirm/i.test(raw)
  ) {
    return t.agentInputEmpty
  }
  // Plain copy: the tail names the Notion parser's reason.
  if (is('notion_url_invalid')) return t.notionUrlInvalid
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
  // Agent handlers name the task id (heartbeat.rs, system_op.rs); HTTP
  // labels of this shape already arrive as `not_found`.
  if (/^task ['"]?[^'"]+['"]? not found$/i.test(raw)) {
    return t.notFound
  }
  if (is('text_no_library_data_available')) {
    return currentCopy().library.loadFailed
  }
  if (is('phantasi_source_save_failed')) {
    return classified(
      joinParts(t.phantasiSourceSaveFailed, failedAction(raw)),
      raw,
      hint,
    )
  }
  if (is('phantasi_category_save_failed')) {
    return classified(joinParts(t.phantasiCategorySaveFailed, failedAction(raw)), raw, hint)
  }

  const byStatus = status > 0 ? httpStatusMessage(status) : ''
  const useful = isUselessErrorText(raw) ? '' : clip(raw)
  const extraHint = usefulExtra(hint, byStatus, useful, fallbackText)

  if (code === 'unmapped') {
    if (status >= 400) return joinParts(byStatus, extraHint)
    return joinParts(t.operationFailed, extraHint)
  }
  // Catch-all for code-less "Failed to …" text (agent handlers, frontend).
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

/** The step a "Failed to <step>" label names, shown next to its category. */
function failedAction(raw: string): string {
  return raw.match(/^failed to ([^:]+)/i)?.[1]?.trim() || ''
}

function usefulExtra(text: string, ...known: string[]): string {
  if (!text || isUselessErrorText(text)) return ''
  if (known.some((item) => item && text === item)) return ''
  return clip(text)
}
