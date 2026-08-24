import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { ApiError } from '../services/api.ts'
import {
  httpStatusMessage,
  isUselessErrorText,
  userFacingError,
} from './userFacingError.ts'

describe('userFacingError', () => {
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

  it('maps brew feed parse to a name hint', () => {
    const err = new ApiError(
      'Failed to parse feed. Please provide a name.',
      400,
      'feed_parse_failed',
    )
    const text = userFacingError(err)
    assert.equal(/Failed to parse feed/i.test(text), false)
  })

  it('maps brew AI parse failures', () => {
    const err = new ApiError('Failed to parse AI response', 500, 'ai_response_invalid')
    const text = userFacingError(err)
    assert.equal(/Failed to parse/i.test(text), false)
    assert.match(text, /AI|解析|解析できませんでした/)
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
    assert.equal(/Could not load/i.test(text), false)
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
    assert.equal(/database connection/i.test(text), false)
    assert.equal(/处理失败/.test(text), false)
    const leftover = userFacingError('抱歉，这次没能完成你的请求：boom')
    assert.equal(/boom/.test(leftover), false)
    const interrupted = userFacingError('任务因服务重启而中断，请重新提交')
    assert.equal(/服务重启/.test(interrupted), false)
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
})
