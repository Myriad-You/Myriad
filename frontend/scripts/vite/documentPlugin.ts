import type { Plugin } from 'vite'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import en from '../../src/i18n/en-US.json' with { type: 'json' }
import { localeLangInlineScript } from '../../src/i18n/localeLangScript.ts'
import { siteBrandingInlineScript } from '../../src/utils/siteBrandingScript.ts'
import { siteTransitionInlineScript } from '../../src/utils/siteTransitionScript.ts'
import { themeBootInlineScript } from '../../src/utils/themeBootScript.ts'
import { siteFontHead } from '../siteFonts.mjs'

function escapeHtml(value: string) {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('"', '&quot;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
}
function script(value: string) {
  return `<script>${value.replaceAll('</script', '<\\/script')}</script>`
}
function source(relative: string) {
  return readFileSync(new URL(relative, import.meta.url), 'utf8')
}

export function renderDocument(html: string, env: Record<string, string> = {}) {
  const version = env.PUBLIC_MYRIAD_VERSION || 'dev'
  const commit = env.PUBLIC_MYRIAD_COMMIT_SHA || ''
  const fonts = siteFontHead()
  const metadata = {
    '@context': 'https://schema.org',
    '@type': 'WebApplication',
    name: 'Myriad',
    description: en.chrome.description,
    applicationCategory: 'LifestyleApplication',
    operatingSystem: 'Any',
    isAccessibleForFree: true,
    ...(version !== 'dev'
      ? { softwareVersion: version.replace(/^v/, '') }
      : {}),
  }
  const slots: Record<string, string> = {
    '%DOCUMENT_TITLE%': escapeHtml(en.chrome.title),
    '%DOCUMENT_DESCRIPTION%': escapeHtml(en.chrome.description),
    '%DOCUMENT_NOSCRIPT%': escapeHtml(en.chrome.noscript),
    '%DOCUMENT_VERSION%': escapeHtml(version),
    '%DOCUMENT_COMMIT%': escapeHtml(commit),
    '<!-- boot:locale -->': script(
      localeLangInlineScript(source('../../src/i18n/parseLocale.shared.js')),
    ),
    '<!-- boot:spa-query -->': script(`;(function () { try {
      const u = new URL(location.href)
      if (u.searchParams.get('_spa') !== '1') return
      u.searchParams.delete('_spa')
      const q = u.searchParams.toString()
      history.replaceState(null, '', u.pathname + (q ? '?' + q : '') + u.hash)
    } catch {} })()`),
    '<!-- boot:brand -->': script(siteBrandingInlineScript()),
    '<!-- boot:theme -->': script(themeBootInlineScript()),
    '<!-- boot:transition -->': script(siteTransitionInlineScript()),
    '<!-- document:json-ld -->': `<script type="application/ld+json" data-myriad-brand="json-ld">${JSON.stringify(metadata).replaceAll('<', '\\u003c')}</script>`,
    '<!-- document:fonts -->': `<style>${fonts.css}</style>${fonts.preloads.map((href) => `<link rel="preload" href="${href}" as="font" type="font/woff2" crossorigin>`).join('')}`,
    '<!-- document:styles -->': `<style>${source('../../src/styles/spa-document.css')}\n${source('../../src/styles/page-loader.css')}\n${source('../../src/styles/site-transition.css')}</style>`,
  }
  for (const [slot, value] of Object.entries(slots))
    html = html.replaceAll(slot, () => value)
  return html
}

const documentFiles = ['../../src/styles/spa-document.css', '../../src/styles/page-loader.css', '../../src/styles/site-transition.css', '../../src/i18n/parseLocale.shared.js'].map(relative => fileURLToPath(new URL(relative, import.meta.url)))

export function documentPlugin(): Plugin {
  let env: Record<string, string>
  return {
    name: 'spa-document',
    configureServer(server) { server.watcher.add(documentFiles) },
    handleHotUpdate(context) {
      if (documentFiles.includes(context.file)) {
        context.server.ws.send({ type: 'full-reload' })
        return []
      }
    },
    configResolved(config) {
      env = config.env
    },
    transformIndexHtml: {
      order: 'pre',
      handler(html) {
        return renderDocument(html, env)
      },
    },
  }
}
