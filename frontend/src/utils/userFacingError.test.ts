import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { before, describe, it } from 'node:test'
import errorCodeSpec from '../../../shared/error_codes.json' with { type: 'json' }
import { loadShellNamespace } from '../i18n/loadLocale.ts'
import { currentCopy, formatCurrent } from '../i18n/localeCopy.ts'
import { ApiError } from '../services/api.ts'
import {
  httpStatusMessage,
  isUselessErrorText,
  userFacingError,
  withoutForeignProse,
} from './userFacingError.ts'

/** What `AppError` sends for a public label: the label plus its inferred code. */
function backendError(label: string, status = 500): ApiError {
  const labels: Record<string, string> = errorCodeSpec.labels
  return new ApiError(label, status, labels[label] ?? 'unmapped')
}

/** A client-side error that carries its own code. */
function codedError(message: string, code: string): Error {
  return Object.assign(new Error(message), { code })
}

function fill(
  template: string,
  params: Record<string, string | number> = {},
): string {
  return formatCurrent(template, params)
}

describe('userFacingError', () => {
  before(async () => {
    await Promise.all([
      loadShellNamespace('tapp', 'en-US'),
      loadShellNamespace('phantasi', 'en-US'),
      loadShellNamespace('merope', 'en-US'),
    ])
  })

  it('guides missing AI configuration without misclassifying an upstream failure', () => {
    for (const code of ['ai_not_configured', 'AI_NOT_CONFIGURED']) {
      assert.equal(userFacingError(new ApiError('No AI provider configured', 409, code)), currentCopy().errors.aiNotConfigured)
    }
    assert.notEqual(userFacingError(new ApiError('bad gateway: client error (Connect)', 502)), currentCopy().errors.aiNotConfigured)
  })

  it('treats API Error: 500 and JSON dumps as useless', () => {
    assert.equal(isUselessErrorText('API Error: 500'), true)
    assert.equal(isUselessErrorText('{"error":"boom"}'), true)
    assert.equal(isUselessErrorText('Failed to fetch'), true)
    assert.equal(isUselessErrorText('HTTP 500: Internal Server Error'), true)
    assert.equal(isUselessErrorText('CSRF token unavailable'), true)
    assert.equal(isUselessErrorText('Database error'), true)
    assert.equal(isUselessErrorText('Failed to save notification preferences'), true)
    assert.equal(isUselessErrorText('Failed to process password'), true)
    assert.equal(isUselessErrorText('Failed to create account'), true)
    assert.equal(isUselessErrorText('Failed to restore settings'), true)
    assert.equal(isUselessErrorText('Failed to clear cache'), true)
    assert.equal(isUselessErrorText('Failed to parse AI response'), true)
    assert.equal(isUselessErrorText('AI error: model exploded'), true)
    assert.equal(isUselessErrorText('Steam 未返回游戏数据'), false)
  })

  it('maps unmapped codes without leaking leftover English', () => {
    const withStatus = new ApiError(
      'Failed to frobnicate the widget',
      500,
      'unmapped',
    )
    const statusText = userFacingError(withStatus)
    assert.equal(/frobnicate/i.test(statusText), false)
    assert.match(statusText, /500/)
    assert.equal(
      userFacingError({ code: 'unmapped' }),
      currentCopy().errors.operationFailed,
    )
    assert.equal(
      userFacingError('Failed to frobnicate the widget'),
      currentCopy().errors.operationFailed,
    )
  })

  it('maps a closed federation gate to the unified region copy', () => {
    const copy = currentCopy().errors.federationDisabledRegion
    assert.match(copy, /not supported in this region/i)
    assert.equal(
      userFacingError(
        new ApiError(
          'Federation is not supported in this region',
          404,
          'federation_disabled_region',
        ),
      ),
      copy,
    )
    assert.equal(
      userFacingError('Federation disabled in this region'),
      copy,
    )
    assert.equal(
      userFacingError('Federation is disabled on this instance'),
      copy,
    )
    assert.equal(
      userFacingError(
        "Federation apps cannot be downloaded or installed in this server's region",
      ),
      copy,
    )
    assert.equal(/not found/i.test(copy), false)
  })

  it('maps locale_invalid from the catalog', () => {
    const err = new ApiError('Bad request', 400, 'locale_invalid')
    assert.equal(userFacingError(err), currentCopy().errors.localeInvalid)
  })

  it('maps new stable save/load codes from the catalog', () => {
    assert.equal(
      userFacingError(new ApiError('Failed to load config', 500, 'config_load_failed')),
      currentCopy().config.loadConfigFailed,
    )
    assert.equal(
      userFacingError(
        new ApiError('Failed to update permissions', 500, 'permissions_save_failed'),
      ),
      currentCopy().config.permissionsSaveFailed,
    )
    assert.equal(
      userFacingError(new ApiError('Failed to persist Tapp', 500, 'tapp_save_failed')),
      currentCopy().errors.tappSaveFailed,
    )
    assert.equal(
      userFacingError('Failed to load configuration'),
      currentCopy().config.loadConfigFailed,
    )
  })

  it('maps HTTP status to a localized reason', () => {
    assert.match(httpStatusMessage(401), /登录|Sign in|ログイン/)
    assert.match(httpStatusMessage(502), /502/)
    assert.match(httpStatusMessage(0), /网络|network|ネットワーク/i)
  })

  it('prefers status copy over boilerplate and keeps useful detail', () => {
    const err = new ApiError('Steam 未返回游戏数据。请确认资料公开。', 502, 'platform_refresh_failed')
    const text = userFacingError(err, '操作失败')
    assert.match(text, /502/)
    assert.match(text, /Steam/)
  })

  it('does not leak API Error: status as the only message', () => {
    const err = new ApiError('API Error: 500', 500)
    const text = userFacingError(err)
    assert.equal(/API Error/i.test(text), false)
    assert.match(text, /500/)
  })

  it('maps database_error to actionable copy', () => {
    const err = new ApiError('Database error', 500, 'database_error')
    const text = userFacingError(err)
    assert.match(text, /数据|data|データ/i)
    assert.equal(/Database error/i.test(text), false)
  })

  it('maps platform test field-required codes', () => {
    const text = userFacingError(
      new ApiError('Username is required', 400, 'username_required'),
    )
    assert.equal(/Username is required/i.test(text), false)
    const uid = userFacingError(new ApiError('UID is required', 400, 'uid_required'))
    assert.equal(/UID is required/i.test(uid), false)
  })

  it('maps leftover Chinese agent wait-loop messages', () => {
    assert.equal(
      /任务已取消/.test(userFacingError('任务已取消')),
      false,
    )
    assert.equal(
      /任务等待通道已断开/.test(userFacingError('任务等待通道已断开')),
      false,
    )
    assert.equal(
      /等待用户输入已超时/.test(
        userFacingError('等待用户输入已超时（服务重启后发现已过期）'),
      ),
      false,
    )
    assert.equal(
      userFacingError('用户取消了任务'),
      currentCopy().errors.agentTaskCancelled,
    )
    assert.equal(
      userFacingError('Resume exceeded the step cap'),
      currentCopy().errors.agentResumeOverCap,
    )
    assert.equal(
      userFacingError('恢复执行超出步骤上限').includes('恢复执行超出'),
      false,
    )
  })

  it('maps leftover Chinese agent progress chrome', () => {
    const queued = userFacingError('Queued (about 3 ahead)')
    assert.equal(queued, fill(currentCopy().errors.agentQueued, { n: 3 }))
    const bili = userFacingError('获取 B 站数据')
    assert.equal(/获取 B 站数据/.test(bili), false)
    assert.match(bili, /Bilibili/)
  })

  it('maps realtime session and missing-report codes', () => {
    const voice = userFacingError(
      new ApiError('Realtime session is unavailable', 404, 'realtime_session_unavailable'),
    )
    assert.equal(/Realtime session is unavailable/i.test(voice), false)
    const report = userFacingError(
      new ApiError('No valid report found', 200, 'no_valid_report'),
    )
    assert.equal(/No valid report found/i.test(report), false)
  })

  it('maps leftover Chinese share-empty and query-token codes', () => {
    const share = userFacingError('分享内容为空：请提供 text，或 title/summary')
    assert.equal(/分享内容为空/.test(share), false)
    const token = userFacingError(
      new ApiError(
        'Do not pass X bearer tokens in the query string',
        400,
        'bearer_token_not_allowed',
      ),
    )
    assert.equal(/query string/i.test(token), false)
  })

  it('maps configuration_mode away from the English label', () => {
    const err = new ApiError(
      'Service in configuration mode',
      503,
      'configuration_mode',
    )
    const text = userFacingError(err)
    assert.match(text, /配置|setup|セットアップ/i)
    assert.equal(/Service in configuration mode/i.test(text), false)
  })

  it('maps setup_completed away from the English label', () => {
    const err = new ApiError(
      'Setup already completed',
      403,
      'setup_completed',
    )
    const text = userFacingError(err)
    assert.match(text, /安装|complete|完了/i)
    assert.equal(/Setup already completed/i.test(text), false)
  })

  it('maps report fetch_failed away from platform fetch copy', () => {
    const text = userFacingError(
      new ApiError('Failed to fetch report', 500, 'fetch_failed'),
    )
    assert.equal(/Failed to fetch report/i.test(text), false)
    assert.equal(/platform/i.test(text), false)
  })

  it('maps API_NOT_FOUND to not-found copy', () => {
    const text = userFacingError(
      new ApiError("API 'demo' not defined in manifest", 404, 'API_NOT_FOUND'),
    )
    assert.match(text, /找不到|not found|見つかり/i)
    assert.equal(/not defined in manifest/i.test(text), false)
  })

  it('maps file and payload size limits without the English template', () => {
    const file = userFacingError(
      new ApiError(
        'File size must be between 1 byte and 104857600 bytes',
        400,
        'file_too_large',
      ),
    )
    assert.equal(/File size must be between/i.test(file), false)
    const payload = userFacingError(
      new ApiError(
        'Message payload too large: 9000 bytes (max 4194304)',
        413,
        'payload_too_large',
      ),
    )
    assert.equal(/Message payload too large/i.test(payload), false)
    assert.match(payload, /9000|4194304/)
  })

  it('maps leftover Chinese setup migration copy', () => {
    const text = userFacingError('数据库迁移失败，请检查数据库连接和权限设置')
    assert.equal(text.includes('请检查数据库连接和权限设置'), false)
  })

  it('maps invalid MCP config away from serde dumps', () => {
    const err = new ApiError(
      'invalid MCP config: missing field command',
      400,
      'mcp_config_invalid',
    )
    const text = userFacingError(err)
    assert.equal(/missing field/i.test(text), false)
  })

  it('maps phantasi feed parse to a name hint', () => {
    const err = new ApiError(
      'Failed to parse feed. Please provide a name.',
      400,
      'feed_parse_failed',
    )
    const text = userFacingError(err)
    assert.equal(/Failed to parse feed/i.test(text), false)
  })

  it('maps phantasi AI parse failures', () => {
    const err = new ApiError('Failed to parse AI response', 500, 'ai_response_invalid')
    const text = userFacingError(err)
    assert.equal(/Failed to parse/i.test(text), false)
    assert.match(text, /AI|解析/)
  })

  it('maps password_failed away from Failed to process password', () => {
    const err = new ApiError('Failed to process password', 500, 'password_failed')
    const text = userFacingError(err)
    assert.equal(/Failed to process/i.test(text), false)
  })

  it('extracts HTTP status from English fallbacks', () => {
    const text = userFacingError(
      new Error('Could not load site face (HTTP 502)'),
      '操作失败',
    )
    assert.match(text, /502/)
    assert.notEqual(text, 'Could not load site face (HTTP 502)')
  })

  it('maps speech labels sent without a code', () => {
    assert.equal(
      userFacingError('Please upload audio data'),
      currentCopy().errors.asrInvalidAudio,
    )
    assert.equal(
      userFacingError('Speech text is too long'),
      currentCopy().errors.speechTextTooLong,
    )
    assert.equal(
      userFacingError('Dialogue list is empty'),
      currentCopy().errors.speechBatchEmpty,
    )
    const notConfigured = userFacingError(
      new ApiError('Speech service is not configured', 503),
    )
    assert.match(notConfigured, /密钥|key|キー/i)
    assert.equal(/腾讯云密钥/.test(notConfigured), false)
  })

  it('maps setup claim leftover Chinese', () => {
    const text = userFacingError(
      '无法写入安装认领标记；管理员账户尚未提交，请检查数据目录权限后重试。',
    )
    assert.equal(text.includes('尚未提交'), false)
  })

  it('maps domain validation away from URL crate dumps', () => {
    const text = userFacingError(
      new ApiError('Invalid URL: relative URL without a base', 400, 'domain_invalid'),
    )
    assert.equal(/relative URL/i.test(text), false)
    assert.match(text, /https:\/\/example\.com|アドレス|地址/)
  })

  it('maps oauth start without state serialize dumps', () => {
    const text = userFacingError(
      new ApiError('state serialize: trailing characters', 500, 'oauth_authorize_failed'),
    )
    assert.equal(/state serialize/i.test(text), false)
  })

  it('maps leftover oauth slug dumps', () => {
    const text = userFacingError(
      new ApiError("invalid slug '!!': only ASCII letters", 400, 'oauth_slug_invalid'),
    )
    assert.equal(/ASCII/i.test(text), false)
  })

  it('maps webfinger dumps away from URL and reqwest text', () => {
    const text = userFacingError(
      new ApiError(
        'WebFinger lookup failed for https://x.example/.well-known/webfinger: error sending request for url',
        502,
        'webfinger_failed',
      ),
    )
    assert.equal(/error sending request/i.test(text), false)
    assert.equal(/well-known/i.test(text), false)
  })

  it('maps leftover agent processing Chinese without dumping internals', () => {
    const text = userFacingError('处理失败: database connection closed')
    assert.match(text, /database connection closed/)
    assert.equal(/处理失败/.test(text), false)
    assert.notEqual(text, currentCopy().errors.operationFailed)
    const leftover = userFacingError('抱歉，这次没能完成你的请求：boom')
    assert.equal(/boom/.test(leftover), false)
    const interrupted = userFacingError('任务因服务重启而中断，请重新提交')
    assert.equal(/服务重启/.test(interrupted), false)
  })

  it('maps leftover Notion fetch dumps and keeps status or official phrase', () => {
    const token = userFacingError(
      'Notion API error: HTTP 401: API token is invalid.',
    )
    const dump = userFacingError(
      'Notion API error: Request failed: error sending request for url (https://api.notion.com/v1/databases)',
    )
    assert.match(token, /401/)
    assert.match(token, /API token is invalid/)
    assert.equal(/error sending request|api\.notion\.com/.test(dump), false)
    assert.notEqual(token, dump)
    assert.notEqual(dump, currentCopy().errors.operationFailed)
  })

  it('maps leftover Notion URL dumps', () => {
    const text = userFacingError(
      new ApiError('Invalid Notion URL: Unknown resource type: workspace', 400, 'notion_url_invalid'),
    )
    assert.equal(/Unknown resource/i.test(text), false)
  })

  it('maps leftover feed name Chinese', () => {
    const text = userFacingError('订阅源名称不能为空')
    assert.equal(text.includes('不能为空'), false)
  })

  it('maps leftover subscribe private-network Chinese', () => {
    const text = userFacingError('不允许访问内网地址')
    assert.equal(text.includes('不允许访问'), false)
    assert.equal(/192\.|localhost|内网 IPv6/.test(text), false)
  })

  it('maps leftover RSSHub setup dumps', () => {
    const text = userFacingError(
      '初始化 RSSHub 默认实例失败: relation "rsshub_instances" does not exist。请确认数据库已迁移且可写（rsshub_instances 表）。',
    )
    assert.equal(/rsshub_instances|does not exist/.test(text), false)
  })

  it('maps leftover RSSHub instance CRUD without SQL and keeps load vs save', () => {
    const load = userFacingError(
      'Failed to fetch RSSHub instances: relation "rsshub_instances" does not exist',
    )
    const save = userFacingError('Failed to create RSSHub instance')
    const denied = userFacingError('Only admins can modify global RSSHub instances')
    assert.equal(/rsshub_instances|does not exist/.test(load), false)
    assert.match(load, /加载|load|読み込/)
    assert.match(save, /保存|save/)
    assert.match(denied, /权限|permission|権限/)
    assert.notEqual(load, save)
    assert.notEqual(save, currentCopy().errors.rsshubUnavailable)
    assert.notEqual(load, currentCopy().errors.database)
  })

  it('maps leftover article lookup dumps', () => {
    const text = userFacingError(
      '未找到文章: id=Some(12), key=Some("abc"), url=Some("https://example.com/x")',
    )
    assert.equal(/example\.com|Some\(/.test(text), false)
  })

  it('maps leftover feed not-found Chinese without leaking the query', () => {
    const text = userFacingError('未找到名为「政治」的订阅源或作者')
    assert.equal(text.includes('政治'), false)
    assert.equal(text.includes('未找到名为'), false)
  })

  it('maps leftover Gemini dumps away from API bodies', () => {
    const text = userFacingError(
      'Gemini API error 400: {"error":{"message":"API key not valid"}}',
    )
    assert.equal(/API key not valid|error":/.test(text), false)
  })

  it('maps leftover scheduled-task Chinese without leaking internals', () => {
    const text = userFacingError('前端任务执行失败')
    assert.equal(text.includes('前端任务'), false)
  })

  it('maps leftover bare 失败 without showing it as the only copy', () => {
    const text = userFacingError('失败')
    assert.notEqual(text, '失败')
  })

  it('maps leftover skip-step Chinese', () => {
    const text = userFacingError('用户选择跳过错误步骤')
    assert.equal(text.includes('用户选择'), false)
    assert.notEqual(text, currentCopy().errors.agentProcessingFailed)
    assert.notEqual(text, currentCopy().errors.operationFailed)
  })

  it('maps leftover scheduler task-id dumps', () => {
    const text = userFacingError("Task 'abc-123' not found")
    assert.equal(/abc-123/.test(text), false)
  })

  it('maps leftover rig compile dumps', () => {
    const text = userFacingError(
      'Rig compilation failed: missing field `layers` at line 1 column 12',
    )
    assert.equal(/missing field|layers/.test(text), false)
    assert.notEqual(text, currentCopy().merope.visualFailed)
    assert.match(text, /骨骼|rig|リグ/i)
  })

  it('maps leftover federation identity dumps', () => {
    const text = userFacingError('Failed to initialize federation identity')
    assert.equal(/initialize federation/i.test(text), false)
  })

  it('maps leftover transfer status dumps', () => {
    const text = userFacingError('Transfer is already sending')
    assert.equal(/already sending/.test(text), false)
  })

  it('maps leftover attachment MIME dumps', () => {
    const text = userFacingError('Unsupported attachment MIME: application/x-dump')
    assert.equal(/application\/x-dump/.test(text), false)
  })

  it('maps leftover attachment URL path dumps', () => {
    const text = userFacingError(
      'Attachment URL must be a media file uploaded via POST /api/federation/media',
    )
    assert.equal(/federation\/media|POST \//.test(text), false)
  })

  it('maps leftover analytics export codes', () => {
    const text = userFacingError('export_integrity_failed')
    assert.equal(/export_integrity|integrity_seal/.test(text), false)
  })

  it('maps leftover Tapp archive dumps', () => {
    const text = userFacingError('Invalid Tapp archive entry: zip local header')
    assert.equal(/zip local header/.test(text), false)
  })

  it('maps game config dumps away from protocol internals', () => {
    const text = userFacingError(
      new ApiError(
        'game.max_players must be 2-16',
        400,
        'GAME_CONFIG_INVALID',
      ),
    )
    assert.equal(/max_players/.test(text), false)
  })

  it('maps leftover Tapp manifest dumps', () => {
    const text = userFacingError(
      'Invalid manifest.json: missing field `id` at line 1 column 12',
    )
    assert.equal(/missing field|column 12/.test(text), false)
    const legacy = userFacingError(
      'manifest.json uses the pre-layer format (main, hasPage). see docs/features/TAPP_FILE_FORMAT.md',
    )
    assert.equal(/hasPage|TAPP_FILE_FORMAT/.test(legacy), false)
    const schema = userFacingError(
      'Invalid Agent schema agents/chat.json: Data Exchange schema does not support $ref',
    )
    assert.equal(/\$ref|agents\/chat/.test(schema), false)
  })

  it('maps leftover scheduler action dumps', () => {
    const text = userFacingError(
      'Invalid backend action: missing field `action` at line 1 column 2',
    )
    assert.equal(/missing field|column 2/.test(text), false)
  })

  it('maps leftover generic platform cache leftovers without os dumps', () => {
    const missing = userFacingError('Platform data not found')
    const read = userFacingError('Failed to read cache: Permission denied (os error 13)')
    assert.match(missing, /缓存|cached|キャッシュ/)
    assert.equal(/os error 13|Permission denied \(os/.test(read), false)
    assert.notEqual(missing, currentCopy().errors.operationFailed)
    assert.notEqual(read, currentCopy().errors.database)
  })

  it('maps leftover platform cache and phantasi reads without dumps', () => {
    const missing = userFacingError('No cached steam data')
    const disk = userFacingError(
      'Failed to read steam data: not enough disk space',
    )
    const items = userFacingError(
      'Failed to fetch phantasi items: relation "phantasi_items" does not exist',
    )
    assert.match(missing, /Steam/)
    assert.equal(/No cached steam data/.test(missing), false)
    assert.match(disk, /Steam/)
    assert.match(disk, /disk|空间|空き/)
    assert.equal(/phantasi_items|does not exist/.test(items), false)
    assert.match(items, /加载|load|読み込/)
    assert.notEqual(missing, disk)
    assert.notEqual(items, currentCopy().errors.operationFailed)
    assert.notEqual(items, currentCopy().errors.database)
  })

  it('maps leftover phantasi refresh without SQL and keeps fetch vs save', () => {
    const fetch = userFacingError('Failed to fetch feed: timed out')
    const save = userFacingError(
      'Failed to update source: relation "phantasi_sources" does not exist',
    )
    assert.match(fetch, /刷新|refresh|更新/)
    assert.match(fetch, /timed out/)
    assert.equal(/phantasi_sources|does not exist/.test(save), false)
    assert.match(save, /保存|save/)
    assert.notEqual(fetch, save)
    assert.notEqual(save, currentCopy().errors.operationFailed)
    assert.notEqual(save, currentCopy().errors.database)
  })

  it('maps leftover phantasi list save delete and category without unifying them', () => {
    const list = userFacingError(
      'Failed to list sources: relation "phantasi_sources" does not exist',
    )
    const save = userFacingError('Failed to save source')
    const remove = userFacingError('Failed to delete source')
    const categorySave = userFacingError('Failed to save category')
    const categoryDelete = userFacingError('Failed to delete category')
    const articles = userFacingError('Failed to list articles')
    assert.equal(/phantasi_sources|does not exist/.test(list), false)
    assert.match(list, /加载|load|読み込/)
    assert.match(list, /list sources/)
    assert.match(save, /保存|save/)
    assert.match(remove, /删除|delete|削除/)
    assert.match(categorySave, /分类|category|カテゴリ/)
    assert.match(categoryDelete, /分类|category|カテゴリ/)
    assert.match(categoryDelete, /删除|delete|削除/)
    assert.match(articles, /加载|load|読み込/)
    assert.match(articles, /list articles/)
    assert.notEqual(list, save)
    assert.notEqual(save, remove)
    assert.notEqual(categorySave, categoryDelete)
    assert.notEqual(remove, categoryDelete)
    assert.notEqual(list, currentCopy().errors.database)
    assert.notEqual(save, currentCopy().errors.database)
  })

  it('maps leftover phantasi parse, discover, and invalid URL without dumps', () => {
    const parse = userFacingError('Failed to parse feed: not valid RSS')
    const named = userFacingError(
      new ApiError(
        'Failed to parse feed. Please provide a name.',
        400,
        'feed_parse_failed',
      ),
    )
    const discover = userFacingError(
      new ApiError('Failed to fetch feed: timed out', 400, 'feed_discover_failed'),
    )
    const refresh = userFacingError('Failed to fetch feed: timed out')
    const invalid = userFacingError(
      'Invalid feed URL: Unsafe or invalid URL',
    )
    const dump = userFacingError(
      'Failed to parse feed: expected value at line 1 column 1',
    )
    assert.match(parse, /解析|parse/)
    assert.match(parse, /not valid RSS/)
    assert.notEqual(parse, named)
    assert.match(named, /名称|name|名前/)
    assert.match(discover, /探测|Detection|検出|network|网络|ネットワーク/)
    assert.match(discover, /timed out/)
    assert.notEqual(discover, refresh)
    assert.match(invalid, /地址|address|アドレス/)
    assert.match(invalid, /Unsafe or invalid URL/)
    assert.equal(/line 1 column 1|expected value/.test(dump), false)
    assert.notEqual(parse, refresh)
    assert.notEqual(parse, currentCopy().errors.operationFailed)
    assert.notEqual(invalid, parse)
  })

  it('maps scheduler store steps and keeps the step', () => {
    const register = userFacingError('Failed to create scheduled task')
    const list = userFacingError('Failed to list scheduled tasks')
    assert.match(register, /create scheduled task/)
    assert.match(list, /list/)
    assert.notEqual(register, list)
    assert.notEqual(register, currentCopy().errors.operationFailed)
  })

  it('maps leftover steering persist dumps', () => {
    const text = userFacingError(
      new ApiError(
        'Failed to persist steering instruction: relation "tapp_registry" does not exist',
        503,
        'steering_unavailable',
      ),
    )
    assert.equal(/tapp_registry|does not exist/.test(text), false)
  })

  it('maps leftover DND schedule English', () => {
    const invalid = userFacingError(
      new ApiError('Invalid do-not-disturb start time', 400, 'dnd_schedule_invalid'),
    )
    assert.equal(/do-not-disturb start time/.test(invalid), false)
    const incomplete = userFacingError(
      new ApiError(
        'Set both start and end, or clear both',
        400,
        'dnd_schedule_incomplete',
      ),
    )
    assert.match(incomplete, /开始和结束|両方|quiet-hours|start and end/)
  })

  it('maps leftover mereope disabled and guest layout English', () => {
    const disabled = userFacingError(
      new ApiError('Agent persona is disabled', 403, 'merope_disabled'),
    )
    assert.equal(/Agent persona is disabled/.test(disabled), false)
    const guest = userFacingError(
      new ApiError(
        'Guests cannot modify list card layout',
        403,
        'GUEST_LAYOUT_READONLY',
      ),
    )
    assert.equal(/list card layout/.test(guest), false)
  })

  it('maps leftover persona load save delete and addressee without unifying them', () => {
    const load = userFacingError(backendError('Failed to load persona'))
    const save = userFacingError('Failed to save persona')
    const remove = userFacingError('Failed to delete persona')
    const addresseeLoad = userFacingError('Failed to load addressee')
    const quiet = userFacingError('Failed to save quiet-hours')
    const face = userFacingError('Failed to load avatar')
    assert.match(load, /人设|persona|ペルソナ/)
    assert.match(save, /保存|save/)
    assert.match(remove, /删除|delete|削除/)
    assert.match(addresseeLoad, /对话对象|addressee|話し相手/)
    assert.match(quiet, /quiet-hours|对话对象|話し相手/)
    assert.notEqual(load, save)
    assert.notEqual(save, remove)
    assert.notEqual(addresseeLoad, quiet)
    assert.notEqual(load, face)
    assert.notEqual(load, currentCopy().errors.database)
    assert.notEqual(save, currentCopy().merope.loadFailed)
  })

  it('maps leftover game message code without leaking validator text', () => {
    const text = userFacingError(
      new ApiError('game.payload too large for protocol', 400, 'GAME_MESSAGE_INVALID'),
    )
    assert.equal(/protocol/.test(text), false)
    const leftover = userFacingError('GAME_MESSAGE_INVALID')
    assert.equal(/GAME_MESSAGE_INVALID/.test(leftover), false)
  })

  it('maps e2e encrypt failures without the serde dump', () => {
    const text = userFacingError(
      new ApiError('payload serialize failed', 400, 'e2e_required'),
    )
    assert.equal(/payload serialize/.test(text), false)
    assert.notEqual(text, currentCopy().errors.operationFailed)
    assert.match(text, /端到端|end-to-end|エンドツーエンド/)
  })

  it('maps leftover Tapp declared-API dumps', () => {
    const http = userFacingError('HTTP request failed: error sending request for url (https://x)')
    assert.equal(/error sending request|https:\/\/x/.test(http), false)
    const proxy = userFacingError('Invalid outbound proxy: builder error')
    assert.equal(/builder error/.test(proxy), false)
    const chat = userFacingError('Invalid AI chat messages: key must be a string at line 1')
    assert.equal(/key must be a string/.test(chat), false)
    const registry = userFacingError(
      'AI_TASK_REGISTRY_UNAVAILABLE: database connection closed',
    )
    assert.match(registry, /database connection closed/)
    assert.equal(/AI_TASK_REGISTRY/.test(registry), false)
  })

  it('maps agent report save failures', () => {
    assert.equal(userFacingError('Failed to save report'), currentCopy().errors.reportSaveFailed)
  })

  it('maps leftover platform fetches without reqwest and keeps name and status', () => {
    const bili = userFacingError(
      'Failed to fetch Bilibili user: error sending request for url (https://api.bilibili.com/x/space/acc/info)',
    )
    const bangumi = userFacingError('Failed to fetch Bangumi user (HTTP 404)')
    const weather = userFacingError('Failed to fetch weather: timed out')
    const phrase = userFacingError('Failed to fetch Bilibili user: 用户不存在')
    assert.equal(/error sending request|api\.bilibili/.test(bili), false)
    assert.match(bili, /Bilibili/)
    assert.match(bangumi, /Bangumi/)
    assert.match(bangumi, /404/)
    assert.match(weather, /Weather|天气|天気/)
    assert.match(weather, /timed out/)
    assert.match(phrase, /用户不存在/)
    assert.notEqual(bili, bangumi)
    assert.notEqual(bili, currentCopy().errors.operationFailed)
    assert.notEqual(bili, currentCopy().errors.platformFetchFailed)
  })

  it('maps leftover reminder note bookmark saves without SQL and keeps the kind', () => {
    const reminder = userFacingError(
      'Failed to save reminder: relation "tapp_storage" does not exist',
    )
    const note = userFacingError('Failed to save note: db connection closed')
    const bookmark = userFacingError('Failed to save bookmark')
    assert.equal(/tapp_storage|does not exist/.test(reminder), false)
    assert.match(reminder, /提醒|reminder|リマインダー/)
    assert.match(note, /笔记|note|メモ/)
    assert.match(note, /db connection closed/)
    assert.match(bookmark, /书签|bookmark|ブックマーク/)
    assert.notEqual(reminder, note)
    assert.notEqual(note, bookmark)
    assert.notEqual(reminder, currentCopy().errors.operationFailed)
  })

  it('maps OAuth start failures sent with or without their code', () => {
    const coded = userFacingError(
      new ApiError('Failed to start Discord authorization', 500, 'oauth_authorize_failed'),
    )
    assert.equal(coded, currentCopy().errors.oauthStartFailed)
    assert.equal(
      userFacingError('Failed to start authorization'),
      currentCopy().errors.oauthStartFailed,
    )
  })

  it('maps game-presence upstream failures', () => {
    const text = userFacingError('Upstream request failed')
    assert.match(text, /502/)
    assert.equal(/Upstream request failed/.test(text), false)
  })

  it('maps leftover feed URL and Gemini JSON dumps', () => {
    const url = userFacingError('Invalid feed URL: Unsafe or invalid URL')
    assert.equal(url === 'Invalid feed URL: Unsafe or invalid URL', false)
    const gemini = userFacingError('invalid Gemini JSON: expected value at line 1')
    assert.equal(/expected value|line 1/.test(gemini), false)
  })

  it('maps leftover Tripo and scheduler-connection dumps', () => {
    const tripo = userFacingError(
      new ApiError('Tripo API request failed', 502, 'TRIPO_ERROR'),
    )
    assert.equal(tripo, currentCopy().errors.model3dFailed)
    const model = userFacingError('Invalid 3D model')
    assert.equal(model, currentCopy().errors.model3dFailed)
    const sched = userFacingError('Failed to register scheduler connection')
    assert.match(sched, /register scheduler connection/)
    assert.equal(/Failed to register scheduler/.test(sched), false)
    const enqueue = userFacingError('Failed to enqueue task')
    assert.match(enqueue, /enqueue task/)
    const schema = userFacingError(
      'Model response failed output schema validation: missing field `title`',
    )
    assert.equal(/missing field|title/.test(schema), false)
  })

  it('maps leftover avatar, scheduler, and updater dumps', () => {
    const avatar = userFacingError('Failed to load avatar sources')
    assert.equal(avatar, currentCopy().errors.avatarSourceLoadFailed)
    const scheduled = userFacingError(
      'Scheduled Tapp is no longer accessible: Record not found',
    )
    assert.match(scheduled, /Record not found/)
    assert.equal(/Scheduled Tapp is no longer/.test(scheduled), false)
    const updater = userFacingError(
      new ApiError('updater upstream 502 Bad Gateway', 502, 'updater_upstream_failed'),
    )
    assert.equal(updater, currentCopy().errors.noticeUpdaterFailed)
  })

  it('maps leftover preset fetch save delete away from updater and SQL', () => {
    const favorites = userFacingError(backendError('Failed to fetch favorites'))
    const history = userFacingError('Failed to fetch history')
    const save = userFacingError('Failed to create preset')
    const remove = userFacingError('Failed to delete preset')
    const leftoverCode = userFacingError(
      new ApiError('Failed to update preset', 500, 'preset_update_failed'),
    )
    const updater = userFacingError(
      new ApiError('updater upstream 502 Bad Gateway', 502, 'updater_upstream_failed'),
    )
    assert.equal(/agent_task_presets|does not exist/.test(favorites), false)
    assert.match(favorites, /预设|preset|プリセット/)
    assert.match(favorites, /fetch favorites/)
    assert.match(history, /fetch history/)
    assert.match(save, /保存|save/)
    assert.match(remove, /删除|delete|削除/)
    assert.notEqual(favorites, history)
    assert.notEqual(save, remove)
    assert.notEqual(favorites, updater)
    assert.notEqual(leftoverCode, updater)
    assert.notEqual(favorites, currentCopy().errors.noticeUpdaterFailed)
    assert.notEqual(favorites, currentCopy().errors.database)
  })

  it('maps leftover account lookup password and local login without unifying them', () => {
    const lookup = userFacingError(backendError('Failed to look up account'))
    const current = userFacingError('Failed to load current user')
    const username = userFacingError('Failed to check username')
    const change = userFacingError('Failed to change password')
    const set = userFacingError('Failed to set password')
    const local = userFacingError('Failed to update local login')
    const updater = userFacingError(
      new ApiError('updater upstream 502 Bad Gateway', 502, 'updater_upstream_failed'),
    )
    const register = userFacingError(
      new ApiError('Failed to create account', 500, 'account_create_failed'),
    )
    assert.match(lookup, /账号|account|アカウント/)
    assert.match(lookup, /look up account/)
    assert.match(current, /load current user/)
    assert.match(username, /check username/)
    assert.match(change, /密码|password|パスワード/)
    assert.match(set, /密码|password|パスワード/)
    assert.match(local, /本地登录|local login|ローカルログイン/)
    assert.notEqual(lookup, change)
    assert.notEqual(change, local)
    assert.notEqual(local, updater)
    assert.notEqual(lookup, register)
    assert.notEqual(lookup, currentCopy().errors.database)
    assert.notEqual(local, currentCopy().errors.noticeUpdaterFailed)
  })

  it('maps leftover admin user store failures without unifying them', () => {
    const list = userFacingError(backendError('Failed to list users'))
    const identities = userFacingError('Failed to list user identities')
    const update = userFacingError('Failed to update user')
    const unlink = userFacingError('Failed to unlink identity')
    const remove = userFacingError('Failed to delete user')
    const commit = userFacingError('Failed to commit user delete')
    assert.match(list, /用户|user|ユーザー/)
    assert.match(list, /list users/)
    assert.match(identities, /list user identities/)
    assert.match(update, /用户|user|ユーザー/)
    assert.match(unlink, /解绑|unlink|解除/)
    assert.match(remove, /删除|delete|削除/)
    assert.match(commit, /commit user delete/)
    assert.notEqual(list, identities)
    assert.notEqual(list, update)
    assert.notEqual(update, unlink)
    assert.notEqual(remove, commit)
    assert.notEqual(list, currentCopy().errors.database)
    assert.notEqual(list, currentCopy().errors.operationFailed)
    assert.notEqual(update, currentCopy().errors.operationFailed)
  })

  it('maps leftover inbox and delivery dumps', () => {
    const sql = userFacingError(
      'claim inbound receipt insert: relation "federation_inbox_receipts" does not exist',
    )
    assert.equal(/federation_inbox_receipts|does not exist/.test(sql), false)
    const peer = userFacingError('HTTP 500: {"error":"Inbox processing failed"}')
    assert.equal(/Inbox processing failed|\{"error"/.test(peer), false)
    const ownership = userFacingError(
      'object attributedTo https://a.example/users/x does not match signing actor',
    )
    assert.equal(/https:\/\/a\.example|attributedTo/.test(ownership), false)
    const header = userFacingError('Invalid `Date` header encoding')
    assert.equal(/`Date`|header encoding/.test(header), false)
    const confirm = userFacingError('Failed to persist confirmation')
    assert.equal(
      confirm,
      currentCopy().errors.byCode.agent_confirmation_store_failed,
    )
  })

  it('maps leftover federation list and key rotate without unifying them', () => {
    const following = userFacingError(
      'Failed to list following: relation "federation_follows" does not exist',
    )
    const followers = userFacingError('Failed to list followers')
    const timeline = userFacingError('Failed to load timeline')
    const rotate = userFacingError('Failed to rotate federation keys')
    assert.equal(/federation_follows|does not exist/.test(following), false)
    assert.match(following, /联邦|federation|連合/)
    assert.match(following, /list following/)
    assert.match(followers, /list followers/)
    assert.match(timeline, /timeline|时间线|タイムライン|联邦|federation|連合/)
    assert.match(rotate, /密钥|key|キー|轮换|rotate|ローテーション/)
    assert.notEqual(following, followers)
    assert.notEqual(following, rotate)
    assert.notEqual(timeline, rotate)
    assert.notEqual(following, currentCopy().errors.database)
    assert.notEqual(rotate, currentCopy().errors.database)
  })

  it('maps leftover agent write steps without unifying to database error', () => {
    const article = userFacingError(
      'Failed to find article: relation "phantasi_items" does not exist',
    )
    const read = userFacingError('Failed to update reading state')
    const storage = userFacingError('Failed to delete Tapp storage')
    const content = userFacingError('Failed to save content')
    assert.equal(/phantasi_items|does not exist/.test(article), false)
    assert.match(article, /文章|article|記事/)
    assert.match(read, /阅读|reading|読書/)
    assert.match(storage, /存储|storage|保存領域/)
    assert.match(content, /内容|content/)
    assert.notEqual(article, read)
    assert.notEqual(read, storage)
    assert.notEqual(article, currentCopy().errors.database)
    assert.notEqual(storage, currentCopy().errors.noticeScheduleFailed)
  })

  it('maps leftover agent session list save archive without unifying them', () => {
    const list = userFacingError(backendError('Failed to list sessions'))
    const create = userFacingError('Failed to create session')
    const archive = userFacingError('Failed to archive session')
    const userMsg = userFacingError('Failed to save user message')
    const assistantMsg = userFacingError('Failed to save assistant message')
    const signIn = userFacingError(new ApiError('session failed', 500, 'session_failed'))
    assert.match(list, /对话|conversation|会話/)
    assert.match(list, /list sessions/)
    assert.match(create, /保存|save/)
    assert.match(archive, /归档|archive|アーカイブ/)
    assert.match(userMsg, /user message|保存|save/)
    assert.match(assistantMsg, /assistant message|保存|save/)
    assert.notEqual(list, create)
    assert.notEqual(create, archive)
    assert.notEqual(userMsg, assistantMsg)
    assert.notEqual(create, signIn)
    assert.notEqual(list, currentCopy().errors.database)
    assert.notEqual(create, currentCopy().errors.sessionFailed)
  })

  it('maps leftover phantasi comment load save delete without unifying them', () => {
    const load = userFacingError('Failed to load comments')
    const save = userFacingError('Failed to save comment')
    const del = userFacingError('Failed to delete comment replies')
    const missing = userFacingError('Comment not found')
    assert.match(load, /加载|load|読み込/)
    assert.match(save, /保存|save/)
    assert.match(del, /删除|delete|削除/)
    assert.notEqual(load, save)
    assert.notEqual(save, del)
    assert.notEqual(missing, load)
    assert.notEqual(save, currentCopy().errors.database)
    assert.notEqual(save, currentCopy().errors.operationFailed)
  })

  it('maps skill file delete failures and keeps the cause', () => {
    const trash = userFacingError(
      new ApiError(
        'Failed to create skill trash directory: storage is not writable',
        400,
        'skill_file_failed',
      ),
    )
    const missing = userFacingError(
      new ApiError('Skill file missing: /data/skills/_auto_demo.md', 400, 'skill_file_failed'),
    )
    assert.match(trash, /技能|skill|スキル/i)
    assert.match(trash, /storage is not writable/)
    assert.match(missing, /_auto_demo\.md/)
    assert.notEqual(trash, currentCopy().errors.operationFailed)
  })

  it('maps leftover skill planning and UI analysis without dumps', () => {
    const plan = userFacingError(
      'Skill AI planning failed: error sending request for url (https://api.openai.com)',
    )
    const ui = userFacingError('UI analysis failed (HTTP 429)')
    assert.equal(/error sending request|openai\.com/.test(plan), false)
    assert.match(plan, /skill ai planning/i)
    assert.match(ui, /ui analysis/i)
    assert.match(ui, /429/)
    assert.notEqual(plan, ui)
    assert.notEqual(plan, currentCopy().errors.aiGenerationFailed)
  })

  it('maps leftover AI step failures without dumps and keeps the step', () => {
    const translate = userFacingError(
      'Translation failed: error sending request for url (https://generativelanguage.googleapis.com)',
    )
    const notes = userFacingError('Annotation generation failed (HTTP 400)')
    const timeout = userFacingError('Podcast script generation failed: timed out')
    assert.equal(/error sending request|googleapis/.test(translate), false)
    assert.match(translate, /translation/i)
    assert.match(notes, /annotation/i)
    assert.match(notes, /400/)
    assert.match(timeout, /podcast/i)
    assert.match(timeout, /timed out/)
    assert.notEqual(translate, notes)
    assert.notEqual(translate, currentCopy().errors.aiGenerationFailed)
    assert.notEqual(translate, currentCopy().errors.operationFailed)
  })

  it('maps leftover MCP runtime errors without dumps and keeps method or tool phrase', () => {
    const timeout = userFacingError(
      "MCP request timed out for method 'tools/call'",
    )
    const write = userFacingError(
      'Failed to write to MCP server: Permission denied (os error 13)',
    )
    const rpc = userFacingError('MCP error (-32601): Method not found')
    const dump = userFacingError(
      'Invalid JSON-RPC response: missing field `result` at line 1 column 2 | raw: {"error":true}',
    )
    const ready = userFacingError("MCP server 'github' is not ready (starting)")
    assert.match(timeout, /超时|timed out|タイムアウト/)
    assert.match(timeout, /tools\/call/)
    assert.equal(/os error 13|Permission denied \(os/.test(write), false)
    assert.match(write, /MCP/)
    assert.match(rpc, /Method not found/)
    assert.equal(/missing field|column 2|raw:/.test(dump), false)
    assert.match(ready, /github/)
    assert.match(ready, /starting/)
    assert.notEqual(timeout, write)
    assert.notEqual(timeout, currentCopy().config.mcpLoadFailed)
    assert.notEqual(rpc, dump)
  })

  it('maps leftover MCP and Tapp persist dumps', () => {
    const save = userFacingError(
      new ApiError('Failed to save MCP config', 503, 'mcp_config_save_failed'),
    )
    assert.equal(/mcp_config_save_failed/.test(save), false)
    const wait = userFacingError('Failed to persist Tapp interaction wait state')
    assert.equal(wait, currentCopy().errors.tappSaveFailed)
    const gen = userFacingError('Tapp generation failed')
    assert.equal(gen, currentCopy().errors.byCode.tapp_generate_failed)
    const fetch = userFacingError(
      'Fetch failed: error sending request for url (https://x)',
    )
    assert.equal(/error sending request|https:\/\/x/.test(fetch), false)
    const inbox = userFacingError('HTTP 500: {"error":"Inbox processing failed"}')
    const ready = userFacingError('Activity not ready')
    const db = userFacingError(
      'claim inbound receipt insert: relation "federation_inbox_receipts" does not exist',
    )
    assert.notEqual(wait, gen)
    assert.notEqual(wait, fetch)
    assert.notEqual(gen, inbox)
    assert.notEqual(ready, inbox)
    assert.notEqual(db, inbox)
    assert.notEqual(wait, currentCopy().errors.operationFailed)
    assert.notEqual(gen, currentCopy().errors.operationFailed)
    assert.notEqual(fetch, currentCopy().errors.operationFailed)
  })

  it('maps profile text leftovers without SQL and keeps load vs save', () => {
    const save = userFacingError(
      'Failed to save profile text source: relation "users" does not exist',
    )
    const load = userFacingError(
      'Failed to load profile text row: db connection closed',
    )
    assert.equal(/relation "|does not exist/.test(save), false)
    assert.match(save, /简介|name & bio|自己紹介/)
    assert.match(save, /保存|save/)
    assert.match(load, /加载|load|読み込/)
    assert.match(load, /db connection closed/)
    assert.notEqual(save, load)
    assert.notEqual(save, currentCopy().errors.operationFailed)
    const identity = userFacingError('Identity not found for this user')
    assert.equal(/Identity not found/.test(identity), false)
    assert.equal(identity, currentCopy().errors.byCode.identity_not_found)
  })

  it('maps avatar source leftovers away from the site-face copy', () => {
    const save = userFacingError('Failed to save avatar source')
    const face = userFacingError('Failed to load avatar')
    assert.match(save, /头像来源|avatar source|アバターの取得元/)
    assert.notEqual(save, face)
    assert.notEqual(save, currentCopy().errors.operationFailed)
  })

  it('maps platform auto-refresh reconcile failures', () => {
    const restore = userFacingError(
      'Settings restored, but platform auto-refresh could not be updated',
    )
    assert.match(restore, /自动刷新|auto-refresh/i)
    const generic = userFacingError(
      new ApiError(
        'Failed to update platform auto-refresh',
        500,
        'platform_refresh_reconcile_failed',
      ),
    )
    assert.notEqual(generic, currentCopy().errors.operationFailed)
    assert.notEqual(restore, currentCopy().errors.operationFailed)
  })

  it('maps storage preflight leftovers and keeps the path', () => {
    const text = userFacingError(
      'backend storage preflight failed; repair /app/data and /app/cache ownership/permissions for uid 1000: create storage directory /app/data: storage is not writable',
    )
    assert.equal(/os error|Permission denied \(os/.test(text), false)
    assert.match(text, /\/app\/data/)
    assert.notEqual(text, currentCopy().errors.operationFailed)
  })

  it('maps image cache leftovers and keeps size or disk cause', () => {
    const large = userFacingError('Image too large: 12582912 bytes')
    assert.match(large, /12582912|太大|大きすぎ/)
    assert.equal(/Image too large:/.test(large), false)
    const disk = userFacingError(
      'Failed to write cache file: not enough disk space',
    )
    assert.match(disk, /disk|空间|空き/)
    assert.equal(/os error/.test(disk), false)
    assert.notEqual(large, disk)
  })

  it('maps media catalog in-use delete', () => {
    assert.equal(
      userFacingError(
        new ApiError(
          'This file is still in use and cannot be deleted.',
          409,
          'MEDIA_IN_USE',
        ),
      ),
      currentCopy().errors.mediaInUse,
    )
  })

  it('maps Tapp storage leftovers and keeps the cause', () => {
    const perm = userFacingError(
      new ApiError(
        'Failed to create Tapp staging directory: storage is not writable. Check data volume ownership/permissions.',
        503,
        'tapp_save_failed',
      ),
    )
    assert.equal(/os error|Permission denied \(os/.test(perm), false)
    assert.match(perm, /writable|权限|書き込め/)
    const full = userFacingError(
      new ApiError('Failed to activate staged Tapp: not enough disk space.', 503, 'tapp_save_failed'),
    )
    assert.match(full, /disk|空间|空き/)
    assert.notEqual(perm, currentCopy().errors.operationFailed)
  })

  it('maps leftover Tapp storage reports access and resources without unifying them', () => {
    const read = userFacingError(
      new ApiError('Failed to read storage', 500, 'storage_read_failed'),
    )
    const save = userFacingError(
      new ApiError('Failed to save storage', 500, 'storage_save_failed'),
    )
    const reportLoad = userFacingError('Failed to list agent reports')
    const reportSave = userFacingError(
      new ApiError('Failed to create report', 500, 'REPORT_CREATE_FAILED'),
    )
    const access = userFacingError(
      new ApiError(
        'Failed to verify Tapp access',
        500,
        'tapp_access_check_failed',
      ),
    )
    const missing = userFacingError('Failed to find Tapp')
    const shortcuts = userFacingError(
      new ApiError('Failed to load shortcuts', 500, 'SHORTCUT_DATABASE_ERROR'),
    )
    const credentials = userFacingError(
      new ApiError(
        'Failed to load Tapp credentials',
        500,
        'TAPP_CREDENTIAL_LOAD_FAILED',
      ),
    )
    assert.match(read, /存储|storage|保存領域/)
    assert.match(save, /存储|storage|保存領域/)
    assert.match(read, /read storage/)
    assert.match(save, /save storage/)
    assert.notEqual(read, save)
    assert.notEqual(read, currentCopy().errors.database)
    assert.match(reportLoad, /报告|report|レポート/)
    assert.match(reportSave, /报告|report|レポート/)
    assert.notEqual(reportLoad, reportSave)
    assert.match(access, /访问|access|アクセス/)
    assert.match(missing, /找到|found|見つかり/)
    assert.notEqual(access, missing)
    assert.notEqual(access, currentCopy().errors.database)
    assert.equal(shortcuts, currentCopy().errors.database)
    assert.match(credentials, /credentials|资源|リソース/)
    assert.notEqual(shortcuts, credentials)
    assert.notEqual(reportLoad, currentCopy().errors.database)
  })

  it('maps leftover config, icon, and phantasiai loads without unifying them', () => {
    const write = userFacingError(
      new ApiError(
        'Failed to write configuration: not enough disk space · /data/site_public.env',
        500,
        'config_file_permission',
      ),
    )
    const read = userFacingError(
      new ApiError(
        'Failed to read configuration: storage is not writable · /data/site_public.env',
        500,
        'config_file_read_failed',
      ),
    )
    const icon = userFacingError(
      'Failed to write icon file: not enough disk space',
    )
    const article = userFacingError('Failed to load article')
    const source = userFacingError('Failed to load source')
    const articles = userFacingError('Failed to load articles')
    assert.match(write, /配置|configuration|設定/)
    assert.match(write, /disk|空间|空き/)
    assert.match(write, /site_public\.env/)
    assert.equal(/os error/.test(write), false)
    assert.match(read, /读取|read|読み込/)
    assert.match(read, /writable|权限|書き込め/)
    assert.notEqual(read, write)
    assert.match(icon, /图标|icon|アイコン/)
    assert.match(icon, /disk|空间|空き/)
    assert.notEqual(icon, write)
    assert.match(article, /文章|article|記事/)
    assert.match(source, /加载|load|読み込/)
    assert.match(articles, /articles|加载|load|読み込/)
    assert.notEqual(article, source)
    assert.notEqual(article, articles)
    assert.notEqual(article, currentCopy().errors.database)
    assert.notEqual(source, currentCopy().errors.database)
  })

  it('maps PSN leftovers and keeps status when useful', () => {
    const npsso = userFacingError(
      'PSN NPSSO exchange failed (status 401). Cookie may be expired.',
    )
    assert.equal(/error sending request|ca\.account\.sony/.test(npsso), false)
    assert.match(npsso, /NPSSO|过期|期限切れ/)
    assert.match(npsso, /401/)
    const token = userFacingError(
      'PSN token request failed: error sending request for url (https://ca.account.sony.com)',
    )
    assert.equal(/error sending request|sony\.com/.test(token), false)
    assert.notEqual(token, currentCopy().errors.operationFailed)
    assert.notEqual(npsso, token)
  })

  it('maps leftover playlist song youtube config media and schedule without unifying them', () => {
    const playlist = userFacingError(
      new ApiError('Failed to fetch playlist: timed out', 502, 'playlist_fetch_failed'),
    )
    const song = userFacingError(
      new ApiError('Failed to fetch song detail', 502, 'song_fetch_failed'),
    )
    const youtube = userFacingError(
      new ApiError('YouTube upstream failed', 502, 'youtube_upstream_failed'),
    )
    const configSave = userFacingError(
      new ApiError('Failed to save', 500, 'config_save_failed'),
    )
    const configInvalid = userFacingError(
      new ApiError('ai_vendor_sources must be an array', 400, 'config_invalid'),
    )
    assert.match(configInvalid, /ai_vendor_sources must be an array/)
    assert.notEqual(configInvalid, configSave)
    const configLoad = userFacingError('Failed to load config')
    const media = userFacingError(
      new ApiError('Invalid action', 400, 'media_action_invalid'),
    )
    const mode = userFacingError(
      new ApiError('Invalid mode', 400, 'media_mode_invalid'),
    )
    const schedule = userFacingError(
      new ApiError('Invalid schedule config', 400, 'schedule_invalid'),
    )
    const retry = userFacingError('The failed step will be retried')
    const skipped = userFacingError('The failed step was skipped')
    const confirm = userFacingError('Confirmation failed')
    const steering = userFacingError(
      new ApiError(
        'Failed to persist steering instruction: disk is full',
        503,
        'steering_unavailable',
      ),
    )
    assert.match(playlist, /歌单|playlist|プレイリスト/)
    assert.match(playlist, /timed out/)
    assert.match(song, /歌曲|song|曲/)
    assert.match(youtube, /YouTube/)
    assert.match(youtube, /502/)
    assert.match(configSave, /配置|settings|設定/)
    assert.match(configLoad, /配置|configuration|設定|load|加载|載入|読み込/)
    assert.notEqual(playlist, song)
    assert.notEqual(configSave, configLoad)
    assert.notEqual(media, mode)
    assert.match(media, /播放|playback|再生/)
    assert.match(schedule, /时间|schedule|スケジュール/)
    assert.notEqual(retry, skipped)
    assert.notEqual(confirm, retry)
    assert.notEqual(steering, currentCopy().errors.agentProcessingFailed)
    assert.match(steering, /disk is full/)
    assert.notEqual(playlist, currentCopy().errors.operationFailed)
    assert.notEqual(song, currentCopy().errors.operationFailed)
    assert.notEqual(youtube, currentCopy().errors.operationFailed)
    assert.notEqual(configSave, currentCopy().errors.operationFailed)
    assert.notEqual(media, currentCopy().errors.operationFailed)
    assert.notEqual(schedule, currentCopy().tapp.unknownError)
    assert.notEqual(schedule, currentCopy().errors.operationFailed)
  })

  it('maps leftover rig portrait and see-through without calling them generation failures', () => {
    const compile = userFacingError(
      'Rig compilation failed: missing field `layers` at line 1 column 12',
    )
    const stored = userFacingError('Stored rig is invalid')
    const atlas = userFacingError('Rig atlas exceeds 20 MB')
    const imported = userFacingError('Invalid rig import')
    const portrait = userFacingError('Portrait image exceeds 10 MB')
    const missing = userFacingError(
      'The current master portrait is not available',
    )
    const token = userFacingError(
      new ApiError(
        'Configure a Hugging Face API token before using See-through',
        428,
        'see_through_token_required',
      ),
    )
    const busy = userFacingError(
      new ApiError(
        'A See-through decomposition is already running',
        409,
        'see_through_busy',
      ),
    )
    assert.equal(/missing field|layers/.test(compile), false)
    assert.notEqual(compile, currentCopy().merope.visualFailed)
    assert.notEqual(stored, compile)
    assert.notEqual(atlas, imported)
    assert.notEqual(portrait, missing)
    assert.notEqual(portrait, currentCopy().merope.visualFailed)
    assert.match(atlas, /20 MB/)
    assert.match(portrait, /10 MB/)
    assert.match(token, /token|令牌|トークン/i)
    assert.notEqual(token, busy)
    assert.notEqual(missing, currentCopy().merope.visualFailed)
  })

  it('maps leftover merope portrait upload and tripo without unifying them', () => {
    const upload = userFacingError('Could not upload portrait')
    const tripo = userFacingError('Tripo request failed')
    assert.equal(upload, currentCopy().merope.portraitUploadFailed)
    assert.match(tripo, /三维|3D|モデル/i)
    assert.equal(/Tripo request failed/i.test(tripo), false)
    assert.notEqual(upload, tripo)
  })

  it('maps leftover store download config phantasi leftovers without unifying them', () => {
    const asset = userFacingError(
      new ApiError(
        'Failed to fetch asset assets/icon.png: remote returned 404 Not Found',
        502,
        'store_asset_fetch_failed',
      ),
    )
    const config = userFacingError('Failed to save configuration')
    assert.match(asset, /assets\/icon\.png/)
    assert.equal(/Failed to fetch asset/i.test(asset), false)
    assert.equal(/Failed to save configuration/i.test(config), false)
  })

  it('maps leftover library and visitor leftovers without calling them empty', () => {
    const library = userFacingError('No library data available')
    const visitor = userFacingError('visitor card unavailable')
    assert.notEqual(library, currentCopy().library.emptyLibrary)
    assert.match(library, /资料库|library|ライブラリ/i)
    assert.equal(/visitor card unavailable/i.test(visitor), false)
  })

  it('maps tapp runtime refusals by their code', () => {
    const missing = userFacingError(
      codedError('Tapp weather-clock is not installed', 'tapp_not_installed'),
    )
    const already = userFacingError(
      codedError('Tapp weather-clock is already installed', 'tapp_already_installed'),
    )
    const reauth = userFacingError(
      codedError(
        'Tapp weather-clock requires permission reauthorization',
        'tapp_reauthorization_required',
      ),
    )
    assert.equal(/is not installed/i.test(missing), false)
    assert.equal(/Tapp weather-clock is already installed/i.test(already), false)
    assert.notEqual(already, missing)
    assert.notEqual(already, currentCopy().tapp.installFailed)
    assert.equal(/reauthorization/i.test(reauth), false)
    assert.notEqual(missing, reauth)
  })

  it('maps setup window closed and secret mismatch without English labels', () => {
    const closed = userFacingError(
      new ApiError('Setup window closed', 401, 'setup_window_closed'),
    )
    assert.equal(/Setup window closed/i.test(closed), false)
    assert.equal(closed.includes('安装向导已关闭'), false)
    const leftoverClosed = userFacingError(
      '安装向导已关闭。认领之后请先修库，不要再用 setup 改宿主配置。',
    )
    assert.equal(leftoverClosed.includes('安装向导已关闭'), false)
    const mismatch = userFacingError(
      new ApiError('Setup secret required', 401, 'setup_secret_mismatch'),
    )
    assert.equal(/Setup secret required/i.test(mismatch), false)
    assert.equal(mismatch.includes('安装暗号不对'), false)
    const leftoverSecret = userFacingError(
      '安装暗号不对。请从服务器 .env 的 MYRIAD_SETUP_SECRET 复制后再试。',
    )
    assert.equal(leftoverSecret.includes('安装暗号不对'), false)
    assert.notEqual(closed, mismatch)
  })

  it('maps leftover Chinese bangumi credentials and agent submitted copy', () => {
    const bangumi = userFacingError('username 或 access_token 至少需要提供一个')
    assert.equal(bangumi.includes('至少需要提供'), false)
    const submitted = userFacingError('任务已提交，等待执行')
    assert.equal(submitted.includes('任务已提交'), false)
    const submittedEn = userFacingError('Task submitted, waiting to run')
    assert.equal(submittedEn, currentCopy().errors.agentSubmitted)
    const planFailed = userFacingError(
      'I understood the request, but planning failed: timeout. Please describe what you want more specifically.',
    )
    assert.equal(
      planFailed,
      fill(currentCopy().errors.agentPlanningFailed, { detail: 'timeout' }),
    )
  })

  it('maps Discord app-missing and leftover music-control chrome', () => {
    const discord = userFacingError(
      new ApiError(
        'Add and enable a Discord app in OAuth login first',
        400,
        'discord_app_not_configured',
      ),
    )
    assert.equal(discord, currentCopy().config.discordOAuthAppMissing)
    const leftoverDiscord = userFacingError(
      '请先在「OAuth 登录」中添加并启用 Discord 应用（client_id / client_secret）。数据授权会复用同一 Application。',
    )
    assert.equal(leftoverDiscord.includes('OAuth 登录'), false)
    const next = userFacingError('切换到下一首')
    assert.equal(next.includes('切换到下一首'), false)
    assert.equal(userFacingError('Playing music'), currentCopy().music.playingNow)
    assert.equal(userFacingError('Muted'), currentCopy().music.muted)
    assert.equal(
      userFacingError('现在没在放歌。'),
      currentCopy().music.noPlaying,
    )
    assert.equal(
      userFacingError('Waited more than 30 seconds'),
      fill(currentCopy().errors.agentQueueTimeout, { sec: 30 }),
    )
  })

  it('maps leftover Chinese agent step chrome and English capability labels', () => {
    assert.equal(userFacingError('AI 对话'), currentCopy().errors.agentChat)
    assert.equal(userFacingError('Chatting'), currentCopy().errors.agentChat)
    assert.equal(userFacingError('好了，都处理完啦~'), currentCopy().errors.agentAllDone)
    assert.equal(
      userFacingError('This will run 打开窗口'),
      fill(currentCopy().errors.willExecute, { name: '打开窗口' }),
    )
    assert.equal(userFacingError('数据读取'), currentCopy().errors.capCategoryData)
    assert.equal(userFacingError('Discovering feeds'), currentCopy().errors.agentDiscoverFeeds)
    assert.equal(
      userFacingError('获取 B 站数据').includes('B 站'),
      false,
    )
    assert.equal(
      userFacingError('即将添加新的 RSS/Atom 订阅源'),
      currentCopy().errors.confirmAddFeed,
    )
    assert.equal(userFacingError('组件列表'), currentCopy().errors.tappWidgets)
    assert.equal(
      userFacingError('未命名报告'),
      currentCopy().errors.unnamedReport,
    )
    assert.equal(userFacingError('未知标题'), currentCopy().errors.unknownTitle)
    assert.equal(
      userFacingError('未知艺术家'),
      currentCopy().library.unknownArtist,
    )
    assert.equal(
      userFacingError('Auto-refresh steam data'),
      fill(currentCopy().errors.autoRefreshNamed, { name: 'steam' }),
    )
    assert.equal(
      userFacingError('Scheduled task: 备份'),
      fill(currentCopy().errors.noticeHeartbeatTask, { name: '备份' }),
    )
    assert.equal(
      userFacingError('未命名内容'),
      currentCopy().errors.untitledContent,
    )
    assert.equal(
      userFacingError('智能阅读列表'),
      currentCopy().phantasi.smartReadingList,
    )
    assert.equal(userFacingError('订阅源'), currentCopy().phantasi.boardFeeds)
    assert.equal(userFacingError('已收藏'), currentCopy().errors.phantasiMarkStarred)
    assert.equal(
      userFacingError('网络搜索 - 科技'),
      fill(currentCopy().errors.webSearchNamed, { name: '科技' }),
    )
    assert.equal(
      userFacingError("This will call tool 'read' on MCP server 'files'"),
      fill(currentCopy().errors.confirmMcpTool, {
        server: 'files',
        tool: 'read',
      }),
    )
    assert.equal(
      userFacingError('Loaded 3 tools'),
      fill(currentCopy().errors.noticeMcpToolsLoaded, { n: 3 }),
    )
    assert.equal(
      userFacingError('维护重试成功'),
      currentCopy().errors.noticeMcpMaintenanceRetry,
    )
    assert.equal(
      userFacingError('Auto-restart succeeded'),
      currentCopy().errors.noticeMcpAutoRestart,
    )
    assert.equal(
      userFacingError('状态监控超时，请在系统更新面板确认任务结果'),
      currentCopy().errors.noticeUpdaterWatchTimeout,
    )
    assert.equal(
      userFacingError('未知用户'),
      currentCopy().userModal.unknownUser,
    )
    assert.equal(userFacingError('游客'), currentCopy().errors.guestLabel)
    assert.equal(
      userFacingError('用户#7'),
      fill(currentCopy().errors.userNumber, { id: '7' }),
    )
    assert.equal(
      userFacingError('网易云音乐用户'),
      currentCopy().errors.neteaseMusicUser,
    )
    assert.equal(userFacingError('Steam 玩家'), currentCopy().errors.steamPlayer)
    assert.equal(
      userFacingError('等待 Tapp 完成交互'),
      currentCopy().errors.waitTappInteraction,
    )
    assert.equal(userFacingError('动态技能'), currentCopy().errors.capDynamicSkills)
    assert.equal(userFacingError('未分类'), currentCopy().phantasi.uncategorized)
    assert.equal(
      userFacingError('最新文章'),
      currentCopy().phantasi.latestArticles,
    )
    assert.equal(
      userFacingError('Waiting for a reply timed out (2 hours); the task was cancelled'),
      fill(currentCopy().errors.waitInputTimeoutHours, { hours: 2 }),
    )
    assert.equal(
      userFacingError('API 速率限制，等待后重试'),
      currentCopy().errors.rateLimited,
    )
    assert.equal(
      userFacingError('内容策略违规，尝试清理敏感内容后重试'),
      currentCopy().errors.contentPolicyRetry,
    )
    assert.equal(
      userFacingError('标题不能为空'),
      currentCopy().phantasi.noteTitleRequired,
    )
    assert.equal(
      userFacingError('A title is required'),
      currentCopy().phantasi.noteTitleRequired,
    )
    assert.equal(
      userFacingError('Note draft was updated elsewhere'),
      currentCopy().phantasi.noteRevisionConflict,
    )
    assert.equal(
      userFacingError('That time has already passed'),
      currentCopy().phantasi.noteSchedulePast,
    )
    assert.equal(
      userFacingError('A schedule time is required'),
      currentCopy().phantasi.noteScheduleNeedTime,
    )
    assert.equal(
      userFacingError('Titles can be at most 200 characters (this one is 201)'),
      fill(currentCopy().phantasi.noteTitleTooLong, { max: '200', chars: '201' }),
    )
    assert.equal(
      userFacingError('Notes can be at most 200000 characters (this one is 200001)'),
      fill(currentCopy().phantasi.noteBodyTooLong, {
        max: '200000',
        chars: '200001',
      }),
    )
    assert.equal(
      userFacingError("I'm in a very low mood and don't want to take on anything new"),
      currentCopy().errors.agentRefuseLowMood,
    )
    assert.equal(userFacingError('重试'), currentCopy().errors.retryStep)
    assert.equal(
      userFacingError('取消整个任务'),
      currentCopy().errors.cancelTaskDesc,
    )
    assert.equal(
      userFacingError('联网搜索结果'),
      currentCopy().errors.webSearchResult,
    )
    assert.equal(
      userFacingError('请尝试其他关键词'),
      currentCopy().errors.tryOtherKeyword,
    )
    assert.equal(
      userFacingError('即将向外部 URL 发起 HTTP 请求'),
      currentCopy().errors.confirmHttpFetch,
    )
    assert.equal(
      userFacingError('This will interact with a page element'),
      currentCopy().errors.confirmPageInteract,
    )
  })

  it('maps leftover public config and comment reply leftovers without unifying them', () => {
    const pub = userFacingError(new TypeError('Public config does not contain platforms'))
    const replies = userFacingError('Failed to load comment replies')
    const comments = userFacingError('Failed to load comments')
    assert.match(pub, /配置|config|設定/i)
    assert.equal(/Public config does not contain/i.test(pub), false)
    assert.notEqual(replies, comments)
    assert.equal(/Failed to load comment replies/i.test(replies), false)
    assert.notEqual(pub, currentCopy().errors.operationFailed)
  })
})

const RULE_BUDGET = 61

describe('userFacingError is driven by codes', () => {
  before(async () => {
    await Promise.all([
      loadShellNamespace('tapp', 'en-US'),
      loadShellNamespace('phantasi', 'en-US'),
      loadShellNamespace('merope', 'en-US'),
    ])
  })

  // Answers to remote federation servers, never shown in this UI.
  const SERVER_TO_SERVER = new Set([
    'gone',
    'activity_not_ready',
    'inbox_failed',
    // Inbox: HTTP signature / Digest / Date gate before parsing.
    'http_signature_missing',
    'http_signature_invalid',
    // Inbox: body buffering and activity shape.
    'inbox_body_unreadable',
    'inbox_busy',
    'activity_invalid',
    'inbox_preflight_incomplete',
    'inbox_ownership_mismatch',
    'inbox_trust_rejected',
    // Inbound FileChunk from a peer.
    'transfer_chunk_invalid',
    'transfer_chunk_gone',
    // Public actor document and /.well-known/webfinger served to peers.
    'actor_unavailable',
    'webfinger_resource_invalid',
    'webfinger_user_not_found',
  ])

  it('gives every code the backend infers its own copy, whatever the text says', () => {
    const codes = new Set(Object.values(errorCodeSpec.labels))
    const unrelated = 'text that matches no rule'
    const bare = [...codes].filter((code) => {
      if (SERVER_TO_SERVER.has(code)) return false
      const copy = userFacingError(new ApiError(unrelated, 400, code), 'FALLBACK')
      return copy === 'FALLBACK' || copy === unrelated
    })
    assert.deepEqual(bare, [])
  })

  it('keeps only short English detail next to translated table copy', () => {
    const prose = 'Local login needs OAuth: user has no linked OAuth identity'
    assert.equal(withoutForeignProse('请先绑定第三方登录', prose), 'Local login needs OAuth')
    assert.equal(withoutForeignProse('Link a sign-in method first', prose), prose)
    const token = 'Unsupported attachment MIME: image/avif'
    assert.equal(withoutForeignProse('不支持的附件类型', token), token)
    assert.equal(withoutForeignProse('不支持的附件类型', 'No detail here'), 'No detail here')
  })

  it('maps codes through the byCode table', () => {
    const copy = currentCopy().errors.byCode.source_refresh_in_progress
    assert.equal(
      userFacingError(new ApiError('Source is already being refreshed', 409, 'source_refresh_in_progress')),
      copy,
    )
  })

  it('only lists codes the backend or the leftover table can produce', () => {
    const known = new Set<string>([
      ...Object.values(errorCodeSpec.labels),
      ...Object.values(errorCodeSpec.leftovers),
      ...Object.values(errorCodeSpec.prefixes),
      ...Object.values(errorCodeSpec.aliases),
      ...errorCodeSpec.explicit,
    ])
    const unknown = Object.keys(currentCopy().errors.byCode).filter(code => !known.has(code))
    assert.deepEqual(unknown, [])
  })

  it('does not grow the text-matching compatibility layer', () => {
    // Regexes over backend prose are a shrinking fallback: new faults get a
    // code and a copy entry instead. Lower this bound as rules are removed.
    const source = readFileSync(new URL('./userFacingError.ts', import.meta.url), 'utf8')
    const rules = source.match(/\/[gimsuy]*\.test\(/g)?.length ?? 0
    assert.ok(rules <= RULE_BUDGET, `${rules} text rules; budget ${RULE_BUDGET}`)
  })
})
