import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import { DOCUMENT_PERMISSIONS_POLICY, isRetiredPath } from '../scripts/vite/constants.mjs'
import { spaFallbackPlugin } from '../scripts/vite/spaFallback.mjs'

describe('SPA document middleware', () => {
  it('leaves routing and queries to Vite and React Router in dev and preview', () => {
    const plugin = spaFallbackPlugin()
    for (const attach of [
      plugin.configureServer,
      plugin.configurePreviewServer,
    ]) {
      let handler: any
      attach({
        config: { root: '/tmp', publicDir: '/tmp', build: { outDir: 'dist' } },
        middlewares: {
          use(fn: any) {
            handler = fn
          },
        },
      })
      for (const url of [
        '/tapp/run/abc?x=1',
        '/studio/notes?keep=1',
        '/agent/settings',
        '/api/config/ui',
      ]) {
        const req = { url }
        const headers = new Map()
        let next = false
        handler(
          req,
          {
            getHeader: (key: string) => headers.get(key),
            setHeader: (key: string, value: string) => headers.set(key, value),
          },
          () => {
            next = true
          },
        )
        assert.equal(req.url, url)
        assert.equal(
          headers.get('Permissions-Policy'),
          DOCUMENT_PERMISSIONS_POLICY,
        )
        assert.equal(next, true)
      }
    }
    assert.match(
      readFileSync(new URL('../vite.config.mjs', import.meta.url), 'utf8'),
      /appType: 'spa'/,
    )
  })

  it('answers retired pages with 410 and keeps embed pages out of the index', () => {
    let handler: any
    spaFallbackPlugin().configureServer({
      config: { root: '/tmp', publicDir: '/tmp' },
      middlewares: {
        use(fn: any) {
          handler = fn
        },
      },
    })
    const run = (url: string) => {
      const headers = new Map()
      const res: any = {
        statusCode: 200,
        body: undefined as string | undefined,
        getHeader: (key: string) => headers.get(key),
        setHeader: (key: string, value: string) => headers.set(key, value),
        end(body?: string) {
          res.body = body
        },
      }
      let next = false
      handler({ url, method: 'GET' }, res, () => {
        next = true
      })
      return { status: res.statusCode, headers, next, body: res.body }
    }

    for (const url of ['/journal', '/journal/articles/42', '/reports', '/reports/latest?x=1']) {
      const out = run(url)
      assert.equal(out.status, 410, url)
      assert.equal(out.next, false, url)
      assert.equal(out.headers.get('X-Robots-Tag'), 'noindex', url)
      assert.match(out.body ?? '', /noindex/)
    }
    for (const url of ['/journalist', '/reportsx', '/blog/', '/']) {
      assert.equal(isRetiredPath(url.split('?')[0]), false, url)
    }
    const embed = run('/embed/comments/42')
    assert.equal(embed.next, true)
    assert.equal(embed.headers.get('X-Robots-Tag'), 'noindex')
  })
})
