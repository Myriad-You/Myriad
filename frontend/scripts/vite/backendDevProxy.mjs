import { Buffer } from 'node:buffer'
import http from 'node:http'
import {
  AI_IMAGE_REQUEST_TIMEOUT_MS,
  AI_REQUEST_TIMEOUT_FLOOR_MS,
  aiRequestTimeoutMs,
} from '../../src/utils/aiRequestTimeout.mjs'
import {
  isBackendUnreachableError,
  isClientAbortError,
  shouldRetryBackendProxy,
  writeDevProxyFailure,
} from '../devServerResponse.mjs'
import { BACKEND_TARGET } from './constants.mjs'
import {
  hasSpaBypass,
  isSeoDocumentShellPath,
  wantsSeoHtmlShell,
} from './seoShell.mjs'

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

/**
 * Paths that must hit the backend in dev, matching production proxy
 * `is_backend_path` in `proxy/src/main.rs`. Path-only (query stripped).
 * Does not proxy ACME under .well-known.
 *
 * This one-shot node:http proxy does not upgrade WebSockets. Federation WS
 * under /api/federation/.../ws is not available through the Vite dev proxy;
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
    path === '/llms.txt' ||
    path === '/journal/notes.xml'
  ) {
    return true
  }
  // Crawler HTML shells; humans stay on the SPA.
  if (
    isSeoDocumentShellPath(path) &&
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
    // Public site media: uploads, portraits and stickers.
    path.startsWith('/media/assets/')
  )
}

/**
 * Dev-only backend proxy implemented with one-shot node:http requests.
 * This avoids Vite http-proxy and undici keep-alive socket reuse while
 * preserving same-origin API URLs during local development.
 *
 * Install routing ahead of HTML transforms, branding and SPA fallback so
 * crawler documents, streams and opaque-origin Tapp requests reach backend.
 */
export function backendDevProxyPlugin({ backendTarget = BACKEND_TARGET } = {}) {
  return {
    name: 'backend-dev-proxy',
    apply: 'serve',
    configureServer(server) {
      const handler = async (req, res, next) => {
        const originalUrl = req.originalUrl || req.url || ''
        const ua = req.headers['user-agent'] || ''
        if (!isBackendDevProxyPath(originalUrl, ua)) {
          next()
          return
        }

        try {
          const targetUrl = new URL(originalUrl, backendTarget)
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
              if (
                !shouldRetryBackendProxy(error, {
                  retryable,
                  attempt,
                  maxAttempts: 3,
                })
              ) {
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
          if (
            isClientAbortError(error) ||
            req.aborted ||
            req.destroyed ||
            res.writableEnded ||
            res.destroyed
          ) {
            return
          }
          const detail =
            error instanceof Error
              ? isBackendUnreachableError(error)
                ? error.message
                : `${error.message}\n${error.stack || ''}`
              : String(error)
          const log = isBackendUnreachableError(error)
            ? server.config.logger.warn.bind(server.config.logger)
            : server.config.logger.error.bind(server.config.logger)
          log(
            `[backend-dev-proxy] ${req.method || 'GET'} ${originalUrl} failed: ${detail}`,
          )
          writeDevProxyFailure(res, error)
        }
      }

      // Post-hook: keep backend routing ahead of document middleware.
      return () => {
        server.middlewares.stack.unshift({
          route: '',
          handle: handler,
        })
      }
    },
  }
}
