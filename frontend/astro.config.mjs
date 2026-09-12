import { Buffer } from 'node:buffer'
import { readdirSync, readFileSync, writeFileSync } from 'node:fs'
import http from 'node:http'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import react from '@astrojs/react'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig, fontProviders } from 'astro/config'
import {
  AI_IMAGE_REQUEST_TIMEOUT_MS,
  AI_REQUEST_TIMEOUT_FLOOR_MS,
  aiRequestTimeoutMs,
} from './src/utils/aiRequestTimeout.mjs'
// rollup-plugin-visualizer is incompatible with Vite/Rolldown; do not import it.
// import { visualizer } from 'rollup-plugin-visualizer'

const __dirname = path.dirname(fileURLToPath(import.meta.url))

const pkg = JSON.parse(
  readFileSync(path.resolve(__dirname, 'package.json'), 'utf-8'),
)
const APP_VERSION = pkg.version || '0.4.9'

/** Align with proxy/backend: document-level geolocation for weather. */
const DOCUMENT_PERMISSIONS_POLICY =
  'geolocation=(self), microphone=(self), camera=()'

/**
 * Dev source maps double every module (original source as base64). On a
 * 1200-file SPA plus icon barrels that is tens of MB of V8 script source,
 * and HMR keeps the old copies. Production builds still emit maps as usual.
 */
function stripDevSourcemapsPlugin() {
  return {
    name: 'strip-dev-sourcemaps',
    apply: 'serve',
    enforce: 'post',
    transform(code, id) {
      const path = id.split('?')[0]
      if (
        path.endsWith('.css') ||
        path.endsWith('.scss') ||
        path.endsWith('.less')
      ) {
        return null
      }
      return { code, map: null }
    },
  }
}

/** Rewrite dynamic routes to catch-all files; leave the browser URL intact for React Router. */
function spaFallbackPlugin() {
  return {
    name: 'spa-fallback',
    enforce: 'pre', // SPA fallback must run before other middleware.
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        // Dev: document gets Permissions-Policy without going through Myriad proxy.
        if (!res.getHeader('Permissions-Policy')) {
          res.setHeader('Permissions-Policy', DOCUMENT_PERMISSIONS_POLICY)
        }

        const url = req.url || ''

        if (/^\/tapp\/run\/[^_/][^/]*/.test(url)) {
          req.url = '/tapp/run/_'
        } else if (/^\/tapp\/run(\?|$)/.test(url)) {
          req.url = '/tapp/run/_'
        } else if (/^\/tapp\/detail\/[^_/][^/]*/.test(url)) {
          req.url = '/tapp/detail/_'
        } else if (/^\/brew\/item\/[^/]+/.test(url)) {
          req.url = '/brew'
        } else if (/^\/federation\/chat\/[^_/][^/]*/.test(url)) {
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

// Must stay >= TappPlaygroundService AbortSignal (30m).
// Node http.request timeout is socket-idle; playground holds the connection
// with no response bytes until generation finishes.
const PLAYGROUND_PROXY_TIMEOUT_MS = 30 * 60 * 1000
// Federation file-meta downloads / chunk uploads can exceed the default 30s.
const FEDERATION_TRANSFER_PROXY_TIMEOUT_MS = 10 * 60 * 1000
// Keep names for tests; values live in aiRequestTimeout.mjs.
/* eslint-disable no-unused-vars, unused-imports/no-unused-vars -- contract aliases */
const MEROPE_PROXY_TIMEOUT_MS = AI_IMAGE_REQUEST_TIMEOUT_MS
const AGENT_PROCESS_PROXY_TIMEOUT_MS = AI_REQUEST_TIMEOUT_FLOOR_MS
/* eslint-enable no-unused-vars, unused-imports/no-unused-vars */

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

function isFederationTransferApiPath(urlPath) {
  const path = requestPathname(urlPath)
  if (path.startsWith('/api/federation/transfers/')) return true
  return (
    /^\/api\/federation\/channels\/[^/]+\/transfers$/.test(path) ||
    /^\/api\/federation\/rooms\/[^/]+\/transfers$/.test(path)
  )
}

async function readRequestBody(req) {
  const chunks = await Array.fromAsync(req, (chunk) =>
    Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk),
  )
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
        // Headers arrived: drop the idle timer for a small JSON body.
        backendReq.setTimeout(0)
        void Array.fromAsync(backendRes, (chunk) =>
          Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk),
        )
          .then((chunks) => {
            resolve({
              statusCode: backendRes.statusCode || 502,
              statusMessage: backendRes.statusMessage || 'Bad Gateway',
              headers: backendRes.headers,
              body: Buffer.concat(chunks),
            })
          })
          .catch(reject)
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
    'google-site-verification',
    'preview',
    'qq-url-preview',
    'dingtalkbot',
  ]
  if (markers.some((m) => s.includes(m))) return true
  return s.includes('bot/') || s.includes('spider') || s.includes('crawler')
}

