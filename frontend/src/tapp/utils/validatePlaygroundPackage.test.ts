/**
 * Pure-function tests for validatePlaygroundPackage.
 * Run from frontend/:
 *   node --experimental-strip-types --test src/tapp/utils/validatePlaygroundPackage.test.ts
 */

/* eslint-disable test/no-import-node-test -- node:test; project has no vitest dep */

import type { TappCodeStructure, TappManifest } from '../types'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  formatPlaygroundPackageErrors,
  validateAssetPath,
  validatePlaygroundPackage,
} from './validatePlaygroundPackage.ts'

function validPageProject(): {
  manifest: TappManifest
  code: TappCodeStructure
} {
  const manifest: TappManifest = {
    id: 'com.example.page',
    name: 'Page App',
    version: '1.0.0',
    main: 'main.js',
    permissions: [],
    category: 'utility',
    hasPage: true,
    styles: 'styles.css',
    pageTemplate: 'page.html',
    cssMode: 'unified',
  }

  const code: TappCodeStructure = {
    core: 'const core = 1;',
    page: 'function renderPage() {}',
    styles: '.root { color: red; }',
    pageHtml: '<div class="root">Hi</div>',
  }

  return { manifest, code }
}

function validWidgetProject(): {
  manifest: TappManifest
  code: TappCodeStructure
} {
  const { manifest, code } = validPageProject()
  return {
    manifest: {
      ...manifest,
      permissions: ['widget:register'],
      widgets: [
        {
          id: 'card',
          name: 'Card',
          defaultSize: '2x2',
          sizes: ['2x2'],
        },
      ],
    },
    code: {
      ...code,
      widget: 'function renderWidget() {}',
      widgetHtml: '<div class="widget">W</div>',
    },
  }
}

describe('validatePlaygroundPackage', () => {
  it('accepts a valid minimal page project', () => {
    const result = validatePlaygroundPackage(validPageProject())
    assert.equal(result.ok, true)
    if (result.ok) {
      assert.ok(result.package.files['main.js'])
      assert.ok(result.package.files['page.html'])
      assert.ok(result.package.files['styles.css'])
    }
  })

  it('accepts a valid widget project with template files', () => {
    const result = validatePlaygroundPackage(validWidgetProject())
    assert.equal(result.ok, true)
    if (result.ok) {
      assert.ok(result.package.files['templates/card.html'])
    }
  })

  it('rejects missing page.html when pageTemplate is declared', () => {
    const { manifest, code } = validPageProject()
    const result = validatePlaygroundPackage({
      manifest,
      code: { ...code, pageHtml: undefined },
    })
    assert.equal(result.ok, false)
    if (!result.ok) {
      assert.ok(
        result.errors.some(
          (e) =>
            e.includes('page code and HTML') ||
            e.includes('page.html') ||
            e.includes('resource not found'),
        ),
        `expected page/html errors, got: ${result.errors.join('; ')}`,
      )
    }
  })

  it('rejects invalid asset path templates/foo.html in manifest.assets', () => {
    const { manifest, code } = validPageProject()
    const result = validatePlaygroundPackage({
      manifest: {
        ...manifest,
        assets: ['templates/foo.html'],
      },
      code,
    })
    assert.equal(result.ok, false)
    if (!result.ok) {
      assert.ok(
        result.errors.some(
          (e) =>
            e.includes('assets/') ||
            e.includes('script or HTML') ||
            e.includes('templates/foo.html'),
        ),
        `expected asset path error, got: ${result.errors.join('; ')}`,
      )
    }
  })

  it('rejects broken assets path that is missing from the package map', () => {
    const { manifest, code } = validPageProject()
    const result = validatePlaygroundPackage({
      manifest: {
        ...manifest,
        assets: ['assets/missing.png'],
      },
      code: {
        ...code,
        // no code.assets entry → file not written
        assets: {},
      },
    })
    assert.equal(result.ok, false)
    if (!result.ok) {
      assert.ok(
        result.errors.some((e) =>
          e.includes('Declared Tapp asset not found: assets/missing.png'),
        ),
        `expected missing asset error, got: ${result.errors.join('; ')}`,
      )
    }
  })

  it('rejects widget declaration without widget HTML', () => {
    const { manifest, code } = validWidgetProject()
    const result = validatePlaygroundPackage({
      manifest,
      code: {
        ...code,
        widgetHtml: undefined,
        widget: 'function renderWidget() {}',
      },
    })
    assert.equal(result.ok, false)
    if (!result.ok) {
      assert.ok(
        result.errors.some(
          (e) =>
            e.includes('widgetHtml') ||
            e.includes('template') ||
            e.includes('resource not found'),
        ),
        `expected widget html errors, got: ${result.errors.join('; ')}`,
      )
    }
  })

  it('rejects invalid semver and missing category', () => {
    const { manifest, code } = validPageProject()
    const result = validatePlaygroundPackage({
      manifest: {
        ...manifest,
        version: 'not-a-version',
        category: undefined as unknown as TappManifest['category'],
      },
      code,
    })
    assert.equal(result.ok, false)
    if (!result.ok) {
      assert.ok(result.errors.some((e) => e.includes('semantic version')))
      assert.ok(result.errors.some((e) => e.includes('category')))
    }
  })

  it('formatPlaygroundPackageErrors numbers multi-error lists', () => {
    assert.equal(formatPlaygroundPackageErrors(['only']), 'only')
    assert.equal(
      formatPlaygroundPackageErrors(['a', 'b']),
      '1. a\n2. b',
    )
  })
})

describe('validateAssetPath', () => {
  it('allows static assets under assets/', () => {
    assert.equal(validateAssetPath('assets/icon.png'), null)
  })

  it('rejects paths outside assets/ and script/html entries', () => {
    assert.match(validateAssetPath('templates/foo.html') || '', /assets\//)
    assert.match(validateAssetPath('assets/hack.js') || '', /script or HTML/)
    assert.match(validateAssetPath('assets/page.html') || '', /script or HTML/)
  })
})
