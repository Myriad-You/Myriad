import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import test from 'node:test'
import { compileFunction } from 'node:vm'
import { act, createElement } from 'react'
import merope from '../../../i18n/merope.zh-CN.json'

const require = createRequire(import.meta.url)
const { JSDOM } = require(require.resolve('jsdom', { paths: [require.resolve('isomorphic-dompurify')] }))
const { build } = createRequire(import.meta.resolve('tsx/package.json'))('esbuild')

const copy = merope.mind.nodes
const at = (day: number) => Date.UTC(2026, 10, day) * 1000

/** Made-up answers in the shapes `/api/agent/memory/nodes` gives. */
const list = {
  mode: 'shadow',
  herName: '阿离',
  audiences: [
    {
      audience: 'personal:local-owner', place: 'private', nodes: [
        { id: 'about', name: 'owner', kind: 'person', status: 'active', own: true, entries: 1, spans: 0, waiting: 1, latest: at(5) },
        { id: 'coffee', name: '咖啡', kind: 'topic', status: 'resting', own: false, entries: 2, spans: 1, waiting: 0, latest: at(4) },
      ],
    },
    { audience: 'own:', place: 'own', nodes: [{ id: 'her', name: '阿离', kind: 'person', status: 'active', own: true, entries: 0, spans: 0, waiting: 0, latest: 0 }] },
    { audience: 'group:qq:5', place: 'group', group: 'qq:5', groupName: '游戏群', nodes: [{ id: 'game', name: '游戏', kind: 'topic', status: 'active', own: false, entries: 1, spans: 0, waiting: 0, latest: at(3) }] },
  ],
}

const coffee = {
  id: 'coffee', audience: 'personal:local-owner', gone: false, name: '咖啡', kind: 'topic', status: 'resting', own: false,
  namedByOwner: true, aliases: [], summary: ['每天早上手冲一杯。'], views: [{ id: 'v1', text: '浅烘更合口味', at: at(4) }], open: [],
  entries: [
    { row: 'e1', text: '豆子只买浅烘', at: at(4), saidBy: ['partner'], seen: false, quoteOnly: true },
    { row: 'e2', text: '群里有人说在戒咖啡', at: at(3), saidBy: ['other_person'], seen: true, quoteOnly: false },
  ],
  heard: [{ row: 'h1', text: '听说换了磨豆机', at: at(2), saidBy: ['third_party'], seen: false, quoteOnly: false }],
  spans: [{ row: 's1', text: '聊了怎么冲', at: at(4), saidBy: ['her', 'partner'], wordsGone: true }],
  intents: [],
}

const about = {
  ...coffee, id: 'about', name: 'owner', kind: 'person', status: 'active', own: true, namedByOwner: false, summary: [], views: [],
  entries: [{ row: 'a1', text: '住在新加坡', at: at(5), saidBy: ['partner'], seen: false, quoteOnly: false }], heard: [], spans: [],
  intents: [{
    id: 'i1', text: '提醒开会', status: 'waiting', origin: 'partner', action: 'say', at: at(5),
    when: { at: at(6), zone: 'Asia/Singapore' },
    instruction: { echo: '好，明天九点叫你', message: null },
  }],
}

