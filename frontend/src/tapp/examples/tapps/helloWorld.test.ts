import assert from 'node:assert/strict'
import { it } from 'node:test'
import { runInNewContext } from 'node:vm'
import { buildLayerRuntime } from '../../runtime/moduleRuntime.ts'
import { buildPlaygroundModules } from '../../utils/playgroundPackageFiles.ts'
import { helloWorldTapp } from './helloWorld.ts'

it('initializes and switches the example locale across isolated core and page modules', async () => {
  const text = new Map<string, string>()
  let ready: (() => Promise<void>) | undefined
  let changeLocale: ((locale: string) => void) | undefined
  const runtime = buildLayerRuntime(buildPlaygroundModules(helloWorldTapp.code), [
    'core.js',
    'page/index.js',
  ])
  runInNewContext(runtime.source, {
    document: { getElementById: (id: string) => ({ id }) },
    Tapp: {
      dom: { setText: (element: { id: string }, value: string) => text.set(element.id, value) },
      lifecycle: {
        onReady(callback: () => Promise<void>) { ready = callback },
        onPause() {},
        onResume() {},
        onDestroy() {},
      },
      ui: {
        getLocale: async () => 'ja-JP',
        onLocaleChange(callback: (locale: string) => void) { changeLocale = callback },
      },
    },
  })
  assert.ok(ready)
  await ready()
  assert.equal(text.get('hw-feat-lifecycle-title'), 'ライフサイクル')
  assert.ok(changeLocale)
  changeLocale('zh_HK')
  assert.equal(text.get('hw-feat-lifecycle-title'), '生命週期')
  changeLocale('unknown')
  assert.equal(text.get('hw-feat-lifecycle-title'), 'Lifecycle')
})
