import { Buffer } from 'node:buffer'
import { readdirSync, readFileSync, writeFileSync } from 'node:fs'
import http from 'node:http'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import react from '@astrojs/react'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig, fontProviders } from 'astro/config'
// rollup-plugin-visualizer 与 Vite 7 (Rolldown) 不兼容，仅在构建时按需加载
// import { visualizer } from 'rollup-plugin-visualizer'

const __dirname = path.dirname(fileURLToPath(import.meta.url))

// 读取 package.json 版本号
const pkg = JSON.parse(
  readFileSync(path.resolve(__dirname, 'package.json'), 'utf-8'),
)
const APP_VERSION = pkg.version || '0.4.0'

/**
 * 自定义 Vite 插件：SPA 路由回退
 * 将动态路由（如 /tapp/run/:id）在服务端重定向到 catch-all 页面
 * 但保留原始 URL，让 React Router 在客户端正确解析参数
 */
/** Align with proxy/backend: document-level geolocation for weather. */
const DOCUMENT_PERMISSIONS_POLICY =
  'geolocation=(self), microphone=(self), camera=()'

function spaFallbackPlugin() {
  return {
    name: 'spa-fallback',
    enforce: 'pre', // 确保在其他中间件之前执行
    configureServer(server) {
      // 直接添加中间件，不返回函数
      server.middlewares.use((req, res, next) => {
        // Dev: document gets Permissions-Policy without going through Myriad proxy
        if (!res.getHeader('Permissions-Policy')) {
          res.setHeader('Permissions-Policy', DOCUMENT_PERMISSIONS_POLICY)
        }

        const url = req.url || ''

        // 动态 Tapp 路由回退：/tapp/run/* 和 /tapp/detail/*
        // 服务端将这些路径重写为占位路径，但浏览器 URL 保持不变
        if (/^\/tapp\/run\/[^_/][^/]*/.test(url)) {
          req.url = '/tapp/run/_'
        } else if (/^\/tapp\/run(\?|$)/.test(url)) {
          // 多任务模式：/tapp/run 或 /tapp/run?multi=true
          req.url = '/tapp/run/_'
        } else if (/^\/tapp\/detail\/[^_/][^/]*/.test(url)) {
          req.url = '/tapp/detail/_'
        }
        // Brew 自有文章 SEO 路径：/brew/item/:id
        else if (/^\/brew\/item\/[^/]+/.test(url)) {
          req.url = '/brew'
        }
        // 联邦动态路由回退
        else if (/^\/federation\/chat\/[^_/][^/]*/.test(url)) {
          req.url = '/federation/chat/_'
        } else if (/^\/federation\/room\/[^_/][^/]*/.test(url)) {
          req.url = '/federation/room/_'
        } else if (/^\/federation\/ring\/[^_/][^/]*/.test(url)) {
          req.url = '/federation/ring/_'
        }

        next()
      })
    },
    configurePreviewServer(server) {
      server.middlewares.use((_req, res, next) => {
        if (!res.getHeader('Permissions-Policy')) {
          res.setHeader('Permissions-Policy', DOCUMENT_PERMISSIONS_POLICY)
        }
        next()
      })
    },
  }
}

const BACKEND_TARGET = 'http://127.0.0.1:1103'

// Must stay >= TappPlaygroundService AbortSignal and cover planner + up to 3
// repair model calls (each may use backend MODEL_REQUEST_TIMEOUT of 1080s).
// Node http.request timeout is socket-idle; playground holds the connection
// with no response bytes until generation finishes.
const PLAYGROUND_PROXY_TIMEOUT_MS = 30 * 60 * 1000
// Federation file-meta downloads / chunk uploads can exceed the default 30s.
const FEDERATION_TRANSFER_PROXY_TIMEOUT_MS = 10 * 60 * 1000
// Merope portrait + Agent persona onboarding (Pro distill / name / draft /
// visual design). Backend Pro and image-generation sockets stay idle until the
// model returns. Default 30s proxy timeout surfaces as "Backend proxy timeout".
const MEROPE_PROXY_TIMEOUT_MS = 15 * 60 * 1000

