import type { TappManifest } from '../types'
import type { RemoteApp, RemoteStoreIndex } from './RemoteStoreService'
import assert from 'node:assert/strict'
import { afterEach, beforeEach, describe, it } from 'node:test'
import RemoteStoreService from './RemoteStoreService.ts'
import {
  installFromStore,
  storeInstallModules,
  storeWidgetStyles,
  updateTappFromStore,
} from './TappInstallationApi.ts'

const originalFetch = globalThis.fetch
const originalRemoteMethods = {
  getSources: RemoteStoreService.getSources,
  clearCache: RemoteStoreService.clearCache,
  fetchStoreIndex: RemoteStoreService.fetchStoreIndex,
  downloadAppPackage: RemoteStoreService.downloadAppPackage,
}

class MemoryStorage implements Storage {
  private readonly values = new Map<string, string>()

  get length(): number {
    return this.values.size
  }

  clear(): void {
    this.values.clear()
  }

  getItem(key: string): string | null {
    return this.values.get(key) ?? null
  }

  key(index: number): string | null {
    return Iterator.from(this.values.keys()).toArray()[index] ?? null
  }

  removeItem(key: string): void {
    this.values.delete(key)
  }

  setItem(key: string, value: string): void {
    this.values.set(key, value)
  }
}

function seedCsrfToken(): void {
  sessionStorage.setItem(
    'csrf_token',
    `v1.${'a'.repeat(100)}.${'b'.repeat(43)}`,
  )
  sessionStorage.setItem('csrf_token_stored_at', String(Date.now()))
  sessionStorage.setItem('csrf_token_expires_at', String(Date.now() + 60_000))
}

function requestBody(init?: RequestInit): Record<string, unknown> {
  assert.equal(typeof init?.body, 'string')
  return JSON.parse(init.body) as Record<string, unknown>
}

beforeEach(() => {
  Object.defineProperty(globalThis, 'sessionStorage', {
    value: new MemoryStorage(),
    configurable: true,
    writable: true,
  })
  seedCsrfToken()
})

afterEach(() => {
  globalThis.fetch = originalFetch
  RemoteStoreService.getSources = originalRemoteMethods.getSources
  RemoteStoreService.clearCache = originalRemoteMethods.clearCache
  RemoteStoreService.fetchStoreIndex = originalRemoteMethods.fetchStoreIndex
  RemoteStoreService.downloadAppPackage =
    originalRemoteMethods.downloadAppPackage
  Reflect.deleteProperty(globalThis, 'sessionStorage')
})