function isInappShareUserAgent(ua) {
  const s = String(ua || '').toLowerCase()
  return (
    s.includes('micromessenger') ||
    s.includes('windowswechat') ||
    s.includes('wxwork') ||
    s.includes('weibo')
  )
}

function hasSpaBypass(urlPath) {
  const raw = String(urlPath || '')
  try {
    const url =
      raw.startsWith('http://') || raw.startsWith('https://')
        ? new URL(raw)
        : new URL(raw, 'http://dev.invalid')
    return url.searchParams.get('_spa') === '1'
  } catch {
    return /(?:^|[?&])_spa=1(?:&|$)/.test(raw)
  }
}

function wantsSeoHtmlShell(userAgent) {
  return isSeoCrawlerUserAgent(userAgent) || isInappShareUserAgent(userAgent)
}

/**
 * Paths that must hit the backend in dev, matching production proxy
 * `is_backend_path` in `proxy/src/main.rs`. Path-only (query stripped).
 * Does not proxy ACME under .well-known.
 *
 * This one-shot node:http proxy does not upgrade WebSockets. Federation WS
 * under /api/federation/.../ws is not available through the Astro dev proxy;
 * use a production-like proxy stack or hit backend:1103 directly for WS.
 *
 * @param {string} urlPath
 * @param {string} [userAgent]
 */