const HOP_BY_HOP_HEADERS = new Set([
  'connection',
  'keep-alive',
  'proxy-authenticate',
  'proxy-authorization',
  'te',
  'trailer',
  'transfer-encoding',
  'upgrade',
  'host',
])

/** Pathname only: drop query, hash, trailing slash, and absolute-URL origin. */
function requestPathname(urlPath) {
  const raw = String(urlPath || '').trim()
  try {
    const url =
      raw.startsWith('http://') || raw.startsWith('https://')
        ? new URL(raw)
        : new URL(raw, 'http://dev.invalid')
    return url.pathname.replace(/\/+$/, '') || '/'
  } catch {
    const path = (raw.split('?')[0] || '').split('#')[0].replace(/\/+$/, '')
    return path || '/'
  }
}

/** Path-only (no query). Completed transfer byte stream — must not buffer. */
function isFederationTransferContentPath(urlPath) {
  const path = requestPathname(urlPath)
  return /^\/api\/federation\/transfers\/[^/]+\/content$/.test(path)
}

function isMeropeApiPath(urlPath) {
  return requestPathname(urlPath).startsWith('/api/merope/')
}

function isAgentPersonaGenerationPath(urlPath) {
  const path = requestPathname(urlPath)
  return (
    path === '/api/agent/persona/signals' ||
    path === '/api/agent/persona/draft' ||
    path === '/api/agent/persona/name' ||
    path === '/api/agent/persona/visual-design'
  )
}

/**
 * Agent live progress. Must pipe even if the browser forgot Accept:
 * text/event-stream — the buffering proxy collects the whole run (30s cap)
 * and dumps thinking + answer as one body.
 */
function isAgentSsePath(urlPath) {
  const path = requestPathname(urlPath)
  return (
    path === '/api/agent/process/stream' ||
    path === '/api/agent/confirm/stream' ||
    /^\/api\/agent\/runs\/[^/]+\/stream$/.test(path) ||
    /^\/api\/agent\/tasks\/[^/]+\/answer\/stream$/.test(path)
  )
}

/** Long-running federation transfer REST (initiate / list / chunk / cancel / get). */
function isFederationTransferApiPath(urlPath) {
  const path = requestPathname(urlPath)
  if (path.startsWith('/api/federation/transfers/')) return true
  return (
    /^\/api\/federation\/channels\/[^/]+\/transfers$/.test(path) ||
    /^\/api\/federation\/rooms\/[^/]+\/transfers$/.test(path)
  )
}

async function readRequestBody(req) {
  const chunks = []
  for await (const chunk of req) {
    chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk))
  }
  return Buffer.concat(chunks)
}

function wait(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

function proxyBackendRequest(targetUrl, method, headers, body, timeoutMs) {
  return new Promise((resolve, reject) => {
    const requestHeaders = Object.fromEntries(headers.entries())
    requestHeaders.connection = 'close'
    if (body && body.length > 0) {
      requestHeaders['content-length'] = String(body.length)
    }

    const backendReq = http.request(
      targetUrl,
      {
        method,
        headers: requestHeaders,
        agent: false,
        timeout: timeoutMs,
      },
      (backendRes) => {
        // Headers arrived: do not keep the idle timer for a small JSON body.
        backendReq.setTimeout(0)
        const chunks = []
        backendRes.on('data', (chunk) =>
          chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk)),
        )
        backendRes.on('end', () => {
          resolve({
            statusCode: backendRes.statusCode || 502,
            statusMessage: backendRes.statusMessage || 'Bad Gateway',
            headers: backendRes.headers,
            body: Buffer.concat(chunks),
          })
        })
        backendRes.on('error', reject)
      },
    )

    if (timeoutMs > 0) {
      backendReq.setTimeout(timeoutMs)
    }
    backendReq.on('timeout', () => {
      backendReq.destroy(new Error('Backend proxy timeout'))
    })
    backendReq.on('error', reject)

    if (body && body.length > 0) {
      backendReq.end(body)
    } else {
      backendReq.end()
    }
  })
}

