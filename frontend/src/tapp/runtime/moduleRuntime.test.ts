import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { runInNewContext } from 'node:vm'
import {
  buildLayerRuntime,
  collectLayerModules,
  collectRequires,
  resolveModulePath,
} from './moduleRuntime.ts'

/** 与安装期解析器共用。两边必须对同一输入给出同一答案。改表时同步 backend SHARED_RESOLUTION_CASES。 */
const SHARED_RESOLUTION_CASES: Array<[string, string, string | null]> = [
  ['page/index.js', './state.js', 'page/state.js'],
  ['page/index.js', '../core.js', 'core.js'],
  ['page/ui/list.js', '../state/store.js', 'page/state/store.js'],
  ['core.js', './lib/a.js', 'lib/a.js'],
  // 逃出包根的写法被拒绝，不折叠回根内。
  ['core.js', '../../outside.js', null],
  ['core.js', '../core.js', null],
  ['page/index.js', '../../core.js', null],
]

describe('resolveModulePath', () => {
  it('agrees with the install-time resolver on every shared case', () => {
    for (const [from, request, expected] of SHARED_RESOLUTION_CASES) {
      assert.equal(
        resolveModulePath(from, request),
        expected,
        `resolving ${request} from ${from}`,
      )
    }
  })
})

/** 与安装期提取器共用。改这里时同步 backend SHARED_EXTRACTION_SOURCE。 */
const SHARED_EXTRACTION_SOURCE = `
        var a = require('./a.js')
        var b = require("../b.js")
        var dynamic = require(name)
        var similar = myRequire('./c.js')
        // require('./commented.js')
        /* require('./blocked.js') */
        var text = "require('./in-string.js')"
        var tpl = \`require('./in-template.js')\`
`

describe('collectRequires', () => {
  it('agrees with the install-time extractor on every shared case', () => {
    // 入口放在层目录里，./a.js 与 ../b.js 才都合法。
    const { resolved, missing } = collectRequires(
      'page/index.js',
      SHARED_EXTRACTION_SOURCE,
      {
        'page/index.js': SHARED_EXTRACTION_SOURCE,
        'page/a.js': '',
        'b.js': '',
      },
    )
    assert.deepEqual(Iterator.from(resolved.keys()).toArray(), ['./a.js', '../b.js'])
    assert.deepEqual(missing, [])
  })
})

describe('collectLayerModules', () => {
  const modules = {
    'core.js': 'module.exports = { shared: 1 };',
    'page/index.js':
      'var core = require("../core.js"); var s = require("./state.js");',
    'page/state.js': 'module.exports = {};',
    'widget/index.js': 'require("../core.js");',
    'orphan.js': 'module.exports = 1;',
  }

  // widget 沙箱不能拿到 Page 的 JS：注入按依赖图，不是整包。
  it('walks only the dependency closure of the given entries', () => {
    const page = collectLayerModules(modules, ['core.js', 'page/index.js'])
    assert.deepEqual(page.included, [
      'core.js',
      'page/index.js',
      'page/state.js',
    ])
    assert.deepEqual(page.missing, [])

    const widget = collectLayerModules(modules, ['core.js', 'widget/index.js'])
    assert.ok(!widget.included.includes('page/index.js'))
    assert.ok(!widget.included.includes('orphan.js'))
  })

  it('reports unresolved requires instead of silently dropping them', () => {
    const broken = collectLayerModules({ 'core.js': 'require("./nope.js");' }, [
      'core.js',
    ])
    assert.deepEqual(broken.missing, ['core.js → ./nope.js'])
  })

  it('reports a missing entry', () => {
    const result = collectLayerModules({}, ['core.js'])
    assert.deepEqual(result.missing, ['core.js'])
    assert.deepEqual(result.included, [])
  })
})

