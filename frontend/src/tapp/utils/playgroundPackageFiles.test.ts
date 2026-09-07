/**
 * Pure-function tests for buildPlaygroundPackageFiles.
 * Run from frontend/:
 *   node --experimental-strip-types --test src/tapp/utils/playgroundPackageFiles.test.ts
 */

import type { TappPlaygroundCode } from '../services/TappPlaygroundService'
import type { TappManifest } from '../types'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  buildPlaygroundPackageFiles,
  packageFilesToDirectInstallBody,
  playgroundCodeToRuntime,
} from './playgroundPackageFiles.ts'

function minimalProject(): {
  manifest: TappManifest
  code: TappPlaygroundCode
} {
  const manifest: TappManifest = {
    id: 'com.example.minimal',
    name: 'Minimal',
    version: '1.0.0',
    permissions: [],
    category: 'utility',
    widgets: [
      {
        id: 'card',
        name: 'Card',
        defaultSize: '2x2',
        sizes: ['2x2'],
      },
    ],
  }

  const code: TappPlaygroundCode = {
    core: 'const core = 1;',
    widget: 'function renderWidget() {}',
    page: 'function renderPage() {}',
    styles: '.root { color: red; }',
    pageHtml: '<div class="root">Hi</div>',
    widgetHtml: '<div class="widget">W</div>',
    i18n: {
      'en-US': { hello: 'Hello' },
      'zh-CN': { hello: '你好' },
    },
    assets: {
      'icon.png':
        'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==',
    },
  }

  return { manifest, code }
}

describe('buildPlaygroundPackageFiles', () => {
  it('lays each layer out as its own file', () => {
    const { manifest, code } = minimalProject()
    const { files, manifest: normalized } = buildPlaygroundPackageFiles(
      manifest,
      code,
    )
    const paths = Object.keys(files).sort()

    assert.ok(paths.includes('manifest.json'))
    assert.ok(paths.includes('core.js'))
    assert.ok(paths.includes('page/index.js'))
    assert.ok(paths.includes('widget/index.js'))
    assert.ok(paths.includes('styles.css'))
    assert.ok(paths.includes('page.html'))
    assert.ok(paths.includes('templates/card.html'))
    assert.ok(paths.includes('i18n/en-US.json'))
    assert.ok(paths.includes('assets/icon.png'))

    assert.deepEqual(normalized.core, {
      entry: 'core.js',
      styles: 'styles.css',
    })
    assert.deepEqual(normalized.page, {
      entry: 'page/index.js',
      template: 'page.html',
    })
    assert.equal(normalized.widgets?.[0]?.entry, 'widget/index.js')
    assert.equal(
      normalized.widgets?.[0]?.templates?.['2x2'],
      'templates/card.html',
    )
  })

  /// 层各自成文件之后，包里不该再出现注释切割标记。
  it('emits no code section markers', () => {
    const { manifest, code } = minimalProject()
    const { files } = buildPlaygroundPackageFiles(manifest, code)
    for (const content of Object.values(files)) {
      if (typeof content !== 'string') continue
      assert.doesNotMatch(content, /Widget Code|Page Code/)
    }
    assert.equal(files['core.js'], code.core)
    assert.equal(files['page/index.js'], code.page)
    assert.equal(files['widget/index.js'], code.widget)
  })

  it('packageFilesToDirectInstallBody preserves install API fields from the same map', () => {
    const { manifest, code } = minimalProject()
    const pkg = buildPlaygroundPackageFiles(manifest, code)
    const body = packageFilesToDirectInstallBody(pkg, code.assets)

    assert.deepEqual(Object.keys(body.modules).sort(), [
      'core.js',
      'page/index.js',
      'widget/index.js',
    ])
    assert.equal(body.coreStyles, code.styles)
    assert.equal(body.pageTemplate, code.pageHtml)
    assert.equal(body.widgetTemplates?.card?.['2x2'], code.widgetHtml)
    assert.equal((body.i18n?.['en-US'] as { hello: string }).hello, 'Hello')
    assert.deepEqual(body.assets, code.assets)
  })

  it('widget-only package omits the page layer entirely', () => {
    const manifest: TappManifest = {
      id: 'com.example.widgetonly',
      name: 'Widget Only',
      version: '1.0.0',
      permissions: ['widget:register'],
      category: 'utility',
      widgets: [
        {
          id: 'card',
          name: 'Card',
          defaultSize: '2x2',
          sizes: ['2x2'],
        },
      ],
    }
    const code: TappPlaygroundCode = {
      core: 'const core = 1;',
      page: '',
      styles: '.w { color: red; }',
      pageHtml: '',
      widget: 'function renderWidget() {}',
      widgetHtml: '<div class="w">W</div>',
    }
    const { files, manifest: normalized } = buildPlaygroundPackageFiles(
      manifest,
      code,
    )
    assert.equal(normalized.page, undefined)
    assert.equal(files['page.html'], undefined)
    assert.equal(files['page/index.js'], undefined)
    assert.ok(files['core.js'])
    assert.ok(files['widget/index.js'])
    assert.ok(files['styles.css'])
    assert.ok(files['templates/card.html'])

    const body = packageFilesToDirectInstallBody(
      { manifest: normalized, files },
      code.assets,
    )
    assert.equal(body.pageTemplate, undefined)
    assert.equal(body.widgetTemplates?.card?.['2x2'], code.widgetHtml)
  })
})

describe('playgroundCodeToRuntime', () => {
  /// 预览跑的东西必须和装出来的包是同一份布局。
  it('projects the editing model onto the packaged layout', () => {
    const { manifest, code } = minimalProject()
    const runtime = playgroundCodeToRuntime(manifest, code)
    const { files } = buildPlaygroundPackageFiles(manifest, code)

    assert.equal(runtime.coreEntry, 'core.js')
    assert.equal(runtime.pageEntry, 'page/index.js')
    // 按真实 widget id 建表，沙箱才取得到自己那层的入口
    assert.deepEqual(runtime.widgetEntries, { card: 'widget/index.js' })
    assert.deepEqual(Object.keys(runtime.modules).sort(), [
      'core.js',
      'page/index.js',
      'widget/index.js',
    ])
    for (const [path, source] of Object.entries(runtime.modules)) {
      assert.equal(source, files[path])
    }
  })

  it('omits entries the project does not define', () => {
    const runtime = playgroundCodeToRuntime(
      {},
      {
        core: 'const core = 1;',
        page: '',
        styles: '',
        pageHtml: '',
      },
    )
    assert.equal(runtime.pageEntry, undefined)
    assert.equal(runtime.widgetEntries, undefined)
    assert.deepEqual(Object.keys(runtime.modules), ['core.js'])
  })

  it('compiles Tailwind utilities used in preview HTML', () => {
    const runtime = playgroundCodeToRuntime(
      {},
      {
        core: '',
        page: '',
        styles: '',
        pageHtml: '<div class="flex gap-2 rounded-xl">Hi</div>',
      },
    )
    assert.match(runtime.pageCSS || '', /display:\s*flex/)
  })
})