/**
 * Stream playground SSE (and similar long responses) without buffering the
 * full body. Client abort closes the upstream request so the backend can cancel.
 */
function proxyBackendRequestStreaming(
  targetUrl,
  method,
  headers,
  body,
  timeoutMs,
  clientReq,
  clientRes,
) {
  return new Promise((resolve, reject) => {
    const requestHeaders = Object.fromEntries(headers.entries())
    requestHeaders.connection = 'close'
    if (body && body.length > 0) {
      requestHeaders['content-length'] = String(body.length)
    }

    let settled = false
    const settle = (fn, value) => {
      if (settled) return
      settled = true
      fn(value)
    }

    const backendReq = http.request(
      targetUrl,
      {
        method,
        headers: requestHeaders,
        agent: false,
        timeout: timeoutMs,
      },
      (backendRes) => {
        clientRes.statusCode = backendRes.statusCode || 502
        if (backendRes.statusMessage) {
          clientRes.statusMessage = backendRes.statusMessage
        }
        clientRes.setHeader('x-myriad-dev-proxy', 'http-stream')

        for (const [name, value] of Object.entries(backendRes.headers)) {
          const lowerName = name.toLowerCase()
          if (value != null && !HOP_BY_HOP_HEADERS.has(lowerName)) {
            clientRes.setHeader(name, value)
          }
        }

        backendRes.on('error', (error) => {
          if (!clientRes.writableEnded) {
            clientRes.destroy(error)
          }
          settle(reject, error)
        })
        backendRes.on('end', () => settle(resolve, undefined))
        backendRes.pipe(clientRes)
      },
    )

    const abortUpstream = () => {
      backendReq.destroy()
      if (!clientRes.writableEnded) {
        clientRes.destroy()
      }
    }

    clientReq.on('aborted', abortUpstream)
    clientReq.on('close', () => {
      if (!clientRes.writableEnded) {
        abortUpstream()
      }
    })
    clientRes.on('close', () => {
      if (!backendReq.destroyed) {
        backendReq.destroy()
      }
    })

    backendReq.on('timeout', () => {
      const error = new Error('Backend proxy timeout')
      backendReq.destroy(error)
      if (!clientRes.headersSent) {
        settle(reject, error)
      } else {
        clientRes.destroy(error)
        settle(reject, error)
      }
    })
    backendReq.on('error', (error) => settle(reject, error))

    if (body && body.length > 0) {
      backendReq.end(body)
    } else {
      backendReq.end()
    }
  })
}

/**
 * Paths that must hit the backend in dev, matching production proxy
 * `is_backend_path` in `proxy/src/main.rs`. Path-only (query stripped).
 * Does NOT proxy ACME challenge under .well-known.
 *
 * TODO: This one-shot node:http proxy does not perform WebSocket upgrades.
 * Federation WS under /api/federation/.../ws is not available through the
 * Astro dev proxy; use a production-like proxy stack or hit backend:1103
 * directly for WS during local development. REST ActivityPub paths are the
 * critical fix.
 */
function isSeoCrawlerUserAgent(ua) {
  const s = String(ua || '').toLowerCase()
  const markers = [
    'googlebot',
    'bingbot',
    'slurp',
    'duckduckbot',
    'baiduspider',
    'yandexbot',
    'facebookexternalhit',
    'facebot',
    'twitterbot',
    'linkedinbot',
    'embedly',
    'pinterest',
    'applebot',
    'semrushbot',
    'ahrefsbot',
    'discordbot',
    'telegrambot',
    'whatsapp',
    'slackbot',
    'redditbot',
    'skypeuripreview',
    'chatgpt-user',
    'gptbot',
    'claudebot',
    'storebot-google',
    'google-inspectiontool',
    'preview',
  ]
  if (markers.some((m) => s.includes(m))) return true
  return s.includes('bot/') || s.includes('spider') || s.includes('crawler')
}

