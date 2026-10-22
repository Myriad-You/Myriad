import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import {
  hasSpaBypass,
  isInappShareUserAgent,
  isSeoCrawlerUserAgent,
  isSeoDocumentShellPath,
  SEO_CRAWLER_MARKERS,
  SEO_SHELL_EXACT,
  SEO_SHELL_PREFIXES,
  wantsSeoHtmlShell,
} from '../scripts/vite/seoShell.mjs'

function rustSeoCrawlerMarkers(source: string): string[] {
  const start = source.indexOf('const MARKERS: &[&str] = &[')
  const end = source.indexOf('];', start)
  return [...source.slice(start, end).matchAll(/"([^"]+)"/g)].map(
    (match) => match[1],
  )
}

const proxySource = readFileSync(
  new URL('../../proxy/src/main.rs', import.meta.url),
  'utf8',
)
const destProxySource = readFileSync(
  new URL('../scripts/vite/backendDevProxy.mjs', import.meta.url),
  'utf8',
)
const spaServerSource = readFileSync(
  new URL('../scripts/spa-server.mjs', import.meta.url),
  'utf8',
)

describe('seoShell', () => {
  it('is the dest copy of proxy crawler-shell paths', () => {
    assert.match(destProxySource, /from ['"]\.\/seoShell\.mjs['"]/)
    assert.doesNotMatch(destProxySource, /seoShellExact/)
    assert.match(proxySource, /fn is_seo_document_shell_path/)
    for (const path of SEO_SHELL_EXACT) {
      assert.match(proxySource, new RegExp(`"${path}"`))
    }
    for (const prefix of SEO_SHELL_PREFIXES) {
      assert.match(
        proxySource,
        new RegExp(`starts_with\\("${prefix.replaceAll('/', '\\/')}"\\)`),
      )
    }
    assert.equal(isSeoDocumentShellPath('/'), true)
    assert.equal(isSeoDocumentShellPath('/tapp/run/com.example'), true)
    assert.equal(isSeoDocumentShellPath('/tapp/store'), false)
  })

  it('keeps dest crawler and in-app share detection on the extracted table', () => {
    assert.deepEqual(SEO_CRAWLER_MARKERS, rustSeoCrawlerMarkers(proxySource))
    assert.equal(isSeoCrawlerUserAgent('Googlebot/2.1'), true)
    assert.equal(isSeoCrawlerUserAgent('Bytespider'), true)
    assert.equal(isSeoCrawlerUserAgent('claude-web anthropic-ai'), true)
    assert.equal(
      isSeoCrawlerUserAgent('Mozilla/5.0 (compatible; archive.org_bot +http://archive.org/details/archive.org_bot)'),
      true,
    )
    assert.equal(wantsSeoHtmlShell('facebookexternalhit/1.1'), true)
    assert.equal(isInappShareUserAgent('Mozilla/5.0 MicroMessenger'), true)
    assert.equal(isSeoCrawlerUserAgent('Mozilla/5.0'), false)
    assert.equal(hasSpaBypass('/tapp?_spa=1'), true)
    assert.equal(hasSpaBypass('/tapp'), false)
  })

  it('lets prod spa-server share Permissions-Policy with dest', () => {
    assert.match(spaServerSource, /from ['"]\.\/vite\/constants\.mjs['"]/)
    assert.doesNotMatch(
      spaServerSource,
      /const DOCUMENT_PERMISSIONS_POLICY/,
    )
  })
})