function isBackendDevProxyPath(urlPath, userAgent) {
  const path = requestPathname(urlPath)
  if (
    path.startsWith('/api/') ||
    path === '/health' ||
    path === '/ready' ||
    path === '/sitemap.xml' ||
    path === '/robots.txt' ||
    path === '/llms.txt'
  ) {
    return true
  }
  // Crawler HTML shells; humans stay on the SPA.
  const seoShellExact = new Set(['/', '/tapp', '/brew', '/library', '/reports'])
  if (
    (seoShellExact.has(path) ||
      path.startsWith('/tapp/run/') ||
      path.startsWith('/brew/item/')) &&
    wantsSeoHtmlShell(userAgent) &&
    !hasSpaBypass(urlPath)
  ) {
    return true
  }
  return (
    path === '/.well-known/webfinger' ||
    path === '/.well-known/nodeinfo' ||
    path === '/nodeinfo/2.1' ||
    path === '/inbox' ||
    path.startsWith('/users/') ||
    // Federation Note attachment media (must match proxy is_backend_path).
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
          const aiTimeoutMs = aiRequestTimeoutMs(requestPath)
          const timeoutMs = requestPath.startsWith('/api/tapp-playground/')
            ? PLAYGROUND_PROXY_TIMEOUT_MS
            : isFederationTransferApiPath(originalUrl) ||
                isFederationTransferContentPath(originalUrl)
              ? FEDERATION_TRANSFER_PROXY_TIMEOUT_MS
              : isAgentSsePath(originalUrl)
                ? PLAYGROUND_PROXY_TIMEOUT_MS
                : aiTimeoutMs ?? 30000
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
              // Content download: idle timeout 10m; SSE playground still uses 0.
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

      // Post-hook: run after Astro unshifts sec-fetch, then put the API proxy first.
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
 * Astro/Vite emits lazy-route CSS as HTML <link>, which blocks FCP.
 * Strip non-shell styles from HTML and inject them when the owning JS chunk runs.
 * Keep tailwind / index / App on the first paint.
 */
function deferNonCriticalCssIntegration() {
  /** @type {{ cssPrefix: string, jsPrefixes: string[] }[]} */
  const DEFER = [
    { cssPrefix: 'AraelPanel-', jsPrefixes: ['AraelPanel-'] },
    { cssPrefix: 'Config-', jsPrefixes: ['Config-'] },
    { cssPrefix: 'ConfigForm-', jsPrefixes: ['Config-'] },
    { cssPrefix: 'Setup-', jsPrefixes: ['Setup-'] },
    { cssPrefix: 'TappPlaygroundPage-', jsPrefixes: ['TappPlaygroundPage-'] },
    // Toast.css is owned by Toast.tsx; ToastContainer is sync in AppLayout, so
    // the chunk may be Toast-* or merged into App-*. Inject both (idempotent).
    { cssPrefix: 'Toast-', jsPrefixes: ['Toast-', 'App-'] },
    { cssPrefix: 'MusicPlayer-', jsPrefixes: ['MusicPlayer-'] },
  ]

  function cssInjectorSnippet(href) {
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

        /** @type {Map<string, string[]>} */
        const injectMap = new Map()
        /** @type {Set<string>} */
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

        for (const [jsName, hrefs] of injectMap) {
          const jsPath = path.join(assetsDir, jsName)
          const original = readFileSync(jsPath, 'utf8')
          if (
            hrefs.every(
              (h) =>
                original.includes(h) &&
                original.includes('createElement("link")'),
            )
          ) {
            // Vite may already inject; still prepend the idempotent snippet below.
          }
          const banner = hrefs.map(cssInjectorSnippet).join('')
          if (!original.startsWith('(function(){try{var h=')) {
            writeFileSync(jsPath, banner + original)
          }
        }

        const stripRe = new RegExp(
          `<link[^>]+href="/assets/(${[...stripCss]
            .map((s) => RegExp.escape(s))
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

export default defineConfig({
  integrations: [react(), deferNonCriticalCssIntegration()],
  output: 'static',
  server: {
    port: 1102,
    host: true,
  },
  build: {
    inlineStylesheets: 'auto',
    // Pair with trailingSlash: 'never' so we emit dist/setup.html, not
    // dist/setup/index.html. Directory output makes tower-http ServeDir 307
    // slashless directory requests to /setup/, which fights the no-trailing-slash
    // URL convention and loops /setup ↔ /setup/. file format has no directory, no 307.
    format: 'file',
  },
  // Astro Fonts API self-hosts these faces; no runtime Google Fonts CDN.
  fonts: [
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
    // Title faces, switched on demand by useTitleFont. Register only weights
    // that fonts.css .title-font-* actually uses.
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
  trailingSlash: 'never',
  // Highlighting is Prism on the React side, not Astro Markdown / Shiki.
  // Disable the default highlighter so non-overlay paths do not import('shiki/wasm').
  markdown: {
    syntaxHighlight: false,
  },
  vite: {
    define: {
      __APP_VERSION__: JSON.stringify(APP_VERSION),
    },
    css: {
      // Inline CSS maps are not needed for dest HMR and inflate the transform cache.
      devSourcemap: false,
    },
    optimizeDeps: {
      // Don't block first-paint on the full crawl. Default true waits for every
      // discovered dep; Agora ESM's ua-parser-js default-import then held
      // react.js forever and the PageLoader never dismissed.
      holdUntilCrawlEnd: false,
      // Mid-session discovery rewrites the dep browserHash. Vite then 504s
      // the old hash, and Astro's island retry of App.tsx dies with
      // "Outdated Optimize Dep" instead of a clean full reload.
      noDiscovery: true,
      include: [
        'axios',
        'isomorphic-dompurify',
        'jszip',
        'prismjs',
        'prismjs/components/prism-json',
        'react',
        'react-dom',
        'react-dom/client',
        'react-router-dom',
        // Not on the first-paint graph; still prebundle so mid-session discovery does not 504.
        'ag-psd',
        'motion/react',
        'pinyin-pro',
        // The default SDK entries are self-contained UMD, not browser ESM.
        // Prebundle them to expose exports; excluding them yields undefined
        // createClient / RTM in the browser. Do not use the optional ESM tree.
        'agora-rtc-sdk-ng',
        'agora-rtm',
      ],
      // Icon barrels are one file per pack (si ≈ 5MB). Prebundling them plus
      // an inline source map was a 13MB script on every page that imports
      // `@lib/icons`. Dest ESM + noDiscovery keeps them out of the shared
      // prebundle hash (no mid-session 504).
      // Agora's optional ESM tree must not enter the Vite prebundle.
      // Stay on the self-contained UMD entries; a failed ESM crawl holds every
      // optimized dep — the PageLoader never dismisses.
      exclude: [
        'lucide-react',
        'react-icons/bs',
        'react-icons/fa',
        'react-icons/fa6',
        'react-icons/lu',
        'react-icons/si',
        '@agora-js/shared',
        '@agora-js/media',
        '@agora-js/report',
        '@agora-js/protocol',
        'ua-parser-js',
        // Shiki's bundle-full does import('shiki/wasm'). Prebundling it lets
        // Vite's module-runner rewrite that specifier to a file path that Node
        // then cannot load under pnpm's isolated layout.
        'shiki',
      ],
    },
    ssr: {
      external: [
        'agora-rtc-sdk-ng',
        'agora-rtm',
        '@agora-js/shared',
        '@agora-js/media',
        '@agora-js/report',
        '@agora-js/protocol',
        // Keep the highlighter on Node's native ESM so import('shiki/wasm')
        // resolves from inside the shiki package, not the project root.
        'shiki',
      ],
    },
    plugins: [
      tailwindcss(),
      backendDevProxyPlugin(),
      spaFallbackPlugin(),
      stripDevSourcemapsPlugin(),
    ],
    resolve: {
      alias: {
        '@': path.resolve(__dirname, './src'),
        '@components': path.resolve(__dirname, './src/components'),
        '@layouts': path.resolve(__dirname, './src/layouts'),
        '@lib': path.resolve(__dirname, './src/lib'),
        '@config': path.resolve(__dirname, './src/config.ts'),
        '@shared': path.resolve(__dirname, '../shared'),
      },
    },
    environments: {
      client: {
        build: {
          target: 'es2025',
          rollupOptions: {
            output: {
              codeSplitting: {
                groups: [
                  {
                    // Claim React before other groups recursively capture their
                    // dependencies. A manualChunks name alone lets Motion take
                    // jsx-runtime and forces every JSX entry to load Motion.
                    name: 'react-vendor',
                    priority: 100,
                    test: (id) =>
                      id.includes('node_modules/react/') ||
                      id.includes('node_modules/react-dom/') ||
                      id.includes('node_modules/react-router') ||
                      id.includes('node_modules/@remix-run') ||
                      id.includes('jsx-runtime'),
                  },
                  {
                    name: (id) => {
                      if (
                        id.includes('node_modules/chart.js') ||
                        id.includes('node_modules/react-chartjs-2')
                      ) {
                        return 'chart-vendor'
                      }
                      // motion-dom / motion-utils are separate packages whose paths
                      // also contain node_modules/motion. Merging them into `motion`
                      // would make shared helpers a static dep of the 124K chunk and
                      // break lazyMotion's dynamic load.
                      if (id.includes('node_modules/motion-utils')) {
                        return 'motion-utils'
                      }
                      if (id.includes('node_modules/motion-dom')) {
                        return 'motion-dom'
                      }
                      if (id.includes('node_modules/motion')) {
                        return 'motion-vendor'
                      }
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
                      if (id.includes('node_modules/axios')) {
                        return 'axios'
                      }
                    },
                  },
                ],
              },
              chunkFileNames: 'assets/[name]-[hash].js',
              entryFileNames: 'assets/[name]-[hash].js',
              assetFileNames: 'assets/[name]-[hash].[ext]',
            },
          },
        },
      },
    },
    build: {
      target: 'es2025',
      cssCodeSplit: true,
      minify: 'terser',
      terserOptions: {
        compress: {
          // eslint-disable-next-line node/prefer-global/process
          drop_console: process.env.NODE_ENV === 'production',
          drop_debugger: true,
          passes: 2,
        },
      },
      assetsInlineLimit: 4096,
      reportCompressedSize: true,
      chunkSizeWarningLimit: 1000,
    },
  },
})