/**
 * @param {string} urlPath
 * @param {string} [userAgent]
 */
function isBackendDevProxyPath(urlPath, userAgent) {
  const path = requestPathname(urlPath)
  if (
    path.startsWith('/api/') ||
    path === '/health' ||
    path === '/sitemap.xml' ||
    path === '/robots.txt'
  ) {
    return true
  }
  // Crawler HTML shells (humans stay on SPA)
  if (path.startsWith('/tapp/run/') && isSeoCrawlerUserAgent(userAgent)) {
    return true
  }
  if (path.startsWith('/brew/item/') && isSeoCrawlerUserAgent(userAgent)) {
    return true
  }
  return (
    path === '/.well-known/webfinger' ||
    path === '/.well-known/nodeinfo' ||
    path === '/nodeinfo/2.1' ||
    path === '/inbox' ||
    path.startsWith('/users/') ||
    // Federation Note attachment media (must match proxy is_backend_path)
    path.startsWith('/media/federation/')
  )
}

/**
 * Dev-only backend proxy implemented with one-shot node:http requests.
 * This avoids Vite http-proxy and undici keep-alive socket reuse while
 * preserving same-origin API URLs during local development.
 *
 * Middleware order note (Astro 7+):
 * Astro's sec-fetch middleware is `unshift`ed in a configureServer post-hook and
 * blocks subresource requests with Sec-Fetch-Site: cross-site. TApp sandboxes
 * use srcdoc (opaque origin), so `<img src="/api/proxy/image…">` is treated as
 * cross-site and never reaches the backend. We therefore install this proxy in a
 * post-hook as well (no `enforce: 'pre'`) so our unshift runs after Astro's and
 * sits at the front of the Connect stack.
 */
