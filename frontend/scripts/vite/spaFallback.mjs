import { statSync } from 'node:fs'
import path from 'node:path'
import {
  DOCUMENT_PERMISSIONS_POLICY,
  EMBED_DOCUMENT,
  EMBED_FRAME_HEADERS,
  isEmbedDocumentPath,
  STATIC_ASSET_PATTERN,
} from './constants.mjs'

/** Document policy and missing-asset guard; Vite owns history fallback. */
export function spaFallbackPlugin() {
  function attach(server, roots) {
    server.middlewares.use((req, res, next) => {
      if (!res.getHeader('Permissions-Policy')) {
        res.setHeader('Permissions-Policy', DOCUMENT_PERMISSIONS_POLICY)
      }
      let pathname
      try { pathname = decodeURIComponent((req.url || '').split('?')[0]) } catch { next(); return }
      if (isEmbedDocumentPath(pathname)) {
        for (const [name, value] of Object.entries(EMBED_FRAME_HEADERS)) res.setHeader(name, value)
      }
      // 嵌入页换成它自己的文档；Vite 照常处理这份 HTML，地址栏不变。
      if (isEmbedDocumentPath(pathname) && (req.method === 'GET' || req.method === 'HEAD')) {
        const query = (req.url || '').includes('?') ? (req.url || '').slice((req.url || '').indexOf('?')) : ''
        req.url = `/${EMBED_DOCUMENT}${query}`
        next()
        return
      }
      // Transformed modules belong to Vite, including imports outside the root.
      if (/^\/(?:@|src\/|node_modules\/)/.test(pathname) || !STATIC_ASSET_PATTERN.test(pathname)) {
        next()
        return
      }
      const exists = roots.some(root => {
        const file = path.resolve(root, `.${pathname}`)
        const relative = path.relative(root, file)
        if (relative.startsWith('..') || path.isAbsolute(relative)) return false
        try { return statSync(file).isFile() } catch { return false }
      })
      if (!exists) {
        res.statusCode = 404
        res.setHeader('Cache-Control', 'no-cache')
        res.end()
        return
      }
      next()
    })
  }
  return {
    name: 'spa-document-policy',
    configureServer(server) {
      attach(server, [server.config.root, server.config.publicDir].filter(Boolean))
    },
    configurePreviewServer(server) {
      attach(server, [path.resolve(server.config.root, server.config.build.outDir)])
    },
  }
}