describe('Tapp store transport strategy', () => {
  it('starts large installs with a metadata-only backend store request', async () => {
    const bodies: Record<string, unknown>[] = []
    globalThis.fetch = async (_input, init) => {
      bodies.push(requestBody(init))
      return Response.json({ success: true, data: { id: 'com.example.large' } })
    }

    await installFromStore(
      {
        source: 'https://store.example/index.json',
        tappId: 'com.example.large',
        permissions: ['storage:read', 'storage:write'],
      },
      { estimatedBytes: 4 * 1024 * 1024 },
    )

    assert.equal(bodies.length, 1)
    assert.deepEqual(bodies[0], {
      source: 'store',
      storeSource: 'https://store.example/index.json',
      tappId: 'com.example.large',
      permissions: ['storage:read', 'storage:write'],
    })
  })

  it('starts large updates with a metadata-only backend store request', async () => {
    const calls: Array<{ url: string; body: Record<string, unknown> }> = []
    globalThis.fetch = async (input, init) => {
      calls.push({ url: String(input), body: requestBody(init) })
      return Response.json({ success: true, data: { id: 'com.example.large' } })
    }

    await updateTappFromStore(
      'com.example.large',
      {
        source: 'https://store.example/index.json',
        permissions: ['storage:read', 'storage:write'],
      },
      { estimatedBytes: 4 * 1024 * 1024 },
    )

    assert.equal(calls.length, 1)
    assert.equal(calls[0]?.url, '/api/tapps/com.example.large/update')
    assert.deepEqual(calls[0]?.body, {
      source: 'store',
      storeSource: 'https://store.example/index.json',
      permissions: ['storage:read', 'storage:write'],
    })
  })

  it('uses the browser package proxy only after a backend 502', async () => {
    const manifest = {
      id: 'com.example.fallback',
      name: 'Fallback',
      version: '1.0.0',
      core: { entry: 'core.js' },
      widgets: [
        {
          id: 'card',
          name: 'Card',
          defaultSize: '2x2',
          sizes: ['2x2'],
          category: 'utility',
          entry: 'widget/index.js',
          styles: 'widget.css',
        },
      ],
      permissions: ['storage:read', 'storage:write', 'widget:register'],
      category: 'utility',
    } as TappManifest
    const app = {
      id: manifest.id,
      name: manifest.name,
      version: manifest.version,
      description: '',
      author: { name: 'Example' },
      category: 'utility',
      permissions: manifest.permissions,
      download: {
        manifest: 'manifest.json',
        code: 'core.js',
        modules: { 'widget/index.js': 'widget/index.js' },
        widget_styles: 'widget.css',
      },
      size: 4 * 1024 * 1024,
    } as RemoteApp
    const index: RemoteStoreIndex = {
      name: 'Example',
      description: '',
      api_version: 2,
      last_updated: '2026-08-03T00:00:00Z',
      base_url: 'https://store.example/',
      apps: [app],
    }

    RemoteStoreService.getSources = async () => [
      {
        id: 7,
        name: 'Example',
        url: 'https://store.example/index.json',
        enabled: true,
      },
    ]
    RemoteStoreService.clearCache = () => {}
    RemoteStoreService.fetchStoreIndex = async () => index
    RemoteStoreService.downloadAppPackage = async () => ({
      manifest,
      code: 'module.exports = {};',
      modules: { 'widget/index.js': 'require("../core.js");' },
      widgetStyles: '.card{}',
    })

    const sources: string[] = []
    const directBodies: Record<string, unknown>[] = []
    globalThis.fetch = async (_input, init) => {
      const body = requestBody(init)
      sources.push(String(body.source))
      if (body.source === 'store') {
        return Response.json(
          { error: 'Upstream fetch failed' },
          { status: 502 },
        )
      }
      directBodies.push(body)
      return Response.json({ success: true, data: { id: manifest.id } })
    }

    await installFromStore(
      {
        source: '7',
        tappId: manifest.id,
        permissions: manifest.permissions,
      },
      { estimatedBytes: app.size },
    )

    assert.deepEqual(sources, ['store', 'direct'])

    // 回退路径必须按层入口交付模块，作者 CSS 走 widgetStyles。
    const direct = directBodies[0]!
    assert.deepEqual(direct.modules, {
      'core.js': 'module.exports = {};',
      'widget/index.js': 'require("../core.js");',
    })
    assert.deepEqual(direct.widgetStyles, { card: '.card{}' })
    assert.equal(Object.hasOwn(direct, 'widgetCss'), false)
  })
})

describe('store package payload shaping', () => {
  const layered: TappManifest = {
    id: 'com.example.layers',
    name: 'Layered',
    version: '1.0.0',
    category: 'utility',
    permissions: ['widget:register'],
    core: { entry: 'core.js' },
    page: { entry: 'page/index.js' },
    widgets: [
      {
        id: 'card',
        name: 'Card',
        defaultSize: '2x2',
        sizes: ['2x2'],
        category: 'utility',
        entry: 'widget/index.js',
        styles: 'widget.css',
      },
    ],
  } as TappManifest

  it('maps download.code onto the declared core entry', () => {
    const modules = storeInstallModules(layered, 'CORE', {
      'page/index.js': 'PAGE',
      'widget/index.js': 'WIDGET',
    })
    assert.deepEqual(modules, {
      'core.js': 'CORE',
      'page/index.js': 'PAGE',
      'widget/index.js': 'WIDGET',
    })
  })

  it('refuses a store index that omits a declared layer entry', () => {
    assert.throws(
      () => storeInstallModules(layered, 'CORE', { 'page/index.js': 'PAGE' }),
      /widget\/index\.js/,
    )
  })

  // 作者样式必须按 widget id 走 widgetStyles，不能混进宿主预编译的 widgetCss。
  it('spreads the single store widget CSS across widgets declaring styles', () => {
    assert.deepEqual(storeWidgetStyles(layered, '.card{}'), {
      card: '.card{}',
    })
  })

  it('refuses a store package whose declared widget styles have no content', () => {
    assert.throws(() => storeWidgetStyles(layered, ''), /card=widget\.css/)
  })

  it('sends no widget styles when no widget declares any', () => {
    const noStyles = {
      ...layered,
      widgets: [{ ...layered.widgets![0]!, styles: undefined }],
    } as TappManifest
    assert.equal(storeWidgetStyles(noStyles, '.card{}'), undefined)
  })
})
