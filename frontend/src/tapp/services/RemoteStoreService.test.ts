/**
 * cd frontend && node --experimental-strip-types --test src/tapp/services/RemoteStoreService.test.ts
 */

import type {
  RemoteApp,
  RemoteStoreIndex,
  RemoteStoreSource,
} from './RemoteStoreService.ts'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { RemoteStoreService } from './RemoteStoreService.ts'

function previewApp(): RemoteApp {
  return {
    id: 'com.example.preview',
    name: 'Preview',
    version: '1.0.0',
    description: 'Static preview fixture',
    author: { name: 'Myriad' },
    category: 'utilities',
    permissions: [],
    download: {
      manifest: 'apps/com.example.preview/manifest.json',
      code: 'apps/com.example.preview/code.js',
      page_template: 'apps/com.example.preview/page.html',
    },
    preview: {
      version: 1,
      type: 'snapshot',
      html: 'apps/com.example.preview/preview.html',
      styles: [
        'apps/com.example.preview/page.css',
        'apps/com.example.preview/preview.css',
      ],
      viewport: { width: 1440, height: 900 },
      fit: 'cover',
      focus: { x: 0.5, y: 0.5 },
      theme: 'dark',
    },
  }
}

describe('RemoteStoreService.downloadAppPreview', () => {
  it('downloads declared snapshot paths instead of rendering the paths as content', async () => {
    const originalFetch = globalThis.fetch
    const requestedPaths: string[] = []
    globalThis.fetch = (async (input: string | URL | Request) => {
      const url = new URL(
        typeof input === 'string'
          ? input
          : input instanceof URL
            ? input.toString()
            : input.url,
      )
      requestedPaths.push(url.pathname)
      const fixtures: Record<string, string> = {
        '/store/apps/com.example.preview/preview.html':
          '<main>Rendered preview</main>',
        '/store/apps/com.example.preview/page.css': 'main { color: white; }',
        '/store/apps/com.example.preview/preview.css':
          'main { background: navy; }',
      }
      const body = fixtures[url.pathname]
      return new Response(body ?? 'missing', { status: body ? 200 : 404 })
    }) as typeof fetch

    try {
      const preview = await RemoteStoreService.downloadAppPreview(
        previewApp(),
        'https://store.example/store/',
      )

      assert.equal(preview.html, '<main>Rendered preview</main>')
      assert.equal(
        preview.css,
        'main { color: white; }\nmain { background: navy; }',
      )
      assert.deepEqual(requestedPaths.sort(), [
        '/store/apps/com.example.preview/page.css',
        '/store/apps/com.example.preview/preview.css',
        '/store/apps/com.example.preview/preview.html',
      ])
    } finally {
      globalThis.fetch = originalFetch
    }
  })

  it('downloads a locale-resolved snapshot instead of the default preview', async () => {
    const originalFetch = globalThis.fetch
    const requestedPaths: string[] = []
    globalThis.fetch = (async (input: string | URL | Request) => {
      const url = new URL(
        typeof input === 'string'
          ? input
          : input instanceof URL
            ? input.toString()
            : input.url,
      )
      requestedPaths.push(url.pathname)
      const fixtures: Record<string, string> = {
        '/store/apps/com.example.preview/preview.en-US.html':
          '<main>English preview</main>',
        '/store/apps/com.example.preview/preview.css': 'main { color: navy; }',
      }
      const body = fixtures[url.pathname]
      return new Response(body ?? 'missing', { status: body ? 200 : 404 })
    }) as typeof fetch

    try {
      const app = previewApp()
      const preview = await RemoteStoreService.downloadAppPreview(
        app,
        'https://store.example/store/',
        {
          ...app.preview!,
          html: 'apps/com.example.preview/preview.en-US.html',
          styles: ['apps/com.example.preview/preview.css'],
        },
      )
      assert.equal(preview.html, '<main>English preview</main>')
      assert.ok(
        requestedPaths.includes(
          '/store/apps/com.example.preview/preview.en-US.html',
        ),
      )
      assert.ok(
        !requestedPaths.some((path) => path.endsWith('/preview.html')),
      )
    } finally {
      globalThis.fetch = originalFetch
    }
  })

  it('falls back to page_template when no preview snapshot is declared', async () => {
    const originalFetch = globalThis.fetch
    const requestedPaths: string[] = []
    globalThis.fetch = (async (input: string | URL | Request) => {
      const url = new URL(
        typeof input === 'string'
          ? input
          : input instanceof URL
            ? input.toString()
            : input.url,
      )
      requestedPaths.push(url.pathname)
      const fixtures: Record<string, string> = {
        '/store/apps/com.example.preview/page.html': '<section>Page</section>',
        '/store/apps/com.example.preview/styles.css': 'section { color: red; }',
      }
      const body = fixtures[url.pathname]
      return new Response(body ?? 'missing', { status: body ? 200 : 404 })
    }) as typeof fetch

    try {
      const app = previewApp()
      delete app.preview
      app.download.styles = 'apps/com.example.preview/styles.css'
      const preview = await RemoteStoreService.downloadAppPreview(
        app,
        'https://store.example/store/',
      )
      assert.equal(preview.html, '<section>Page</section>')
      assert.equal(preview.css, 'section { color: red; }')
      assert.ok(
        requestedPaths.includes('/store/apps/com.example.preview/page.html'),
      )
      assert.ok(
        !requestedPaths.some((p) => p.includes('preview.html')),
      )
    } finally {
      globalThis.fetch = originalFetch
    }
  })

  it('returns empty when neither preview nor page_template exists', async () => {
    const app = previewApp()
    delete app.preview
    delete app.download.page_template
    const preview = await RemoteStoreService.downloadAppPreview(
      app,
      'https://store.example/store/',
    )
    assert.deepEqual(preview, {})
  })
})

