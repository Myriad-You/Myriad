/**
 * Pure-function tests for buildPlaygroundPackageFiles.
 * Run from frontend/:
 *   node --experimental-strip-types --test src/tapp/utils/playgroundPackageFiles.test.ts
 */

/* eslint-disable test/no-import-node-test -- node:test; project has no vitest dep */

import type { TappCodeStructure, TappManifest } from '../types'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  buildPlaygroundMainJs,
  buildPlaygroundPackageFiles,
  packageFilesToDirectInstallBody,
} from './playgroundPackageFiles.ts'

function minimalProject(): {
  manifest: TappManifest
  code: TappCodeStructure
} {
  const manifest: TappManifest = {
    id: 'com.example.minimal',
    name: 'Minimal',
    version: '1.0.0',
    main: 'main.js',
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

  const code: TappCodeStructure = {
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
    pageModules: {
      'helpers.js': 'export const x = 1;',
      'index.js': 'export function main() {}',
    },
    pageModuleOrder: ['helpers.js', 'index.js'],
    assets: {
      'icon.png':
        'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==',
    },
  }

  return { manifest, code }
}

describe('buildPlaygroundPackageFiles', () => {
  it('includes expected package paths for a minimal playground project', () => {
    const { manifest, code } = minimalProject()
    const { files, manifest: normalized } = buildPlaygroundPackageFiles(
      manifest,
      code,
    )
    const paths = Object.keys(files).sort()

    assert.ok(paths.includes('manifest.json'))
    assert.ok(paths.includes('main.js'))
    assert.ok(paths.includes('styles.css'))
    assert.ok(paths.includes('page.html'))
    assert.ok(paths.includes('templates/card.html'))
    assert.ok(paths.includes('i18n/en-US.json'))
    assert.ok(paths.includes('i18n/zh-CN.json'))
    assert.ok(paths.includes('page/helpers.js'))
    assert.ok(paths.includes('page/index.js'))
    assert.ok(paths.includes('assets/icon.png'))

    assert.deepEqual(normalized.pageModules, ['helpers.js', 'index.js'])
    assert.equal(normalized.styles, 'styles.css')
    assert.equal(normalized.pageTemplate, 'page.html')
    assert.equal(
      normalized.widgets?.[0]?.templates?.['2x2'],
      'templates/card.html',
    )

    const main = files['main.js']
    assert.equal(typeof main, 'string')
    assert.match(String(main), /const core = 1;/)
    assert.match(String(main), /Widget Code/)
    // pageModules present → page body is not merged into main.js
    assert.doesNotMatch(String(main), /Page Code/)
  })

  it('buildPlaygroundMainJs matches package main.js entry', () => {
    const { code } = minimalProject()
    const { files } = buildPlaygroundPackageFiles(
      minimalProject().manifest,
      code,
    )
    assert.equal(files['main.js'], buildPlaygroundMainJs(code))
  })

  it('packageFilesToDirectInstallBody preserves install API fields from the same map', () => {
    const { manifest, code } = minimalProject()
    const pkg = buildPlaygroundPackageFiles(manifest, code)
    const body = packageFilesToDirectInstallBody(pkg, code.assets)

    assert.equal(body.code, pkg.files['main.js'])
    assert.equal(body.styles, code.styles)
    assert.equal(body.pageTemplate, code.pageHtml)
    assert.equal(body.widgetTemplates?.card?.['2x2'], code.widgetHtml)
    assert.deepEqual(body.pageModules, code.pageModules)
    assert.equal(
      (body.i18n?.['en-US'] as { hello: string }).hello,
      'Hello',
    )
    assert.deepEqual(body.assets, code.assets)
  })

  it('widget-only package omits page.html and sets hasPage false', () => {
    const manifest: TappManifest = {
      id: 'com.example.widgetonly',
      name: 'Widget Only',
      version: '1.0.0',
      main: 'main.js',
      permissions: ['widget:register'],
      category: 'utility',
      hasPage: false,
      widgets: [
        {
          id: 'card',
          name: 'Card',
          defaultSize: '2x2',
          sizes: ['2x2'],
        },
      ],
    }
    const code: TappCodeStructure = {
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
    assert.equal(normalized.hasPage, false)
    assert.equal(normalized.pageTemplate, undefined)
    assert.equal(files['page.html'], undefined)
    assert.ok(files['main.js'])
    assert.ok(files['styles.css'])
    assert.ok(files['templates/card.html'])
    assert.match(String(files['main.js']), /Widget Code/)
    assert.doesNotMatch(String(files['main.js']), /Page Code/)

    const body = packageFilesToDirectInstallBody(
      { manifest: normalized, files },
      code.assets,
    )
    assert.equal(body.pageTemplate, undefined)
    assert.equal(body.widgetTemplates?.card?.['2x2'], code.widgetHtml)
  })
})