test('the node page lists each audience, opens a node and changes only what the owner may', async () => {
  const dom = new JSDOM('<div id="root"></div>')
  const globals = { window: dom.window, document: dom.window.document, IS_REACT_ACT_ENVIRONMENT: true }
  const previous = new Map(Object.keys(globals).map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)]))
  for (const [key, value] of Object.entries(globals)) Object.defineProperty(globalThis, key, { configurable: true, value })
  // Loaded once a window is there, so React handles typing as a browser's.
  const { createRoot } = await import('react-dom/client')
  const calls: string[] = []
  let refuseCancel = true
  const pages: Record<string, unknown> = { coffee, about }
  const api = {
    list: async () => { calls.push('list'); return list },
    node: async (_audience: string, node: string) => { calls.push(`node:${node}`); return pages[node] },
    rename: async (audience: string, node: string, name: string) => { calls.push(`rename:${audience}:${node}:${name}`) },
    forget: async (audience: string, node: string, row?: string) => { calls.push(`forget:${node}:${row ?? ''}`) },
    cancelIntent: async (_audience: string, node: string, intent: string) => {
      if (refuseCancel) throw Object.assign(new Error('refused'), { code: 'mind_elsewhere' })
      calls.push(`cancel:${node}:${intent}`)
    },
    answerHeard: async (_audience: string, node: string, row: string, answer: string) => { calls.push(`heard:${node}:${row}:${answer}`) },
  }
  const bundle = await build({
    entryPoints: [new URL('./AgentNodesPanel.tsx', import.meta.url).pathname], bundle: true, write: false,
    platform: 'node', format: 'cjs', packages: 'external', define: { 'import.meta.env': '{}' },
    plugins: [{ name: 'boundaries', setup(builder) {
      builder.onResolve({ filter: /(\/settings|contexts\/I18nContext|services\/agent|\.css)$/ }, ({ path }) => ({ path, external: true }))
    } }],
  })
  const mockRequire = (path: string) => {
    if (path.endsWith('.css')) return {}
    if (path.endsWith('/settings')) {
      return {
        SettingsButton: ({ children, onClick, type }: { children: React.ReactNode; onClick?: () => void; type?: 'submit' | 'button' }) =>
          createElement('button', { onClick, type: type ?? 'button' }, children),
      }
    }
    if (path.endsWith('/I18nContext')) return { useI18n: () => ({ t: { merope }, locale: 'zh-CN' }) }
    if (path.endsWith('/services/agent')) return { memoryNodesApi: api }
    return require(path)
  }
  const module = { exports: {} as typeof import('./AgentNodesPanel') }
  compileFunction(bundle.outputFiles[0].text, ['require', 'module', 'exports'])(mockRequire, module, module.exports)
  const root = createRoot(dom.window.document.getElementById('root'))
  const text = () => dom.window.document.body.textContent as string
  const buttons = (label: string) =>
    [...dom.window.document.querySelectorAll('button')].filter(element => element.textContent === label) as HTMLButtonElement[]
  const click = (element: HTMLElement) => act(async () => element.click())
  try {
    await act(async () => root.render(createElement(module.exports.default)))
    // Each audience under its own title; 「关于你」 for the owner's own node.
    assert.ok(text().includes(copy.private) && text().includes('阿离 自己') && text().includes('群：游戏群'))
    assert.ok(text().includes(copy.aboutYou) && text().includes(copy.status.resting))
    assert.ok(text().includes('记忆 2') && text().includes('待提起 1'))
    assert.ok(text().includes(copy.modeShadow))

    await click(dom.window.document.querySelector('[data-node="coffee"]'))
    assert.ok(calls.includes('node:coffee'))
    assert.ok(text().includes('每天早上手冲一杯。') && text().includes('浅烘更合口味'))
    assert.ok(text().includes(copy.namedByYou) && text().includes(copy.quoteOnly) && text().includes(copy.seenElsewhere))
    assert.ok(text().includes(copy.said.partner))
    // Where the original words went, only her note is kept.
    assert.ok(text().includes('原话已删除，只保留阿离记下的'))
    // Heard: the two answers.
    await click(buttons(copy.heardNo)[0])
    assert.ok(calls.includes('heard:coffee:h1:denied'))

    // Deleting an entry asks first.
    await click(buttons(copy.deleteEntry)[0])
    assert.ok(text().includes(copy.deleteEntryTitle) && text().includes(copy.deleteEntryMessage))
    assert.ok(!calls.some(call => call.startsWith('forget:')))
    await click(dom.window.document.querySelector('[role="alertdialog"] button'))
    assert.ok(calls.includes('forget:coffee:e1'))

    // Renaming.
    await click(buttons(copy.rename)[0])
    const input = dom.window.document.querySelector('input') as HTMLInputElement
    assert.equal(input.value, '咖啡')
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(dom.window.HTMLInputElement.prototype, 'value')!.set!
      setter.call(input, '手冲咖啡')
      input.dispatchEvent(new dom.window.Event('input', { bubbles: true }))
    })
    await act(async () => (dom.window.document.querySelector('form') as HTMLFormElement).requestSubmit())
    assert.ok(calls.includes('rename:personal:local-owner:coffee:手冲咖啡'))

    // The whole node.
    await click(buttons(copy.deleteNode)[0])
    assert.ok(text().includes('要删除「咖啡」吗？'))
    await click(dom.window.document.querySelector('[role="alertdialog"] button'))
    assert.ok(calls.includes('forget:coffee:'))
    // Back on the list once it is gone.
    assert.ok(text().includes(copy.private))

    // 「关于你」: an instruction whose message was deleted, and no whole-node delete.
    await click(dom.window.document.querySelector('[data-node="about"]'))
    assert.ok(text().includes('提醒开会') && text().includes(copy.intent.byYou) && text().includes(copy.intent.state.waiting))
    assert.ok(text().includes(copy.deleted) && text().includes('好，明天九点叫你'))
    assert.equal(buttons(copy.deleteNode).length, 0)
    // A refusal says why, by its code.
    await click(buttons(copy.intent.cancel)[0])
    assert.ok(text().includes(copy.failed.away))
    refuseCancel = false
    await click(buttons(copy.intent.cancel)[0])
    assert.ok(calls.includes('cancel:about:i1'))
    await click(buttons(copy.back)[0])
    assert.ok(text().includes('群：游戏群'))
  } finally {
    await act(async () => root.unmount())
    dom.window.close()
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor)
      else Reflect.deleteProperty(globalThis, key)
    }
  }
})
