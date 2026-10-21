/** Align with proxy/backend: document-level geolocation for weather. */
export const DOCUMENT_PERMISSIONS_POLICY =
  'geolocation=(self), microphone=(self), camera=()'

/** `/embed/*` documents are framed by `/blog` pages on this origin and nowhere else. */
export const EMBED_FRAME_HEADERS = Object.freeze({
  'Content-Security-Policy': "frame-ancestors 'self'",
  'X-Frame-Options': 'SAMEORIGIN',
  // 嵌入页只是博客页里的一块，单独收录只会多出一批标题是站名的空页。
  'X-Robots-Tag': 'noindex',
})

/**
 * 下线的页面：手帐（换成 /blog）和报告页（换成报告卡的舞台弹窗）。都不做跳转，
 * 回 410 让搜索引擎把旧地址清掉，而不是收到一份 200 的空壳当软 404 慢慢猜。
 */
const RETIRED_PREFIXES = ['/journal', '/reports']

export function isRetiredPath(pathname) {
  return RETIRED_PREFIXES.some((prefix) => pathname === prefix || pathname.startsWith(`${prefix}/`))
}

export const RETIRED_PAGE_HEADERS = Object.freeze({
  'Content-Type': 'text/html; charset=utf-8',
  'Cache-Control': 'public, max-age=3600',
  'X-Robots-Tag': 'noindex',
})

export const RETIRED_PAGE_HTML =
  '<!DOCTYPE html><html><head><meta charset="utf-8"><meta name="robots" content="noindex">'
  + '<meta name="viewport" content="width=device-width, initial-scale=1"><title>410 Gone</title></head>'
  + '<body><p>This page has been retired.</p><p><a href="/">Home</a></p></body></html>'

/**
 * 构建时 Vite 把它写进文档里每个 `<script>`、modulepreload 和 `meta[property=csp-nonce]`
 * （`html.cspNonce`），spa-server 每次请求换成新的 nonce。含下划线，不会被当成真 nonce。
 */
export const CSP_NONCE_PLACEHOLDER = '__MYRIAD_CSP_NONCE__'

/** 与 backend `security.rs` 的生产默认一致；声网实时通话要 wss/stun/turn。 */
export const DEFAULT_CSP_CONNECT_SRC = "'self' https: wss: stun: turn:"

/**
 * 前端文档的 CSP。除脚本外与 backend 生产策略一致。
 *
 * 脚本用 nonce + 'strict-dynamic'，不用哈希 + 'self'：TApp 的 srcdoc iframe 继承这份策略，
 * 它的脚本是运行时拼的，只能带同一个 nonce 通过；统计脚本（GA、站长填的 Umami 地址）由
 * 我们自己的脚本插入，靠 'strict-dynamic' 放行。'self' 只给不认 'strict-dynamic' 的旧浏览器。
 * connect-src / media-src 的 blob: data: 也是给 TApp 的（它自己的策略只放这两种）。
 * 不发 upgrade-insecure-requests：自托管可能只有 HTTP，文档上加它整站资源都会失败。
 */
export function documentContentSecurityPolicy({ nonce, embed, connectSrc = DEFAULT_CSP_CONNECT_SRC }) {
  return [
    "default-src 'self'",
    `script-src 'nonce-${nonce}' 'strict-dynamic' 'wasm-unsafe-eval' 'self'`,
    "script-src-attr 'none'",
    "style-src 'self' 'unsafe-inline' https:",
    "img-src 'self' data: blob: https: http:",
    "font-src 'self' data: https: blob:",
    "media-src 'self' https: blob: data:",
    `connect-src ${connectSrc} blob: data:`,
    "object-src 'none'",
    "base-uri 'self'",
    "form-action 'self'",
    `frame-ancestors ${embed ? "'self'" : "'none'"}`,
    "frame-src 'self'",
    "worker-src 'self' blob:",
    "manifest-src 'self'",
  ].join('; ')
}

export function isEmbedDocumentPath(pathname) {
  return pathname === '/embed' || pathname.startsWith('/embed/')
}

/** 嵌入页（伴读、评论、友链申请、创作台）用自己的 `embed.html`，不走主站的 `index.html`。 */
export const EMBED_DOCUMENT = 'embed.html'

export const BACKEND_TARGET = 'http://127.0.0.1:1103'

export const STATIC_ASSET_PATTERN =
  /\.(?:js|mjs|cjs|css|map|png|webp|jpe?g|gif|svg|ico|woff2?|ttf|otf|json|webmanifest|txt|wasm|webm|mp3|mp4)$/i
