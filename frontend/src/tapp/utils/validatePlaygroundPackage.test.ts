import type { TappPlaygroundCode } from '../services/TappPlaygroundService'
import type { TappManifest } from '../types'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  formatPlaygroundPackageErrors,
  validateAssetPath,
  validatePlaygroundPackage,
} from './validatePlaygroundPackage.ts'

function validPageProject(): {
  manifest: TappManifest
  code: TappPlaygroundCode
} {
  const manifest: TappManifest = {
    id: 'com.example.page',
    name: 'Page App',
    version: '1.0.0',
    permissions: [],
    category: 'utility',
    core: { entry: 'core.js', styles: 'styles.css' },
    page: { entry: 'page/index.js', template: 'page.html' },
  }

  const code: TappPlaygroundCode = {
    core: 'const core = 1;',
    page: 'function renderPage() {}',
    styles: '.root { color: red; }',
    pageHtml: '<div class="root">Hi</div>',
  }

  return { manifest, code }
}

function validWidgetProject(): {
  manifest: TappManifest
  code: TappPlaygroundCode
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
      widget:
        "Tapp.widgets['card'] = { render: function (container) { container.textContent = 'W'; } };",
      widgetHtml: '<div class="widget">W</div>',
    },
  }
}

function validWidgetOnlyProject(): {
  manifest: TappManifest
  code: TappPlaygroundCode
} {
  return {
    manifest: {
      id: 'com.example.widgetonly',
      name: 'Widget Only',
      version: '1.0.0',
      permissions: ['widget:register'],
      category: 'utility',
      core: { entry: 'core.js', styles: 'styles.css' },
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
      core: 'const core = 1;',
      page: '',
      styles: '.widget { color: red; }',
      pageHtml: '',
      widget:
        "Tapp.widgets['card'] = { render: function (container) { container.textContent = 'W'; } };",
      widgetHtml: '<div class="widget">W</div>',
    },
  }
}

