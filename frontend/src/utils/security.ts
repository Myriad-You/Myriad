/**
 * Content Security Policy (CSP) 配置
 * 用于防止 XSS、点击劫持等攻击
 *
 * 注意: 某些安全响应头只能通过 HTTP 响应头设置，不能通过 meta 标签设置:
 * - X-Frame-Options (必须在后端设置)
 * - X-Content-Type-Options (必须在后端设置)
 * - X-XSS-Protection (必须在后端设置)
 * - Strict-Transport-Security (必须在后端设置)
 *
 * 请参考 docs/SECURITY_HEADERS.md 了解如何在后端配置这些响应头
 */

// 动态获取 API URL
function getApiUrl(): string {
  // 优先使用环境变量（去除首尾空格）
  const envUrl = (import.meta.env.PUBLIC_API_URL || '').trim()
  if (envUrl) {
    return envUrl
  }

  // 浏览器环境：使用当前域名（空字符串表示相对路径）
  if (typeof window !== 'undefined') {
    const origin = window.location.origin
    // 如果是标准端口（生产环境），返回空字符串使用相对路径
    if (window.location.port === '' || window.location.port === '80' || window.location.port === '443') {
      return ''
    }
    // 开发环境返回当前 origin
    return origin
  }

  // SSR/构建时默认值：空字符串（使用相对路径）
  return ''
}

// 动态生成 connect-src 列表
function getConnectSources(): string[] {
  const sources = ['\'self\'']

  const apiUrl = getApiUrl()

  // 添加 API URL（如果不是 'self'）
  if (apiUrl && apiUrl !== '') {
    try {
      const url = new URL(apiUrl)
      const apiOrigin = url.origin
      if (apiOrigin !== (typeof window !== 'undefined' ? window.location.origin : '')) {
        sources.push(apiOrigin)
      }
    }
    catch {
      // 如果解析失败，尝试直接添加
      if (apiUrl.startsWith('http')) {
        sources.push(apiUrl)
      }
    }
  }

  // 添加常见的本地开发地址（仅开发环境）
  if (typeof window !== 'undefined' && (
    window.location.hostname === 'localhost'
    || window.location.hostname === '127.0.0.1'
  )) {
    // 开发环境才添加 localhost 地址
    if (window.location.port !== '' && window.location.port !== '80' && window.location.port !== '443') {
      sources.push('http://localhost:3000')
      sources.push('http://127.0.0.1:3000')
    }
  }

  // 添加其他必需的外部服务
  sources.push('https://api.github.com')
  sources.push('https://image.pollinations.ai')

  // 去重
  return Array.from(new Set(sources))
}

export const CSP_DIRECTIVES = {
  // 默认源：只允许同源内容
  'default-src': ['\'self\''],

  // 脚本源：允许同源和内联脚本（Astro需要）
  'script-src': [
    '\'self\'',
    '\'unsafe-inline\'', // Astro 内联脚本需要
    '\'unsafe-eval\'', // 开发环境需要，生产环境应移除
  ],

  // 样式源：允许同源和内联样式
  'style-src': [
    '\'self\'',
    '\'unsafe-inline\'', // Tailwind CSS 需要
    'https://fonts.googleapis.com',
  ],

  // 字体源
  'font-src': [
    '\'self\'',
    'https://fonts.gstatic.com',
  ],

  // 图片源:允许同源、data URI 和外部图片服务
  'img-src': [
    '\'self\'',
    'data:',
    'blob:',
    'https:', // 允许所有 HTTPS 图片(壁纸服务)
  ],

  // 媒体源
  'media-src': ['\'self\''],

  // 连接源：API 请求 - 动态生成
  'connect-src': [], // 将在 generateCSPString 中动态填充

  // Frame 源：禁止嵌入
  'frame-src': ['\'none\''],

  // Object 源：禁止插件
  'object-src': ['\'none\''],

  // Base URI：限制 <base> 标签
  'base-uri': ['\'self\''],

  // Form 动作：限制表单提交
  'form-action': ['\'self\''],

  // Frame 祖先：防止点击劫持
  'frame-ancestors': ['\'none\''],

  // 升级不安全请求（生产环境）
  'upgrade-insecure-requests': [],
}

/**
 * 生成 CSP 字符串
 */
export function generateCSPString(isDev: boolean = false): string {
  const directives: Record<string, string[]> = { ...CSP_DIRECTIVES }

  // 动态设置 connect-src
  directives['connect-src'] = getConnectSources()

  // 生产环境移除 unsafe-eval
  if (!isDev && directives['script-src']) {
    directives['script-src'] = directives['script-src'].filter(
      src => src !== '\'unsafe-eval\'',
    )
  }

  return Object.entries(directives)
    .map(([key, values]) => {
      if (values.length === 0)
        return key
      return `${key} ${values.join(' ')}`
    })
    .join('; ')
}

/**
 * 其他安全响应头配置
 *
 * ⚠️ 注意: 这些响应头必须在后端设置，不能通过 HTML meta 标签设置
 * 请在 Rust 后端或 Nginx 反向代理中配置这些响应头
 */
export const SECURITY_HEADERS = {
  // 防止点击劫持 (必须在后端设置)
  'X-Frame-Options': 'DENY',

  // 防止 MIME 类型嗅探 (必须在后端设置)
  'X-Content-Type-Options': 'nosniff',

  // XSS 保护 - 旧浏览器 (必须在后端设置)
  'X-XSS-Protection': '1; mode=block',

  // Referrer 策略 (可以通过 meta 标签设置，已在 Layout.astro 中配置)
  'Referrer-Policy': 'strict-origin-when-cross-origin',

  // 权限策略 (必须在后端设置)
  'Permissions-Policy': 'geolocation=(), microphone=(), camera=()',

  // HSTS - 仅生产环境且使用 HTTPS 时启用 (必须在后端设置)
  'Strict-Transport-Security': 'max-age=31536000; includeSubDomains; preload',
}

/**
 * 将安全头转换为 meta 标签
 *
 * ⚠️ 注意: 只有 CSP 和 Referrer-Policy 可以通过 meta 标签设置
 * 其他安全响应头必须在后端设置
 */
export function generateSecurityMetaTags(isDev: boolean = false): string {
  const csp = generateCSPString(isDev)

  return `
    <meta name="referrer" content="strict-origin-when-cross-origin">
    ${!isDev ? `<meta http-equiv="Content-Security-Policy" content="${csp}">` : ''}
  `.trim()
}