function backendDevProxyPlugin() {
  return {
    name: 'backend-dev-proxy',
    apply: 'serve',
    // Intentionally not `enforce: 'pre'`: post-hooks from pre plugins run before
    // Astro's, so Astro's sec-fetch unshift would still land in front of us.
    configureServer(server) {
      const handler = async (req, res, next) => {
        const originalUrl = req.originalUrl || req.url || ''
        const ua = req.headers['user-agent'] || ''
        if (!isBackendDevProxyPath(originalUrl, ua)) {
          next()
          return
        }

        try {
          const targetUrl = new URL(originalUrl, BACKEND_TARGET)
          const headers = new Headers()

          for (const [name, value] of Object.entries(req.headers)) {
            if (HOP_BY_HOP_HEADERS.has(name.toLowerCase()) || value == null) {
              continue
            }
            if (Array.isArray(value)) {
              for (const item of value) {
                headers.append(name, item)
              }
            } else {
              headers.set(name, value)
            }
          }

          const method = req.method || 'GET'
          const hasBody = method !== 'GET' && method !== 'HEAD'
          const body = hasBody ? await readRequestBody(req) : undefined
          const retryable = method === 'GET' || method === 'HEAD'
          const requestPath = requestPathname(originalUrl)
          const timeoutMs = requestPath.startsWith('/api/tapp-playground/')
            ? PLAYGROUND_PROXY_TIMEOUT_MS
            : isFederationTransferApiPath(originalUrl) ||
                isFederationTransferContentPath(originalUrl)
              ? FEDERATION_TRANSFER_PROXY_TIMEOUT_MS
              : isMeropeApiPath(originalUrl) ||
                  isAgentPersonaGenerationPath(originalUrl)
                ? MEROPE_PROXY_TIMEOUT_MS
                : 30000
          // SSE and large transfer downloads must be piped. Buffering a multi-MB
          // GET /transfers/{id}/content (or a long-lived EventSource) hits the
          // ordinary timeout / memory path and turns a healthy stream into 502.
          const streamResponse =
            headers
              .get('accept')
              ?.toLowerCase()
              .includes('text/event-stream') ||
            originalUrl.startsWith('/api/tapp-playground/generate-stream') ||
            isFederationTransferContentPath(originalUrl) ||
            isAgentSsePath(originalUrl)

          if (streamResponse) {
            await proxyBackendRequestStreaming(
              targetUrl,
              method,
              headers,
              body,
              // content download: idle timeout 10m; SSE playground still uses 0
              isFederationTransferContentPath(originalUrl)
                ? FEDERATION_TRANSFER_PROXY_TIMEOUT_MS
                : 0,
              req,
              res,
            )
            return
          }

          let response
          let lastError
          for (let attempt = 0; attempt < 4; attempt++) {
            try {
              response = await proxyBackendRequest(
                targetUrl,
                method,
                headers,
                body,
                timeoutMs,
              )
              break
            } catch (error) {
              lastError = error
              if (!retryable || attempt === 3) {
                throw error
              }
              await wait(120 * (attempt + 1))
            }
          }

          if (!response) {
            throw lastError || new Error('Backend proxy failed')
          }

          res.statusCode = response.statusCode
          res.statusMessage = response.statusMessage
          res.setHeader('x-myriad-dev-proxy', 'http')

          for (const [name, value] of Object.entries(response.headers)) {
            const lowerName = name.toLowerCase()
            if (value != null && !HOP_BY_HOP_HEADERS.has(lowerName)) {
              res.setHeader(name, value)
            }
          }

          res.end(response.body)
        } catch (error) {
          server.config.logger.error(
            `[backend-dev-proxy] ${req.method || 'GET'} ${originalUrl} failed: ${
              error instanceof Error
                ? `${error.message}\n${error.stack || ''}`
                : String(error)
            }`,
          )
          if (!res.headersSent) {
            res.statusCode = 502
            res.setHeader('Content-Type', 'application/json')
            res.setHeader('x-myriad-dev-proxy', 'http')
          }
          res.end(
            JSON.stringify({
              error: 'Backend proxy failed',
              message: error instanceof Error ? error.message : String(error),
            }),
          )
        }
      }

      // Post-hook: run after Astro unshifts sec-fetch, then put API proxy first.
      return () => {
        server.middlewares.stack.unshift({
          route: '',
          handle: handler,
        })
      }
    },
  }
}

/**
 * 首屏 CSS 瘦身：Astro/Vite 会把懒加载路由的 CSS 也写成 HTML <link>，
 * 阻塞首页 FCP。将非首屏样式从 HTML 剥离，并在对应 JS chunk 执行时再注入。
 *
 * 保留（首屏/全局需要）：
 * - tailwind / index / App 全局样式
 *
 * 已 defer（从 HTML 剥离，随拥有方 JS 注入）：
 * - Toast：Toast 组件 chunk；若并入 shell 则 App 也注入（幂等）
 * - MusicPlayer：控制面板懒加载 MusicPlayer chunk
 * - AraelPanel / Config / ConfigForm / Setup / TappPlaygroundPage
 */
