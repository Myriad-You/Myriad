import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import test from 'node:test'
import { compileFunction } from 'node:vm'
import { act, createElement, forwardRef, useImperativeHandle } from 'react'
import { createRoot } from 'react-dom/client'
import en from '../../i18n/en-US.json'
import zh from '../../i18n/zh-CN.json'

const require = createRequire(import.meta.url)
const { JSDOM } = require(require.resolve('jsdom', { paths: [require.resolve('isomorphic-dompurify')] }))
const { build } = createRequire(import.meta.resolve('tsx/package.json'))('esbuild')

test('partial refresh reloads persisted views and keeps localized failure details across language changes', async () => {
  const dom = new JSDOM('<div id="root"></div>')
  let response: { success: boolean; partial: boolean; issues: { stage: string; reason: string }[]; message?: string } = { success: false, partial: true, issues: [{ stage: 'bangumi', reason: 'private' }] }
  const globals = { window: dom.window, document: dom.window.document, IS_REACT_ACT_ENVIRONMENT: true }
  const previous = new Map(Object.keys(globals).map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)]))
  for (const [key, value] of Object.entries(globals)) Object.defineProperty(globalThis, key, { configurable: true, value })
  dom.window.confirm = () => true
  let statusLoads = 0
  let previewLoads = 0
  let locale = 'zh-CN'
  const cacheStatus = async () => ({ exists: true })
  const platformTasks = {
    getPlatformCacheStatus: cacheStatus,
    getPlatformMetadataStatus: async () => { statusLoads++; return { success: true, has_raw_data: true } },
    fetchPlatformData: async () => response,
    clearPlatformCache: async () => {},
    submitPlatformTask: async () => 'task',
  }
  const bindGuide = () => ({})
  const Preview = forwardRef((_props, ref) => { useImperativeHandle(ref, () => ({ reload: async () => { previewLoads++ } })); return null })
  const bundle = await build({
    entryPoints: [new URL('./PlatformDataManagement.tsx', import.meta.url).pathname], bundle: true, write: false,
    platform: 'node', format: 'cjs', packages: 'external', define: { 'import.meta.env': '{}' },
    plugins: [{ name: 'boundaries', setup(builder) {
      builder.onResolve({ filter: /(@lib\/icons|contexts\/I18nContext|services\/platformTasksApi|utils\/(recentActivity|userFacingError)|\/settings|\/TaskStatus|\/PlatformDataPreview)$/ }, ({ path }) => ({ path, external: true }))
    } }],
  })
  const mockRequire = (path: string) => {
    if (path.includes('platformTasksApi')) return platformTasks
    if (path.includes('I18nContext')) return { useI18n: () => ({ t: locale === 'zh-CN' ? zh : en, locale, format: (text: string) => text }) }
    if (path.includes('recentActivity')) return { notifyRecentActivityUpdated: () => {} }
    if (path.includes('userFacingError')) return { userFacingError: (message: unknown, fallback: string) => typeof message === 'string' ? message : fallback }
    if (path.endsWith('/settings')) return { useSettingGuide: () => ({ catalog: { platforms: {} }, bindGuide }), SettingGroup: ({ children }: { children: React.ReactNode }) => createElement('div', null, children), ButtonItem: (props: { itemKey: string; onClick: () => void }) => createElement('button', { onClick: props.onClick, 'data-key': props.itemKey }) }
    if (path.endsWith('/PlatformDataPreview')) return Preview
    if (path.endsWith('/TaskStatus')) return { TaskStatus: () => null }
    if (path === '@lib/icons') return { FaSyncAlt: () => null, FaTrash: () => null }
    return require(path)
  }
  const module = { exports: {} as typeof import('./PlatformDataManagement') }
  compileFunction(bundle.outputFiles[0].text, ['require', 'module', 'exports'])(mockRequire, module, module.exports)
  const root = createRoot(dom.window.document.getElementById('root'))
  const showMessage = () => {}
  const render = () => act(async () => root.render(createElement(module.exports.default, { platformName: 'Bilibili', showMessage })))
  try {
    await render()
    const initialLoads = statusLoads
    await act(async () => dom.window.document.querySelector('[data-key="platform-raw-refresh"]').click())
    assert.ok(statusLoads > initialLoads, 'partial response must refresh metadata status')
    assert.equal(previewLoads, 1, 'partial response must refresh the preview')
    assert.match(dom.window.document.querySelector('[role="status"]').textContent, /部分可用数据.*\n追番.*公开/)
    locale = 'en-US'
    await render()
    assert.match(dom.window.document.querySelector('[role="status"]').textContent, /Some data is still available.*\nFollowed series: Data is private/)
    response = { success: false, partial: false, issues: [], message: 'X API credits are depleted (HTTP 402).' }
    await act(async () => dom.window.document.querySelector('[data-key="platform-raw-refresh"]').click())
    assert.match(dom.window.document.querySelector('[role="status"]').textContent, /Fetch failed:.*\nX API credits are depleted/)
  } finally {
    await act(async () => root.unmount())
    dom.window.close()
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor)
      else Reflect.deleteProperty(globalThis, key)
    }
  }
})