describe('buildLayerRuntime', () => {
  it('runs entries in order with core first', () => {
    const plan = buildLayerRuntime(
      {
        'core.js': 'globalThis.__order.push("core");',
        'page/index.js': 'globalThis.__order.push("page");',
      },
      ['core.js', 'page/index.js'],
    )
    assert.deepEqual(plan.entries, ['core.js', 'page/index.js'])

    const order: string[] = []
    ;(globalThis as { __order?: string[] }).__order = order
    // eslint-disable-next-line no-new-func -- host-side test only; the sandbox never does this
    new Function(plan.source)()
    assert.deepEqual(order, ['core', 'page'])
  })

  it('isolates module top-level declarations and shares via exports', () => {
    const plan = buildLayerRuntime(
      {
        'core.js':
          'var secret = 42; module.exports = { get: function () { return secret; } };',
        'page/index.js':
          'var core = require("../core.js"); globalThis.__result = { fromExports: core.get(), leaked: typeof secret };',
      },
      ['core.js', 'page/index.js'],
    )
    // eslint-disable-next-line no-new-func -- host-side test only
    new Function(plan.source)()
    const result = (globalThis as { __result?: Record<string, unknown> })
      .__result
    assert.equal(result?.fromExports, 42)
    assert.equal(result?.leaked, 'undefined')
  })

  it('executes each module once even with several dependents', () => {
    const plan = buildLayerRuntime(
      {
        'core.js': 'globalThis.__count = (globalThis.__count || 0) + 1;',
        'a.js': 'require("./core.js");',
        'page/index.js': 'require("../core.js"); require("../a.js");',
      },
      ['core.js', 'page/index.js'],
    )
    ;(globalThis as { __count?: number }).__count = 0
    // eslint-disable-next-line no-new-func -- host-side test only
    new Function(plan.source)()
    assert.equal((globalThis as { __count?: number }).__count, 1)
  })

  it('keeps module ownership checks intact when app code replaces Object.hasOwn', () => {
    const plan = buildLayerRuntime({
      'core.js': 'module.exports = { value: 7 };',
      'page/index.js': `
        const core = require('../core.js');
        Object.hasOwn = () => true;
        if (require('../core.js') !== core) throw new Error('Lost module cache');
        require('toString');
      `,
    }, ['core.js', 'page/index.js'])
    assert.throws(() => runInNewContext(plan.source), /Cannot find module "toString"/)
  })

  it('breaks require cycles with partial exports', () => {
    const plan = buildLayerRuntime(
      {
        'a.js':
          'exports.name = "a"; var b = require("./b.js"); exports.sawB = b.name;',
        'b.js':
          'var a = require("./a.js"); exports.name = "b"; exports.sawA = a.name;',
      },
      ['a.js'],
    )
    // eslint-disable-next-line no-new-func -- host-side test only
    new Function(`${plan.source}`)()
    assert.ok(plan.includedModules.includes('b.js'))
  })

  it('consumes the host resolution graph without rescanning source', () => {
    const plan = buildLayerRuntime(
      {
        'layers/page.js':
          'var shared = require("./shared"); globalThis.__hostGraph = shared.value;',
        'lib/shared.js': 'module.exports = { value: 7 };',
        'widget/private.js': 'globalThis.__wrongLayer = true;',
      },
      ['layers/page.js'],
      {
        'layers/page.js': {
          './shared': 'lib/shared.js',
        },
      },
    )

    assert.deepEqual(plan.includedModules, ['layers/page.js', 'lib/shared.js'])
    assert.ok(!plan.source.includes('widget/private.js'))
    // eslint-disable-next-line no-new-func -- host-side test only
    new Function(plan.source)()
    assert.equal((globalThis as { __hostGraph?: number }).__hostGraph, 7)
  })

  it('throws inside the sandbox for an unknown module', () => {
    const plan = buildLayerRuntime({ 'core.js': 'require("./ghost.js");' }, [
      'core.js',
    ])
    // eslint-disable-next-line no-new-func -- host-side test only
    assert.throws(() => new Function(plan.source)(), /Cannot find module/)
  })

  it('neutralises a closing script tag inside module source', () => {
    const plan = buildLayerRuntime(
      { 'core.js': 'var html = "</script><script>alert(1)</script>";' },
      ['core.js'],
    )
    assert.ok(!plan.source.includes('</script>'))
  })

  it('produces nothing when the layer has no modules', () => {
    const plan = buildLayerRuntime({}, [])
    assert.equal(plan.source, '')
    assert.deepEqual(plan.entries, [])
  })
})