describe('RemoteStoreService.fetchAllApps', () => {
  it('deduplicates ids by official source and then configured source order', async () => {
    const service = RemoteStoreService as unknown as {
      getEnabledSources: () => Promise<RemoteStoreSource[]>
      fetchStoreIndex: (
        source: RemoteStoreSource,
        forceRefresh?: boolean,
      ) => Promise<RemoteStoreIndex>
      fetchAllApps: typeof RemoteStoreService.fetchAllApps
    }
    const originalGetEnabledSources = service.getEnabledSources
    const originalFetchStoreIndex = service.fetchStoreIndex
    const sources: RemoteStoreSource[] = [
      {
        id: 2,
        name: 'First custom',
        url: 'https://first.example/index.json',
        enabled: true,
      },
      {
        id: 3,
        name: 'Second custom',
        url: 'https://second.example/index.json',
        enabled: true,
      },
      {
        id: 1,
        name: 'Official',
        url: 'https://official.example/index.json',
        enabled: true,
        official: true,
      },
    ]
    const catalog = (
      source: RemoteStoreSource,
      apps: RemoteApp[],
    ): RemoteStoreIndex => ({
      name: source.name,
      description: '',
      api_version: 1,
      last_updated: '2026-08-02T00:00:00Z',
      base_url: './',
      apps,
    })
    const app = (id: string, name: string): RemoteApp => ({
      ...previewApp(),
      id,
      name,
    })

    service.getEnabledSources = async () => sources
    service.fetchStoreIndex = async (source) => {
      if (source.name === 'First custom') {
        await new Promise((resolve) => setTimeout(resolve, 15))
        return catalog(source, [
          app('com.example.shared', 'Custom shared'),
          app('com.example.custom-shared', 'First custom copy'),
        ])
      }
      if (source.name === 'Second custom') {
        return catalog(source, [
          app('com.example.custom-shared', 'Second custom copy'),
        ])
      }
      return catalog(source, [app('com.example.shared', 'Official copy')])
    }

    try {
      const result = await service.fetchAllApps()
      assert.deepEqual(
        result.apps.map(({ id, name, sourceName }) => ({
          id,
          name,
          sourceName,
        })),
        [
          {
            id: 'com.example.shared',
            name: 'Official copy',
            sourceName: 'Official',
          },
          {
            id: 'com.example.custom-shared',
            name: 'First custom copy',
            sourceName: 'First custom',
          },
        ],
      )
    } finally {
      service.getEnabledSources = originalGetEnabledSources
      service.fetchStoreIndex = originalFetchStoreIndex
    }
  })
})
