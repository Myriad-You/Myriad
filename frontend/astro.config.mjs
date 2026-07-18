import { Buffer } from 'node:buffer'
import { readFileSync } from 'node:fs'
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
const APP_VERSION = pkg.version || '0.3.1'

/**
 * 自定义 Vite 插件：SPA 路由回退
 * 将动态路由（如 /tapp/run/:id）在服务端重定向到 catch-all 页面
 * 但保留原始 URL，让 React Router 在客户端正确解析参数
 */
function spaFallbackPlugin() {
  return {
    name: 'spa-fallback',
    enforce: 'pre', // 确保在其他中间件之前执行
    configureServer(server) {
      // 直接添加中间件，不返回函数
      server.middlewares.use((req, res, next) => {
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
  }
}

const BACKEND_TARGET = 'http://127.0.0.1:1103'

// Must stay >= TappPlaygroundService AbortSignal and cover planner + up to 3
// repair model calls (each may use backend MODEL_REQUEST_TIMEOUT of 720s).
// Node http.request timeout is socket-idle; playground holds the connection
// with no response bytes until generation finishes.
const PLAYGROUND_PROXY_TIMEOUT_MS = 20 * 60 * 1000

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
 * Dev-only backend proxy implemented with one-shot node:http requests.
 * This avoids Vite http-proxy and undici keep-alive socket reuse while
 * preserving same-origin API URLs during local development.
 */
function backendDevProxyPlugin() {
  return {
    name: 'backend-dev-proxy',
    apply: 'serve',
    enforce: 'pre',
    configureServer(server) {
      server.middlewares.use(async (req, res, next) => {
        const originalUrl = req.url || ''
        if (!originalUrl.startsWith('/api/') && originalUrl !== '/health') {
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
          const timeoutMs = originalUrl.startsWith('/api/tapp-playground/')
            ? PLAYGROUND_PROXY_TIMEOUT_MS
            : 30000
          // SSE must be piped. Buffering a long-lived EventSource response hits
          // the ordinary 30s proxy timeout and turns a healthy stream into 502.
          const streamResponse =
            headers.get('accept')?.toLowerCase().includes('text/event-stream') ||
            originalUrl.startsWith('/api/tapp-playground/generate-stream')

          if (streamResponse) {
            await proxyBackendRequestStreaming(
              targetUrl,
              method,
              headers,
              body,
              0,
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
      })
    },
  }
}

// https://astro.build/config
export default defineConfig({
  integrations: [react()],
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
    {
      provider: fontProviders.google(),
      name: 'Qwitcher Grypen',
      cssVariable: '--font-qwitcher-grypen',
      weights: [400, 700],
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
      weights: [400, 700],
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
      weights: [400, 700],
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
      weights: [400, 700],
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
      include: ['jszip'],
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
                  id.includes('node_modules/@remix-run')
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
                // Motion
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
