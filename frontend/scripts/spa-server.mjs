/**
 * Production SPA server: static files + fallback, stamps site identity.
 *  Proxy stays a dumb hop. Crawler / share HTML is backend SEO, not this process.
 */
import { Buffer } from 'node:buffer'
import { createReadStream, existsSync, statSync } from 'node:fs'
import { readFile } from 'node:fs/promises'
import http from 'node:http'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'
import zlib from 'node:zlib'

import { isClientAbortError } from './devServerResponse.mjs'
import {
  createBrandLoader,
  stampDocumentHtml,
  stampWebManifest,
} from './siteBrandingStamp.mjs'
import { STATIC_ASSET_PATTERN as ASSET_EXT, DOCUMENT_PERMISSIONS_POLICY } from './vite/constants.mjs'

const HERE = path.dirname(fileURLToPath(import.meta.url))
const DIST = path.resolve(process.env.DIST_DIR || path.join(HERE, '..', 'dist'))
const PORT = Number(process.env.PORT || 1102)
const HOST = process.env.HOST || '0.0.0.0'
const INDEX = path.join(DIST, 'index.html')
const MANIFEST_NAME = 'manifest.webmanifest'

const MIME = new Map([
  ['.html', 'text/html; charset=utf-8'],
  ['.js', 'text/javascript; charset=utf-8'],
  ['.mjs', 'text/javascript; charset=utf-8'],
  ['.css', 'text/css; charset=utf-8'],
  ['.json', 'application/json; charset=utf-8'],
  ['.webmanifest', 'application/manifest+json; charset=utf-8'],
  ['.map', 'application/json'],
  ['.webp', 'image/webp'],
  ['.png', 'image/png'],
  ['.jpg', 'image/jpeg'],
  ['.jpeg', 'image/jpeg'],
  ['.gif', 'image/gif'],
  ['.svg', 'image/svg+xml'],
  ['.ico', 'image/x-icon'],
  ['.woff', 'font/woff'],
  ['.woff2', 'font/woff2'],
  ['.ttf', 'font/ttf'],
  ['.wasm', 'application/wasm'],
])

const loadBrand = createBrandLoader(process.env.BRANDING_METADATA_URL || '')

function requestPathname(rawUrl) {
  const pathOnly = String(rawUrl || '/').split('?')[0]
  try {
    return decodeURIComponent(pathOnly)
  } catch {
    return null
  }
}

function resolveUnderDist(urlPath) {
  if (urlPath == null || urlPath.includes('\0')) return null
  const rel = urlPath.replace(/^\/+/, '')
  if (!rel || rel === '.') return { abs: INDEX, rel: 'index.html' }
  const abs = path.resolve(DIST, rel)
  const relToDist = path.relative(DIST, abs)
  if (relToDist.startsWith('..') || path.isAbsolute(relToDist)) return null
  return { abs, rel: relToDist }
}

function lookup(urlPath) {
  const located = resolveUnderDist(urlPath)
  if (!located) return { status: 400 }

  const tryFile = (abs, rel) => {
    try {
      const st = statSync(abs)
      if (st.isFile()) {
        return {
          status: 200,
          file: abs,
          rel,
          stampHtml: abs.endsWith('.html'),
          stampManifest: rel === MANIFEST_NAME,
        }
      }
      if (st.isDirectory()) {
        const nested = path.join(abs, 'index.html')
        if (existsSync(nested) && statSync(nested).isFile()) {
          return {
            status: 200,
            file: nested,
            rel: path.posix.join(rel.replaceAll('\\', '/'), 'index.html'),
            stampHtml: true,
            stampManifest: false,
          }
        }
      }
    } catch {
      // missing
    }
    return null
  }

  const direct = tryFile(located.abs, located.rel)
  if (direct) return direct

  if (!path.extname(located.abs)) {
    const htmlRel = `${located.rel}.html`
    const htmlAbs = `${located.abs}.html`
    const asHtml = tryFile(htmlAbs, htmlRel)
    if (asHtml) return asHtml
  }

  if (ASSET_EXT.test(urlPath)) return { status: 404 }
  if (!existsSync(INDEX)) return { status: 404 }
  return {
    status: 200,
    file: INDEX,
    rel: 'index.html',
    stampHtml: true,
    stampManifest: false,
  }
}

function cacheControl(urlPath) {
  if (
    urlPath.startsWith('/assets/') ||
    /^\/fonts\/site\/[^/]+-[a-f0-9]{16}\.woff2$/.test(urlPath)
  ) {
    return 'public, max-age=31536000, immutable'
  }
  if (
    urlPath === '/sw.js' ||
    urlPath === `/${MANIFEST_NAME}` ||
    urlPath === '/' ||
    urlPath.endsWith('.html')
  ) {
    return 'no-cache'
  }
  if (
    urlPath.startsWith('/icons/') ||
    urlPath.startsWith('/game-logos/') ||
    urlPath.startsWith('/fonts/') ||
    /\.(?:webp|png|jpe?g|gif|svg|avif|ico|woff2?)$/i.test(urlPath)
  ) {
    return 'public, max-age=604800, stale-while-revalidate=86400'
  }
  return 'no-cache'
}