function deferNonCriticalCssIntegration() {
  /** CSS 文件名前缀 → 应注入该 CSS 的 JS chunk 前缀列表 */
  const DEFER = [
    { cssPrefix: 'AraelPanel-', jsPrefixes: ['AraelPanel-'] },
    { cssPrefix: 'Config-', jsPrefixes: ['Config-'] },
    { cssPrefix: 'ConfigForm-', jsPrefixes: ['Config-'] },
    { cssPrefix: 'Setup-', jsPrefixes: ['Setup-'] },
    { cssPrefix: 'TappPlaygroundPage-', jsPrefixes: ['TappPlaygroundPage-'] },
    // Toast.css 来自 Toast.tsx；ToastContainer 在 AppLayout 同步引用，
    // chunk 可能是 Toast-* 或并入 App-*，两者都注入（createElement 幂等）。
    { cssPrefix: 'Toast-', jsPrefixes: ['Toast-', 'App-'] },
    // MusicPlayer.css 由 ControlPanel/MusicPlayer 懒加载引入
    { cssPrefix: 'MusicPlayer-', jsPrefixes: ['MusicPlayer-'] },
  ]

  function cssInjectorSnippet(href) {
    // 幂等：已存在则跳过（含 HTML 误保留或重复执行）
    return `(function(){try{var h=${JSON.stringify(href)};if(document.querySelector('link[href="'+h+'"]'))return;var l=document.createElement("link");l.rel="stylesheet";l.href=h;document.head.appendChild(l)}catch(e){}})();`
  }

  return {
    name: 'defer-non-critical-css',
    hooks: {
      'astro:build:done': async ({ dir }) => {
        const outDir = fileURLToPath(dir)
        const assetsDir = path.join(outDir, 'assets')
        let assetFiles = []
        try {
          assetFiles = readdirSync(assetsDir)
        } catch {
          return
        }

        const cssFiles = assetFiles.filter((f) => f.endsWith('.css'))
        const jsFiles = assetFiles.filter((f) => f.endsWith('.js'))

        /** @type {Map<string, string[]>} jsFileName -> css hrefs to inject */
        const injectMap = new Map()
        /** @type {Set<string>} basenames stripped from HTML */
        const stripCss = new Set()

        for (const rule of DEFER) {
          const matchedCss = cssFiles.filter((f) =>
            f.startsWith(rule.cssPrefix),
          )
          for (const cssName of matchedCss) {
            stripCss.add(cssName)
            const href = `/assets/${cssName}`
            for (const jsPrefix of rule.jsPrefixes) {
              const matchedJs = jsFiles.filter((f) => f.startsWith(jsPrefix))
              for (const jsName of matchedJs) {
                const list = injectMap.get(jsName) || []
                if (!list.includes(href)) list.push(href)
                injectMap.set(jsName, list)
              }
            }
          }
        }

        // 注入到异步 chunk 头部
        for (const [jsName, hrefs] of injectMap) {
          const jsPath = path.join(assetsDir, jsName)
          const original = readFileSync(jsPath, 'utf8')
          // 避免重复注入
          if (
            hrefs.every(
              (h) =>
                original.includes(h) &&
                original.includes('createElement("link")'),
            )
          ) {
            // 可能已有 vite 注入；仍确保我们的幂等片段存在
          }
          const banner = hrefs.map(cssInjectorSnippet).join('')
          if (!original.startsWith('(function(){try{var h=')) {
            writeFileSync(jsPath, banner + original)
          }
        }

        // 从所有 HTML 去掉对应 <link rel="stylesheet">
        const stripRe = new RegExp(
          `<link[^>]+href="/assets/(${[...stripCss]
            .map((s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))
            .join('|')})"[^>]*>`,
          'g',
        )

        let htmlCount = 0
        let removed = 0
        for (const name of readdirSync(outDir)) {
          if (!name.endsWith('.html')) continue
          const htmlPath = path.join(outDir, name)
          let html = readFileSync(htmlPath, 'utf8')
          const before = html
          html = html.replace(stripRe, () => {
            removed++
            return ''
          })
          if (html !== before) {
            writeFileSync(htmlPath, html)
            htmlCount++
          }
        }

        console.log(
          `[defer-non-critical-css] stripped ${removed} link(s) from ${htmlCount} html; injected into ${injectMap.size} js chunk(s)`,
        )
      },
    },
  }
}