describe('validatePlaygroundPackage', () => {
  it('accepts a valid minimal page project', () => {
    const result = validatePlaygroundPackage(validPageProject())
    assert.equal(result.ok, true)
    if (result.ok) {
      assert.ok(result.package.files['core.js'])
      assert.ok(result.package.files['page/index.js'])
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

  it('accepts a widget-only project without page.html', () => {
    const result = validatePlaygroundPackage(validWidgetOnlyProject())
    assert.equal(result.ok, true)
    if (result.ok) {
      assert.equal(result.package.manifest.page, undefined)
      assert.ok(result.package.files['core.js'])
      assert.ok(result.package.files['widget/index.js'])
      assert.ok(result.package.files['templates/card.html'])
      assert.equal(result.package.files['page.html'], undefined)
    }
  })

  it('does not require a core layer unless backgroundRequirements are declared', () => {
    const { manifest, code } = validPageProject()
    const result = validatePlaygroundPackage({
      manifest: { ...manifest, core: undefined },
      code: { ...code, core: '', styles: '' },
    })
    assert.equal(result.ok, true)
  })

  it('requires core when backgroundRequirements are declared without core code', () => {
    const { manifest, code } = validPageProject()
    const result = validatePlaygroundPackage({
      manifest: {
        ...manifest,
        core: undefined,
        backgroundRequirements: ['scheduler'],
      },
      code: { ...code, core: '' },
    })
    assert.equal(result.ok, false)
    if (!result.ok) {
      assert.ok(
        result.errors.some((e) => e.includes('backgroundRequirements')),
        `expected core/background error, got: ${result.errors.join('; ')}`,
      )
    }
  })

  it('rejects project with neither page nor widgets', () => {
    const { manifest, code } = validPageProject()
    const result = validatePlaygroundPackage({
      manifest: {
        ...manifest,
        page: undefined,
        widgets: undefined,
      },
      code: { ...code, page: '', pageHtml: '' },
    })
    assert.equal(result.ok, false)
    if (!result.ok) {
      assert.ok(
        result.errors.some(
          (e) =>
            e.includes('Page') ||
            e.includes('Widgets') ||
            e.includes('page'),
        ),
        `expected empty-project error, got: ${result.errors.join('; ')}`,
      )
    }
  })

  it('rejects a declared page layer without page content', () => {
    const { manifest, code } = validPageProject()
    const result = validatePlaygroundPackage({
      manifest,
      code: { ...code, page: '', pageHtml: '' },
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

  it('rejects missing page.html when page.template is declared', () => {
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
        widget:
          "Tapp.widgets['card'] = { render: function (container) { container.textContent = 'W'; } };",
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

  it('accepts valid locales and rejects malformed entries', () => {
    const { manifest, code } = validPageProject()
    const ok = validatePlaygroundPackage({
      manifest: {
        ...manifest,
        locales: { 'en-US': { name: 'Page App', description: 'Demo' } },
      },
      code,
    })
    assert.equal(ok.ok, true)

    const bad = validatePlaygroundPackage({
      manifest: {
        ...manifest,
        locales: {
          'not a tag': { name: 'X' },
          'en-US': { name: '   ' },
          'ja-JP': { description: 'x'.repeat(2001) },
        },
      },
      code,
    })
    assert.equal(bad.ok, false)
    if (!bad.ok) {
      assert.ok(bad.errors.some((e) => e.includes("key 'not a tag'")))
      assert.ok(bad.errors.some((e) => e.includes("locales['en-US'].name")))
      assert.ok(
        bad.errors.some((e) => e.includes("locales['ja-JP'].description")),
      )
    }
  })

  it('formatPlaygroundPackageErrors numbers multi-error lists', () => {
    assert.equal(formatPlaygroundPackageErrors(['only']), 'only')
    assert.equal(
      formatPlaygroundPackageErrors(['a', 'b']),
      '1. a\n2. b',
    )
  })

  it('rejects Tapp.ai without manifest.ai before install', () => {
    const { manifest, code } = validPageProject()
    const result = validatePlaygroundPackage({
      manifest,
      code: {
        ...code,
        page: "Tapp.ai.tasks.create({ version: 2, operation: 'image' })",
      },
    })
    assert.equal(result.ok, false)
    if (!result.ok) {
      assert.ok(result.errors.some((error) => error.includes('manifest.ai')))
    }
  })

  it('rejects Widget register instead of Tapp.widgets render', () => {
    const { manifest, code } = validWidgetOnlyProject()
    const result = validatePlaygroundPackage({
      manifest,
      code: {
        ...code,
        widget: "Tapp.widget.register({ id: 'card', name: 'Card' });",
      },
    })
    assert.equal(result.ok, false)
    if (!result.ok) {
      assert.ok(
        result.errors.some(
          (error) =>
            error.includes('Tapp.widgets') || error.includes('Page-only'),
        ),
      )
    }
  })

  it('rejects Widget Page-only SDK: confirm, fullscreen, and model3d', () => {
    const { manifest, code } = validWidgetOnlyProject()
    const confirm = validatePlaygroundPackage({
      manifest,
      code: {
        ...code,
        widget:
          "Tapp.widgets['card'] = { render: function () {} }; Tapp.ui.confirm('x');",
      },
    })
    assert.equal(confirm.ok, false)
    if (!confirm.ok) {
      assert.ok(
        confirm.errors.some((error) => error.includes('Tapp.ui.confirm')),
      )
    }

    const model3d = validatePlaygroundPackage({
      manifest,
      code: {
        ...code,
        widget:
          "Tapp.widgets['card'] = { render: function () {} }; Tapp.model3d.getUrl('x');",
      },
    })
    assert.equal(model3d.ok, false)
    if (!model3d.ok) {
      assert.ok(
        model3d.errors.some((error) => error.includes('Tapp.model3d')),
      )
    }

    const fullscreen = validatePlaygroundPackage({
      manifest,
      code: {
        ...code,
        widget:
          "Tapp.widgets['card'] = { render: function () {} }; Tapp.ui.fullscreen.request();",
      },
    })
    assert.equal(fullscreen.ok, false)
    if (!fullscreen.ok) {
      assert.ok(
        fullscreen.errors.some((error) => error.includes('Tapp.ui.fullscreen')),
      )
    }

    const { manifest: pageManifest, code: pageCode } = validPageProject()
    const breakpoints = validatePlaygroundPackage({
      manifest: pageManifest,
      code: {
        ...pageCode,
        pageHtml: '<div class="p-4 md:p-6">Hi</div>',
      },
    })
    assert.equal(breakpoints.ok, false)
    if (!breakpoints.ok) {
      assert.ok(breakpoints.errors.some((error) => error.includes('md:')))
    }

    const okClasses = validatePlaygroundPackage({
      manifest: pageManifest,
      code: {
        ...pageCode,
        pageHtml: '<div class="p-4 text-sm rounded-md">Hi</div>',
      },
    })
    assert.equal(okClasses.ok, true)
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