// Variants emitted by scripts/vite/precompress.mjs. dist is immutable for
// the life of the process, so each file is probed once.
const ENCODINGS = [
  { token: 'br', ext: '.br' },
  { token: 'gzip', ext: '.gz' },
]
const variantCache = new Map()

function variantsOf(file) {
  let found = variantCache.get(file)
  if (!found) {
    found = ENCODINGS.flatMap(({ token, ext }) => {
      try {
        const st = statSync(file + ext)
        return st.isFile() ? [{ token, file: file + ext, size: st.size }] : []
      } catch {
        return []
      }
    })
    variantCache.set(file, found)
  }
  return found
}

/** Encodings the client accepts (q > 0), in server preference order. */
function acceptedEncodings(header) {
  const accepted = new Set()
  for (const part of String(header || '').split(',')) {
    const [token, ...params] = part.trim().toLowerCase().split(';')
    const q = params.map((p) => p.trim()).find((p) => p.startsWith('q='))
    if (token && !(q && Number(q.slice(2)) === 0)) accepted.add(token)
  }
  return ENCODINGS.map(({ token }) => token).filter((token) => accepted.has(token))
}

/** Stamped HTML is per-request and small; fast levels keep it sub-millisecond. */
function compressDynamic(body, token) {
  if (token === 'br') {
    return zlib.brotliCompressSync(body, {
      params: { [zlib.constants.BROTLI_PARAM_QUALITY]: 5 },
    })
  }
  return zlib.gzipSync(body, { level: 6 })
}

function contentType(file) {
  return MIME.get(path.extname(file).toLowerCase()) || 'application/octet-stream'
}

function send(res, { status, headers, body, method }) {
  res.writeHead(status, headers)
  if (method === 'HEAD' || body == null) {
    res.end()
    return
  }
  res.end(body)
}

async function handle(req, res) {
  const method = req.method || 'GET'
  if (method !== 'GET' && method !== 'HEAD') {
    send(res, {
      status: 405,
      headers: { Allow: 'GET, HEAD' },
      method,
    })
    return
  }

  const urlPath = requestPathname(req.url)
  if (urlPath == null) {
    send(res, { status: 400, headers: {}, method })
    return
  }

  const found = lookup(urlPath)
  if (found.status !== 200) {
    send(res, { status: found.status, headers: { 'Cache-Control': 'no-cache' }, method })
    return
  }

  const headers = {
    'Content-Type': contentType(found.file),
    'Cache-Control': cacheControl(urlPath),
    'X-Content-Type-Options': 'nosniff',
  }

  if (found.stampHtml || found.stampManifest) {
    const raw = await readFile(found.file)
    const brand = await loadBrand()
    let body = raw
    if (found.stampHtml) {
      headers['Permissions-Policy'] = DOCUMENT_PERMISSIONS_POLICY
      if (brand) body = Buffer.from(stampDocumentHtml(raw.toString('utf8'), brand))
    } else if (found.stampManifest && brand) {
      const parsed = JSON.parse(raw.toString('utf8'))
      body = Buffer.from(JSON.stringify(stampWebManifest(parsed, brand)))
    }
    headers.Vary = 'Accept-Encoding'
    const [token] = acceptedEncodings(req.headers['accept-encoding'])
    if (token) {
      body = compressDynamic(body, token)
      headers['Content-Encoding'] = token
    }
    headers['Content-Length'] = body.length
    send(res, { status: 200, headers, body, method })
    return
  }

  let file = found.file
  let { size } = statSync(file)
  const variants = variantsOf(file)
  if (variants.length > 0) {
    headers.Vary = 'Accept-Encoding'
    const accepted = acceptedEncodings(req.headers['accept-encoding'])
    const variant = variants.find(({ token }) => accepted.includes(token))
    if (variant) {
      file = variant.file
      size = variant.size
      headers['Content-Encoding'] = variant.token
    }
  }
  headers['Content-Length'] = size
  res.writeHead(200, headers)
  if (method === 'HEAD') {
    res.end()
    return
  }
  createReadStream(file).pipe(res)
}

if (!existsSync(INDEX)) {
  console.error(`[spa-server] missing ${INDEX}`)
  process.exit(1)
}

const server = http.createServer((req, res) => {
  handle(req, res).catch((error) => {
    if (isClientAbortError(error) || res.writableEnded || res.destroyed) return
    console.error('[spa-server]', error)
    if (!res.headersSent) {
      res.writeHead(500)
    }
    if (!res.writableEnded) res.end()
  })
})

void loadBrand()

server.listen(PORT, HOST, () => {
  console.log(`[spa-server] ${HOST}:${PORT} → ${DIST}`)
})