// https://astro.build/config
export default defineConfig({
  integrations: [react(), deferNonCriticalCssIntegration()],
  // 使用 hybrid 模式：默认静态预渲染，但允许特定页面动态渲染
  // 这样可以支持 /tapp/run/:id 等动态路由
  output: 'static',
  server: {
    port: 1102,
    host: true,
  },
  build: {
    inlineStylesheets: 'auto',
    // 与下方 trailingSlash: 'never' 配对：产出 dist/setup.html 而非
    // dist/setup/index.html。否则每个预渲染路由都是目录，后端 tower-http
    // ServeDir 对无斜杠的目录请求会 307 到 /setup/，与前端"URL 不带斜杠"
    // 的约定冲突，触发 /setup ↔ /setup/ 无限重定向。file 格式下无目录、无 307。
    format: 'file',
  },
  // Astro 6: 内置 Fonts API - 自动下载并自托管 Google Fonts，优化性能和隐私
  // 所有字体均通过此 API 自托管，消除对 Google Fonts CDN 的运行时请求
  fonts: [
    // 主体字体
    {
      provider: fontProviders.google(),
      name: 'Inter',
      cssVariable: '--font-inter',
      weights: [400, 500, 600, 700],
      styles: ['normal'],
      fallbacks: [
        '-apple-system',
        'BlinkMacSystemFont',
        'Segoe UI',
        'sans-serif',
      ],
    },
    // 标题装饰字体（由 useTitleFont hook 按需切换）
    // 每个字体只注册实际使用的字重（与 fonts.css .title-font-* 对齐），避免多余 face
    {
      provider: fontProviders.google(),
      name: 'Qwitcher Grypen',
      cssVariable: '--font-qwitcher-grypen',
      weights: [700],
      styles: ['normal'],
      fallbacks: ['cursive'],
    },
    {
      provider: fontProviders.google(),
      name: 'Codystar',
      cssVariable: '--font-codystar',
      weights: [400],
      styles: ['normal'],
      fallbacks: ['system-ui'],
    },
    {
      provider: fontProviders.google(),
      name: 'Henny Penny',
      cssVariable: '--font-henny-penny',
      weights: [400],
      styles: ['normal'],
      fallbacks: ['system-ui'],
    },
    {
      provider: fontProviders.google(),
      name: 'Srisakdi',
      cssVariable: '--font-srisakdi',
      weights: [700],
      styles: ['normal'],
      fallbacks: ['system-ui'],
    },
    {
      provider: fontProviders.google(),
      name: 'Fleur De Leah',
      cssVariable: '--font-fleur-de-leah',
      weights: [400],
      styles: ['normal'],
      fallbacks: ['cursive'],
    },
    {
      provider: fontProviders.google(),
      name: 'League Script',
      cssVariable: '--font-league-script',
      weights: [400],
      styles: ['normal'],
      fallbacks: ['cursive'],
    },
    {
      provider: fontProviders.google(),
      name: 'Megrim',
      cssVariable: '--font-megrim',
      weights: [400],
      styles: ['normal'],
      fallbacks: ['system-ui'],
    },
    {
      provider: fontProviders.google(),
      name: 'Silkscreen',
      cssVariable: '--font-silkscreen',
      weights: [700],
      styles: ['normal'],
      fallbacks: ['system-ui'],
    },
    {
      provider: fontProviders.google(),
      name: 'UnifrakturMaguntia',
      cssVariable: '--font-unifraktur-maguntia',
      weights: [400],
      styles: ['normal'],
      fallbacks: ['serif'],
    },
    {
      provider: fontProviders.google(),
      name: 'Cinzel',
      cssVariable: '--font-cinzel',
      weights: [700],
      styles: ['normal'],
      fallbacks: ['serif'],
    },
  ],
  // SPA 模式：所有路由都重定向到 index.html
  trailingSlash: 'never',
  vite: {
    define: {
      __APP_VERSION__: JSON.stringify(APP_VERSION),
    },
    optimizeDeps: {
      // Pre-bundle deps used by lazy routes (Tapp detail / playground).
      // Discovering them mid-session triggers "504 Outdated Optimize Dep" and
      // breaks React.lazy chunks like TappDetailView until a full hard reload.
      include: [
        'jszip',
        'prismjs',
        'prismjs/components/prism-json',
        'react',
        'react-dom',
        'react-dom/client',
        'react-router-dom',
      ],
      // UMD bundle; Vite prebundle rewrites break createClient.
      exclude: ['agora-rtc-sdk-ng'],
    },
    ssr: {
      external: ['agora-rtc-sdk-ng'],
    },
    plugins: [
      tailwindcss(), // Tailwind CSS v4 Vite plugin
      backendDevProxyPlugin(), // 开发环境 API 转发，绕开 Vite http-proxy 的 socket 500
      spaFallbackPlugin(), // 自定义 SPA 路由回退
    ],
    resolve: {
      alias: {
        '@': path.resolve(__dirname, './src'),
        '@components': path.resolve(__dirname, './src/components'),
        '@layouts': path.resolve(__dirname, './src/layouts'),
        '@lib': path.resolve(__dirname, './src/lib'),
        '@config': path.resolve(__dirname, './src/config.ts'),
        // 与后端共用的静态契约（image_proxy_hosts.json 等）
        '@shared': path.resolve(__dirname, '../shared'),
      },
    },
    // Astro 6 / Vite 7: 客户端 Rollup 输出配置迁移到 environments.client
    environments: {
      client: {
        build: {
          rollupOptions: {
            output: {
              manualChunks: (id) => {
                // React 核心 + React Router 合并到同一 chunk
                // 避免 React Router v7 在 React Context 初始化前加载导致 hydration 错误
                if (
                  id.includes('node_modules/react/') ||
                  id.includes('node_modules/react-dom/') ||
                  id.includes('node_modules/react-router') ||
                  id.includes('node_modules/@remix-run') ||
                  // jsx-runtime 的模块 id 可能不带 node_modules/react/ 前缀
                  // （pnpm 布局 / 虚拟模块）。不显式归类的话，Rolldown 会把它
                  // 塞进任意 chunk（实测进了 motion），导致所有 JSX chunk
                  // 为了 1KB 的 jsx-runtime 静态依赖整个 124K motion chunk
                  id.includes('jsx-runtime')
                ) {
                  return 'react-vendor'
                }
                // Chart.js
                if (
                  id.includes('node_modules/chart.js') ||
                  id.includes('node_modules/react-chartjs-2')
                ) {
                  return 'chart-vendor'
                }
                // Motion — 注意 motion-dom / motion-utils 是独立包，
                // 路径同样含 node_modules/motion，若并入同一 chunk，
                // 其中被共享的小工具会让整个 124K chunk 变成静态依赖，
                // 破坏 lazyMotion 的动态加载设计
                if (id.includes('node_modules/motion-utils')) {
                  return 'motion-utils'
                }
                if (id.includes('node_modules/motion-dom')) {
                  return 'motion-dom'
                }
                if (id.includes('node_modules/motion')) {
                  return 'motion'
                }
                // react-icons 各子包分开打包（仅动态导入时使用）
                if (id.includes('node_modules/react-icons/fa6/')) {
                  return 'icons-fa6'
                }
                if (id.includes('node_modules/react-icons/fa/')) {
                  return 'icons-fa'
                }
                if (id.includes('node_modules/react-icons/si/')) {
                  return 'icons-si'
                }
                if (id.includes('node_modules/react-icons')) {
                  return 'icons-base'
                }
                // Axios
                if (id.includes('node_modules/axios')) {
                  return 'axios'
                }
              },
              // 优化文件名用于长期缓存
              chunkFileNames: 'assets/[name]-[hash].js',
              entryFileNames: 'assets/[name]-[hash].js',
              assetFileNames: 'assets/[name]-[hash].[ext]',
            },
          },
        },
      },
    },
    build: {
      cssCodeSplit: true,
      minify: 'terser',
      terserOptions: {
        compress: {
          // eslint-disable-next-line node/prefer-global/process
          drop_console: process.env.NODE_ENV === 'production',
          drop_debugger: true,
          passes: 2,
        },
        mangle: {
          safari10: true,
        },
      },
      assetsInlineLimit: 4096,
      // 启用 gzip 和 brotli 压缩报告
      reportCompressedSize: true,
      chunkSizeWarningLimit: 1000,
    },
  },
})
