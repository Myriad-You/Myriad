import assert from 'node:assert/strict'
import { before, describe, it } from 'node:test'
import { loadShellNamespace } from '../i18n/loadLocale.ts'
import { currentCopy, formatCurrent } from '../i18n/localeCopy.ts'
import { ApiError } from '../services/api.ts'
import {
  httpStatusMessage,
  isUselessErrorText,
  userFacingError,
} from './userFacingError.ts'

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
    const queued = userFacingError('排队中（前方约 3 个任务）…')
    assert.equal(/排队中（前方约 3 个任务）/.test(queued), false)
    assert.match(queued, /3/)
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
    assert.match(text, /site face|形象|顔/)
  })

  it('maps leftover speech Chinese and dumps', () => {
    assert.equal(
      /无效的音频数据|Invalid Audio/i.test(
        userFacingError('无效的音频数据: Invalid byte 64'),
      ),
      false,
    )
    assert.equal(
      /语音服务未配置/.test(
        userFacingError('语音服务未配置，请在设置中配置腾讯云密钥'),
      ),
      false,
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
      'WebFinger lookup failed for https://x.example/.well-known/webfinger: error sending request for url',
    )
    assert.equal(/error sending request/i.test(text), false)
    assert.equal(/well-known/i.test(text), false)
  })

  it('maps leftover Steam Chinese without leaking internals', () => {
    const text = userFacingError('获取 Steam 在线状态失败: error sending request for url (http://x)')
    assert.equal(/error sending request/i.test(text), false)
    assert.equal(/获取 Steam/.test(text), false)
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

  it('maps leftover channel status dumps', () => {
    const text = userFacingError(
      'Channel is pending, cannot send messages (must be accepted first)',
    )
    assert.equal(/pending/.test(text), false)
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

  it('maps leftover high-risk step confirmation dumps', () => {
    const text = userFacingError(
      "步骤 'dyn-1' 涉及未经确认的高风险操作（system.export，风险 High），动态生成的子步骤不允许自动执行",
    )
    assert.equal(/dyn-1|system\.export|High/.test(text), false)
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

  it('maps leftover agent permission Chinese without leaking setting paths', () => {
    const text = userFacingError(
      'Agent 未对普通用户开放 AI 对话（请在「Tapp 权限管理」中下放 ai:chat 或选用助手预设）',
    )
    assert.equal(/ai:chat|Tapp 权限管理/.test(text), false)
  })

  it('maps leftover context.reference Chinese without leaking recipe internals', () => {
    const text = userFacingError(
      'context.reference 不应被直接调用。请使用 xxxFrom 参数引用上游步骤的输出。',
    )
    assert.equal(/xxxFrom|context\.reference/.test(text), false)
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

  it('maps leftover database-not-connected Chinese', () => {
    const text = userFacingError('数据库未连接')
    assert.equal(text.includes('数据库未连接'), false)
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

  it('maps leftover scheduler store steps without SQL and keeps the step', () => {
    const register = userFacingError(
      'Failed to register scheduled task: relation "tapp_scheduled_tasks" does not exist',
    )
    const list = userFacingError('Failed to list scheduled tasks')
    const leftover = userFacingError(
      'Query failed: relation "tapp_scheduled_tasks" does not exist',
    )
    assert.equal(/tapp_scheduled_tasks|does not exist/.test(register), false)
    assert.equal(/tapp_scheduled_tasks|does not exist/.test(leftover), false)
    assert.match(register, /register/)
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
    const load = userFacingError(
      'Failed to load persona: relation "merope_persona" does not exist',
    )
    const save = userFacingError('Failed to save persona')
    const remove = userFacingError('Failed to delete persona')
    const addresseeLoad = userFacingError('Failed to load addressee')
    const quiet = userFacingError('Failed to save quiet-hours')
    const face = userFacingError('Failed to load avatar')
    assert.equal(/merope_persona|does not exist/.test(load), false)
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

  it('maps leftover e2e serialize dumps', () => {
    const text = userFacingError('payload serialize: EOF while parsing a value at line 1')
    assert.equal(/EOF|line 1/.test(text), false)
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

  it('maps leftover report persist dumps', () => {
    const text = userFacingError('insert report for steam: relation "platform_reports" does not exist')
    assert.equal(/platform_reports|does not exist/.test(text), false)
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

  it('maps leftover OAuth and Discord token dumps', () => {
    const github = userFacingError('GitHub token exchange failed: RequestTokenError { .. }')
    assert.equal(/RequestTokenError/.test(github), false)
    const oidc = userFacingError('OIDC token endpoint returned 400: {"error":"invalid_grant"}')
    assert.equal(/invalid_grant/.test(oidc), false)
    const discord = userFacingError('token endpoint 401 — {"error":"invalid_client"}')
    assert.equal(/invalid_client/.test(discord), false)
  })

  it('maps leftover game-presence upstream dumps', () => {
    const text = userFacingError('Upstream HTTP 502: <html>bad gateway</html>')
    assert.equal(/bad gateway|<html>/.test(text), false)
  })

  it('maps leftover feed URL and Gemini JSON dumps', () => {
    const url = userFacingError('Unsafe or invalid URL: Invalid URL')
    assert.equal(/Invalid URL/.test(url) && url === 'Unsafe or invalid URL: Invalid URL', false)
    const gemini = userFacingError('invalid Gemini JSON: expected value at line 1')
    assert.equal(/expected value|line 1/.test(gemini), false)
    const key = userFacingError('Invalid remote E2E public key: invalid length')
    assert.equal(/invalid length/.test(key), false)
  })

  it('maps leftover Tripo and scheduler-connection dumps', () => {
    const tripo = userFacingError(
      'Tripo API returned HTTP 502: {"error":"upstream exploded"}',
    )
    assert.equal(/upstream exploded/.test(tripo), false)
    const glb = userFacingError('Invalid GLB JSON: expected value at line 1')
    assert.equal(/expected value|line 1/.test(glb), false)
    const sched = userFacingError(
      'Failed to register scheduler connection: db closed',
    )
    assert.match(sched, /db closed/)
    assert.equal(/Failed to register scheduler/.test(sched), false)
    const enqueue = userFacingError(
      'Failed to enqueue scheduler task: relation "tapp_registry" does not exist',
    )
    assert.equal(/tapp_registry/.test(enqueue), false)
    const schema = userFacingError(
      'Model response failed output schema validation: missing field `title`',
    )
    assert.equal(/missing field|title/.test(schema), false)
  })

  it('maps leftover avatar, scheduler, and updater dumps', () => {
    const avatar = userFacingError('Failed to load avatar row: db closed')
    assert.match(avatar, /db closed/)
    assert.equal(/Failed to load avatar/.test(avatar), false)
    const scheduled = userFacingError(
      'Scheduled Tapp is no longer accessible: Record not found',
    )
    assert.match(scheduled, /Record not found/)
    assert.equal(/Scheduled Tapp is no longer/.test(scheduled), false)
    const retries = userFacingError(
      'All 3 retries failed. Last error: relation "tapp_scheduled_tasks" does not exist',
    )
    assert.equal(/tapp_scheduled_tasks|does not exist/.test(retries), false)
    const updater = userFacingError('decode json failed: expected value at line 1')
    assert.equal(/expected value|line 1/.test(updater), false)
  })

  it('maps leftover preset fetch save delete away from updater and SQL', () => {
    const favorites = userFacingError(
      'Failed to fetch favorites: relation "agent_task_presets" does not exist',
    )
    const history = userFacingError('Failed to fetch history')
    const save = userFacingError('Failed to create preset')
    const remove = userFacingError('Failed to delete preset')
    const leftoverCode = userFacingError(
      new ApiError('Failed to update preset', 500, 'preset_update_failed'),
    )
    const updater = userFacingError('decode json failed: expected value at line 1')
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
    const lookup = userFacingError(
      'Failed to look up account: relation "users" does not exist',
    )
    const current = userFacingError('Failed to load current user')
    const username = userFacingError('Failed to check username')
    const change = userFacingError('Failed to change password')
    const set = userFacingError('Failed to set password')
    const local = userFacingError('Failed to update local login')
    const updater = userFacingError('decode json failed: expected value at line 1')
    const register = userFacingError(
      new ApiError('Failed to create account', 500, 'account_create_failed'),
    )
    assert.equal(/users"|does not exist/.test(lookup), false)
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
    const list = userFacingError(
      'Failed to list users: relation "users" does not exist',
    )
    const identities = userFacingError('Failed to list user identities')
    const update = userFacingError('Failed to update user')
    const unlink = userFacingError('Failed to unlink identity')
    const remove = userFacingError('Failed to delete user')
    const commit = userFacingError('Failed to commit user delete')
    assert.equal(/users"|does not exist/.test(list), false)
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
    const confirm = userFacingError(
      'Failed to persist confirmation: db connection closed',
    )
    assert.match(confirm, /db connection closed/)
    assert.equal(/Failed to persist confirmation/.test(confirm), false)
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
    const list = userFacingError(
      'Failed to list sessions: relation "agent_sessions" does not exist',
    )
    const create = userFacingError('Failed to create session')
    const archive = userFacingError('Failed to archive session')
    const userMsg = userFacingError('Failed to save user message')
    const assistantMsg = userFacingError('Failed to save assistant message')
    const signIn = userFacingError(new ApiError('session failed', 500, 'session_failed'))
    assert.equal(/agent_sessions|does not exist/.test(list), false)
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

  it('maps leftover skill file and cooldown errors without OS dumps', () => {
    const write = userFacingError(
      'Failed to write skill file: Permission denied (os error 13)',
    )
    const missing = userFacingError('Skill file missing: /data/skills/_auto_demo.md')
    const wait = userFacingError(
      'Skill improvement on cooldown (3600 seconds remaining)',
    )
    const invalid = userFacingError('Invalid skill file format')
    assert.equal(/os error 13|Permission denied \(os/.test(write), false)
    assert.match(write, /技能|skill|スキル/i)
    assert.match(missing, /_auto_demo\.md/)
    assert.match(wait, /3600/)
    assert.notEqual(write, wait)
    assert.notEqual(invalid, write)
    assert.notEqual(write, currentCopy().errors.operationFailed)
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
      "MCP server timeout (30s) for method 'tools/call'",
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
    const mcp = userFacingError(
      'serialize mcp config: key must be a string at line 1 column 2',
    )
    assert.equal(/key must be a string|column 2/.test(mcp), false)
    const save = userFacingError(
      new ApiError('Failed to save MCP config', 503, 'mcp_config_save_failed'),
    )
    assert.equal(/mcp_config_save_failed/.test(save), false)
    const wait = userFacingError(
      'persist Tapp interaction wait state failed: relation "tapp_registry" does not exist',
    )
    assert.equal(/tapp_registry|does not exist/.test(wait), false)
    const gen = userFacingError('Tapp generation failed: Gemini API error 400: boom')
    assert.equal(/Tapp generation failed/.test(gen), false)
    assert.match(gen, /Gemini API error 400/)
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
    const disk = userFacingError('Failed to persist confirmation: disk is full')
    assert.match(disk, /disk is full/)
    assert.equal(/Failed to persist confirmation/.test(disk), false)
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
    assert.match(identity, /找不到|not found|見つかり/)
  })

  it('maps avatar source leftovers away from the site-face copy', () => {
    const save = userFacingError('Failed to save avatar source')
    const face = userFacingError('Failed to load avatar')
    assert.match(save, /头像来源|avatar source|アバターの取得元/)
    assert.notEqual(save, face)
    assert.notEqual(save, currentCopy().errors.operationFailed)
  })

  it('maps platform auto-refresh leftovers and keeps the platform', () => {
    const steam = userFacingError(
      'Failed to update steam core task: relation "tapp_scheduled_tasks" does not exist',
    )
    assert.equal(/tapp_scheduled_tasks|does not exist/.test(steam), false)
    assert.match(steam, /Steam|自动刷新/)
    const generic = userFacingError(
      new ApiError(
        'Failed to update platform auto-refresh',
        500,
        'platform_refresh_reconcile_failed',
      ),
    )
    assert.notEqual(generic, currentCopy().errors.operationFailed)
    assert.notEqual(steam, currentCopy().errors.operationFailed)
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
      'Failed to create Tapp staging directory: storage is not writable. Check data volume ownership/permissions.',
    )
    assert.equal(/os error|Permission denied \(os/.test(perm), false)
    assert.match(perm, /writable|权限|書き込め/)
    const full = userFacingError(
      'Failed to activate staged Tapp: not enough disk space.',
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
    const reportLoad = userFacingError('Failed to load report')
    const reportSave = userFacingError('Failed to create report')
    const access = userFacingError(
      new ApiError(
        'Failed to verify Tapp access',
        500,
        'tapp_access_check_failed',
      ),
    )
    const missing = userFacingError('Failed to find Tapp')
    const shortcuts = userFacingError('Failed to load shortcuts')
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
    assert.match(shortcuts, /shortcuts|资源|リソース/)
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

  it('maps leftover merope load see-through tripo and playback without unifying them', () => {
    const face = userFacingError(new Error('Could not load site face (HTTP 502)'))
    const seeThrough = userFacingError('Could not load See-through status')
    const tokenSave = userFacingError('Could not save Hugging Face token')
    const decompose = userFacingError('See-through decomposition failed')
    const generate = userFacingError('Could not generate site portrait')
    const upload = userFacingError('Could not upload portrait')
    const preview = userFacingError('Could not preview persona rig')
    const commit = userFacingError('Could not commit persona rig')
    const tripo = userFacingError('Could not load Tripo status')
    const webgl = userFacingError('WebGL2 is required for Anime2.5DRig playback')
    const missing = userFacingError('Anime2.5DRig playback missing mouth-open')
    const shader = userFacingError('Anime2.5DRig shader compile failed')
    assert.match(face, /502/)
    assert.notEqual(face, 'Could not load site face (HTTP 502)')
    assert.notEqual(face, seeThrough)
    assert.notEqual(tokenSave, seeThrough)
    assert.notEqual(decompose, generate)
    assert.notEqual(upload, generate)
    assert.notEqual(preview, commit)
    assert.match(tripo, /三维|3D|モデル/i)
    assert.equal(/Could not load Tripo/i.test(tripo), false)
    assert.match(webgl, /WebGL2/)
    assert.match(missing, /mouth-open/)
    assert.equal(/shader compile/.test(shader), false)
    assert.notEqual(webgl, shader)
    assert.notEqual(generate, currentCopy().merope.loadFailed)
  })

  it('maps leftover store download config phantasi leftovers without unifying them', () => {
    const download = userFacingError(
      'Failed to download manifest (apps/foo/manifest.json): HTTP 502',
    )
    const asset = userFacingError('Failed to fetch asset assets/icon.png: HTTP 404')
    const mismatch = userFacingError(
      'Store package version mismatch: catalog lists 1.0.0 but manifest.json is 1.0.1. Refresh the store and retry.',
    )
    const config = userFacingError('Failed to save configuration')
    const reload = userFacingError('Failed to reload configuration')
    assert.match(download, /manifest/)
    assert.match(download, /502/)
    assert.equal(/Failed to download/i.test(download), false)
    assert.match(asset, /assets\/icon\.png/)
    assert.match(asset, /404/)
    assert.match(mismatch, /1\.0\.0/)
    assert.match(mismatch, /1\.0\.1/)
    assert.equal(/manifest\.json is/i.test(mismatch), false)
    assert.notEqual(config, reload)
    assert.equal(/Failed to save configuration/i.test(config), false)
    assert.equal(/Failed to reload/i.test(reload), false)
  })

  it('maps leftover library analytics visitor and invite leftovers without calling them empty', () => {
    const library = userFacingError('No library data available')
    const analytics = userFacingError('Unable to load analytics')
    const visitor = userFacingError('visitor card unavailable')
    const invite = userFacingError('Missing room_id')
    const stream = userFacingError('Runtime event stream failed (502)')
    assert.notEqual(library, currentCopy().library.emptyLibrary)
    assert.match(library, /资料库|library|ライブラリ/i)
    assert.match(analytics, /统计|analytics|統計/i)
    assert.equal(/Unable to load analytics/i.test(analytics), false)
    assert.equal(/visitor card unavailable/i.test(visitor), false)
    assert.equal(/room_id/.test(invite), false)
    assert.match(stream, /502|流|stream|ストリーム/i)
    assert.equal(/Runtime event stream failed/i.test(stream), false)
  })

  it('maps leftover dashboard scheme and tapp runtime leftovers without unifying them', () => {
    const layout = userFacingError('Failed to save dashboard layout: HTTP 502')
    const title = userFacingError('Failed to save dashboard title: HTTP 403')
    const platforms = userFacingError(
      'Failed to save custom platforms: HTTP 500',
    )
    const scheme = userFacingError('Failed to save window schemes: HTTP 502')
    const missing = userFacingError('Tapp weather-clock is not installed')
    const already = userFacingError('Tapp weather-clock is already installed')
    const reauth = userFacingError(
      'Tapp weather-clock requires permission reauthorization',
    )
    const list = userFacingError('Failed to load site Tapp catalog')
    assert.match(layout, /布局|layout|レイアウト/i)
    assert.match(layout, /502/)
    assert.equal(/Failed to save dashboard layout/i.test(layout), false)
    assert.notEqual(layout, title)
    assert.notEqual(title, platforms)
    assert.match(scheme, /方案|scheme|スキーム/i)
    assert.match(scheme, /502/)
    assert.equal(/Failed to save window schemes/i.test(scheme), false)
    assert.equal(/is not installed/i.test(missing), false)
    assert.equal(/Tapp weather-clock is already installed/i.test(already), false)
    assert.notEqual(already, missing)
    assert.notEqual(already, currentCopy().tapp.installFailed)
    assert.equal(/reauthorization/i.test(reauth), false)
    assert.notEqual(missing, reauth)
    assert.match(list, /列表|list|一覧/i)
    assert.equal(/site Tapp catalog/i.test(list), false)
    assert.notEqual(layout, currentCopy().errors.operationFailed)
    assert.notEqual(scheme, currentCopy().errors.operationFailed)
  })

  it('maps leftover control panel title style and widget theme leftovers without unifying them', () => {
    const panel = userFacingError('Failed to save control panel: HTTP 502')
    const style = userFacingError('Failed to save title style: HTTP 403')
    const theme = userFacingError('Failed to save widget theme: HTTP 500')
    assert.match(panel, /控制面板|control panel|コントロールパネル/i)
    assert.match(panel, /502/)
    assert.equal(/Failed to save control panel/i.test(panel), false)
    assert.notEqual(panel, style)
    assert.notEqual(style, theme)
    assert.match(style, /标题|title style|タイトル/i)
    assert.match(theme, /外观|appearance|見た目|卡片|card/i)
    assert.notEqual(panel, currentCopy().errors.operationFailed)
  })

  it('maps leftover AI usage platform preview and password leftovers without unifying them', () => {
    const usage = userFacingError('Unable to load AI usage (502)')
    const preview = userFacingError('Unable to load platform data preview (403)')
    const status = userFacingError('Unable to load platform data status (500)')
    const cloud = userFacingError('Failed to save to cloud: HTTP 502')
    assert.match(usage, /AI|使用/i)
    assert.match(usage, /502/)
    assert.equal(/Unable to load AI usage/i.test(usage), false)
    assert.notEqual(preview, status)
    assert.equal(/Unable to load platform data preview/i.test(preview), false)
    assert.match(cloud, /方案|scheme|スキーム/i)
    assert.match(cloud, /502/)
    assert.equal(/Failed to save to cloud/i.test(cloud), false)
    assert.notEqual(usage, currentCopy().errors.operationFailed)
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
    const leftoverPlan = userFacingError(
      '我理解了你的请求，但生成执行计划时出现问题：timeout。请更具体地描述你想要什么。',
    )
    assert.equal(leftoverPlan.includes('我理解了你的请求'), false)
    assert.match(leftoverPlan, /timeout/)
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
      /系统繁忙/.test(
        userFacingError('系统繁忙，排队超过 30 秒仍未获得执行许可，请稍后重试'),
      ),
      false,
    )
  })

  it('maps leftover Chinese agent step chrome and English capability labels', () => {
    assert.equal(userFacingError('AI 对话'), currentCopy().errors.agentChat)
    assert.equal(userFacingError('Chatting'), currentCopy().errors.agentChat)
    assert.equal(userFacingError('好了，都处理完啦~'), currentCopy().errors.agentAllDone)
    assert.equal(
      userFacingError('此操作将执行 打开窗口'),
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
    assert.equal(
      userFacingError('正在加载网易云歌单...'),
      fill(currentCopy().errors.loadingNamedPlaylist, { name: 'NetEase' }),
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
      userFacingError('自动刷新 steam 数据'),
      fill(currentCopy().errors.autoRefreshNamed, { name: 'steam' }),
    )
    assert.equal(
      userFacingError('定时任务: 备份'),
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
      userFacingError("将调用外部 MCP 服务 'files' 的工具 'read'"),
      fill(currentCopy().errors.confirmMcpTool, {
        server: 'files',
        tool: 'read',
      }),
    )
    assert.equal(
      userFacingError('已加载 3 个工具'),
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
      userFacingError('任务等待用户输入超时（2小时），已自动取消'),
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
      userFacingError('标题最多 200 字，现在有 201 字'),
      fill(currentCopy().phantasi.noteTitleTooLong, { max: '200', chars: '201' }),
    )
    assert.equal(
      userFacingError('正文最多 200000 字，现在有 200001 字'),
      fill(currentCopy().phantasi.noteBodyTooLong, {
        max: '200000',
        chars: '200001',
      }),
    )
    assert.equal(
      userFacingError('我现在心情很低，不想接新的事情。我们先说说话吧。'),
      currentCopy().errors.agentRefuseLowMood,
    )
    assert.equal(
      userFacingError('我对这个请求的理解置信度较低（20%），可能会误解你的意图。能再详细描述一下你想要做什么吗？'),
      currentCopy().errors.agentNeedClarification,
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
      userFacingError('AI 已根据近期失败原因改写该自动技能。'),
      currentCopy().errors.noticeSkillImprovedBody,
    )
    assert.equal(
      userFacingError('请尝试其他关键词'),
      currentCopy().errors.tryOtherKeyword,
    )
    assert.equal(
      userFacingError(
        'page.content 读取 Tapp 页需要 context.tappId，或由前端提供 content 快照',
      ),
      currentCopy().errors.pageContentNeedsTapp,
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
    const pub = userFacingError('Failed to fetch public config: HTTP 502')
    const replies = userFacingError('Failed to load comment replies')
    const comments = userFacingError('Failed to load comments')
    assert.match(pub, /配置|config|設定/i)
    assert.match(pub, /502/)
    assert.equal(/Failed to fetch public config/i.test(pub), false)
    assert.notEqual(replies, comments)
    assert.equal(/Failed to load comment replies/i.test(replies), false)
    assert.notEqual(pub, currentCopy().errors.operationFailed)
  })
})
