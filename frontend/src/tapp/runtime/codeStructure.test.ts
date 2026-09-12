import type { TappCodeStructure } from '../types'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  buildLayerScript,
  getCodeStructureFingerprint,
  getLayerEntries,
} from './codeStructure.ts'

function sampleCode(): TappCodeStructure {
  return {
    modules: {
      'core.js': 'module.exports = { shared: true };',
      'page/index.js': 'require("../core.js");',
      'widget/index.js': 'require("../core.js");',
    },
    coreEntry: 'core.js',
    pageEntry: 'page/index.js',
    widgetEntries: { card: 'widget/index.js' },
  }
}

describe('getLayerEntries', () => {
  // core 是共享层：三种模式都先执行。
  it('always runs core first', () => {
    const code = sampleCode()
    assert.deepEqual(getLayerEntries(code, 'page'), [
      'core.js',
      'page/index.js',
    ])
    assert.deepEqual(getLayerEntries(code, 'widget', 'card'), [
      'core.js',
      'widget/index.js',
    ])
    assert.deepEqual(getLayerEntries(code, 'background'), ['core.js'])
  })

  it('keeps page code out of widget mode and vice versa', () => {
    const code = sampleCode()
    assert.ok(!getLayerEntries(code, 'widget', 'card').includes('page/index.js'))
    assert.ok(!getLayerEntries(code, 'page').includes('widget/index.js'))
  })

  // 一个 widget 的 iframe 不执行同 Tapp 其它 widget 的代码；想共用就各自 require。
  it('loads only the named widget entry', () => {
    const code = sampleCode()
    code.modules['widget-a.js'] = 'a();'
    code.modules['widget-b.js'] = 'b();'
    code.widgetEntries = { a: 'widget-a.js', b: 'widget-b.js' }

    assert.deepEqual(getLayerEntries(code, 'widget', 'a'), [
      'core.js',
      'widget-a.js',
    ])
    const { source } = buildLayerScript(code, 'widget', 'a')
    assert.ok(source.includes('"widget-a.js"'))
    assert.ok(!source.includes('"widget-b.js"'))
  })

  it('loads no widget entry without a widget id', () => {
    assert.deepEqual(getLayerEntries(sampleCode(), 'widget'), ['core.js'])
  })

  it('rejects a widget id that is not in the projection', () => {
    assert.throws(
      () => getLayerEntries(sampleCode(), 'widget', 'ghost'),
      /Unknown widget id: ghost/,
    )
  })
})

describe('buildLayerScript', () => {
  it('emits nothing for a layer with no entries', () => {
    const code = sampleCode()
    delete code.coreEntry
    delete code.widgetEntries
    assert.equal(buildLayerScript(code, 'widget').source, '')
  })

  it('emits a runnable module registry for the layer', () => {
    const { source, includedModules } = buildLayerScript(sampleCode(), 'page')
    assert.ok(source.includes('"core.js"'))
    assert.ok(source.includes('"page/index.js"'))
    assert.ok(!source.includes('"widget/index.js"'))
    assert.deepEqual(includedModules, ['core.js', 'page/index.js'])
  })

  // iframe 不能 eval；宿主把源码原样塞进 srcdoc。
  it('runs the injected srcdoc script the way an iframe would', () => {
    const code = sampleCode()
    code.modules['core.js'] = 'module.exports = { appName: "My Tapp" };'
    code.modules['page/index.js'] =
      'var core = require("../core.js"); globalThis.__proof = core.appName; var note = "</script>";'
    code.modules['widget/index.js'] = 'globalThis.__widgetRan = true;'
    code.moduleResolutions = {
      'page/index.js': { '../core.js': 'core.js' },
    }

    const plan = buildLayerScript(code, 'page')
    assert.ok(!plan.source.includes('widget/index.js'))
    assert.ok(!plan.source.includes('</script>'))
    assert.ok(plan.source.includes('<\\/script'))

    // eslint-disable-next-line no-new-func -- host-side stand-in for the iframe script tag
    new Function(plan.source)()
    assert.equal((globalThis as { __proof?: string }).__proof, 'My Tapp')
    assert.equal(
      (globalThis as { __widgetRan?: boolean }).__widgetRan,
      undefined,
    )
  })
})

describe('getCodeStructureFingerprint', () => {
  it('changes when a module in the dependency graph changes', () => {
    const before = getCodeStructureFingerprint(sampleCode(), 'page')
    const code = sampleCode()
    code.modules['core.js'] = 'module.exports = { shared: false };'
    assert.notEqual(before, getCodeStructureFingerprint(code, 'page'))
  })

  // 无关层的代码变化不该重建这个 iframe。
  it('ignores modules outside the layer graph', () => {
    const before = getCodeStructureFingerprint(sampleCode(), 'page')
    const code = sampleCode()
    code.modules['widget/index.js'] = 'require("../core.js"); var extra = 1;'
    assert.equal(before, getCodeStructureFingerprint(code, 'page'))
  })

  it('distinguishes equal-length edits', () => {
    const before = getCodeStructureFingerprint(sampleCode(), 'background')
    const code = sampleCode()
    code.modules['core.js'] = 'module.exports = { shared: TRUE };'
    assert.notEqual(before, getCodeStructureFingerprint(code, 'background'))
  })
})
