import type { HerAlive } from '../../services/agent/agentApi'
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import test from 'node:test'
import { compileFunction } from 'node:vm'
import { act, createElement } from 'react'
import { createRoot } from 'react-dom/client'
import zh from '../../i18n/config.zh-CN.json'

const require = createRequire(import.meta.url)
const { JSDOM } = require(require.resolve('jsdom', { paths: [require.resolve('isomorphic-dompurify')] }))
const { build } = createRequire(import.meta.resolve('tsx/package.json'))('esbuild')

const NOW = '2026-11-16T04:00:00Z'

function alive(change: (shown: HerAlive) => void = () => {}): HerAlive {
  const shown: HerAlive = {
    now: NOW,
    startedAt: '2026-11-16T01:00:00Z',
    host: { holder: 'site', moving: false, since: null, temporary: false },
    life: {
      personaOn: true,
      shouldLive: true,
      background: 'running',
      livedAt: '2026-11-16T03:59:50Z',
      quietSecs: 10,
      alarmedFrom: null,
      quietAfterSecs: 600,
      gaps: { count: 0, secs: 0 },
      hostStops: { count: 1, secs: 3600 },
      drivers: [
        { name: 'her own time', everySecs: 60, ranAt: '2026-11-16T03:59:50Z', runs: 180, endedAt: '2026-11-16T03:59:51Z', running: false, failedAt: null, failures: 0, skippedAt: null, skippedWhy: null, skips: 0, overdue: false },
      ],
    },
    memory: { settledAt: null, settledOutcome: null, standingWrittenAt: null, standingFailures: null, standingFailed: 0, standingFailedAt: null, settlingFailures: { unreadable: 2, asked_again: 3, left_unread: 1, brought_along: 1, kept_unsettled: 4 }, cutOff: { chat_remember: 1, memory_node_upkeep: 2 }, unplaced: { kept: 3, placed_later: 2, failed: 1, errors: 4, let_go: 5 }, forgetting: { left_to_sweep: 1, swept: 6, sweep_errors: 2, left_out: 3, sweep_errors_in_a_row: 0 }, correctionsUnkept: 4, takeBackChecks: { pending: 2, changed: 1, unsure: 3 } },
    ends: [
      {
        id: 'dev_a',
        name: '书房的 Mac',
        connected: true,
        linkedAt: '2026-11-16T03:00:00Z',
        lastSeenAt: null,
        protocol: 2,
        app: { name: 'MyriadNative', version: '0.9.1' },
        coreAbi: 3,
        capabilities: { declared: ['log3', 'act', 'extra'], approved: null, granted: ['log3', 'act'] },
        log: { behindRows: 12, audiencesBehind: 1, heardAt: '2026-11-16T03:59:00Z' },
        dropped: [],
        unread: [{ t: 'say', count: 2, lastAt: '2026-11-16T03:30:00Z' }],
      },
    ],
    screenKeys: [{ t: 'presence', key: 'others_text', count: 4, lastAt: '2026-11-16T03:58:00Z' }],
    calls: [{ kind: 'act.request', outcomes: { asked: 3, completed: 2, act_busy: 1 } }],
  }
  change(shown)
  return shown
}

test('shows whether she is living, with the three capability layers apart', async () => {
  const dom = new JSDOM('<div id="root"></div>')
  const globals = { window: dom.window, document: dom.window.document, IS_REACT_ACT_ENVIRONMENT: true }
  const previous = new Map(Object.keys(globals).map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)]))
  for (const [key, value] of Object.entries(globals)) Object.defineProperty(globalThis, key, { configurable: true, value })
  let shown = alive()
  const bundle = await build({
    entryPoints: [new URL('./AgentAliveGroup.tsx', import.meta.url).pathname], bundle: true, write: false,
    platform: 'node', format: 'cjs', packages: 'external', define: { 'import.meta.env': '{}' }, loader: { '.css': 'empty' },
    plugins: [{ name: 'boundaries', setup(builder) {
      builder.onResolve({ filter: /(@lib\/icons|contexts\/I18nContext|services\/agent|\/settings)$/ }, ({ path }) => ({ path, external: true }))
    } }],
  })
  const fill = (text: string, values: Record<string, string> = {}) => text.replace(/\{(\w+)\}/g, (_, key: string) => values[key] ?? '')
  const mockRequire = (path: string) => {
    if (path.includes('I18nContext')) return { useConfigI18n: () => ({ t: { config: zh }, locale: 'zh-CN', format: fill }) }
    if (path.endsWith('services/agent')) return { agentService: { getHerAlive: async () => shown } }
    if (path.endsWith('/settings')) {
      return {
        SettingGroup: ({ title, children }: { title: string, children: React.ReactNode }) => createElement('section', { 'data-title': title }, children),
        InfoActionCard: ({ title, tone, children }: { title: string, tone: string, children: React.ReactNode }) => createElement('div', { 'data-verdict': tone }, title, children),
      }
    }
    if (path === '@lib/icons') return { LuActivity: () => null }
    return require(path)
  }
  const module = { exports: {} as typeof import('./AgentAliveGroup') }
  compileFunction(bundle.outputFiles[0].text, ['require', 'module', 'exports'])(mockRequire, module, module.exports)
  const root = createRoot(dom.window.document.getElementById('root'))
  const render = async () => {
    await act(async () => root.render(null))
    await act(async () => root.render(createElement(module.exports.AgentAliveGroup)))
  }
  const text = () => dom.window.document.getElementById('root').textContent ?? ''
  const verdict = () => dom.window.document.querySelector('[data-verdict]')
  try {
    await render()
    assert.equal(verdict().getAttribute('data-verdict'), 'success')
    assert.match(verdict().textContent, /她在活，上次有动静是10 秒前/)
    assert.match(text(), /近一天没有停过/)
    assert.match(text(), /这台主机自己睡了或停了 1 次，共 60 分钟，不算她停/)
    // Declared, approved, granted: three rows, the middle one said to be absent.
    const rows = [...dom.window.document.querySelectorAll('.agent-alive-end dt')].map(dt => [dt.textContent, dt.nextElementSibling.textContent])
    assert.deepEqual(rows.slice(2, 5), [
      ['它声明的能力', '让它办事 · extra（未知） · 记录同步的版本：3'],
      ['你批准的', '现在没有这一层：没有哪项能力需要你批准'],
      ['实际授予的', '让它办事 · 记录同步的版本：3'],
    ])
    assert.match(text(), /MyriadNative 0\.9\.1 · 连接契约 2 · 核心 3/)
    assert.match(text(), /没整理成、原样留作长期的 4 条（没经过归并去重）/)
    assert.match(text(), /回答被输出上限截断chat_remember 1 · memory_node_upkeep 2/)
    assert.match(text(), /没放上节点的话记下待补放 3 段，补放成功 2 批、又失败 1 批、出错停下 4 次，超过 7 天没放成丢掉 5 句/)
    assert.match(text(), /收回或忘掉后节点上的清理还等着扫的 1 条，扫清 6 次，扫时出错停下 2 次，写入时因依据已不在而丢掉的版本 3 个，新事实不成立没做的改口 4 次/)
    assert.match(text(), /收回后节点上的再次确认等着再问一次的 2 条，判成变了或都不是、只放下的 1 条，问不出结果、只放下的 3 条/)
    assert.doesNotMatch(text(), /连续出错/)
    assert.match(text(), /落后 12 行/)
    // What it told of frames: none dropped; two of hers acked and not read.
    assert.deepEqual(rows.slice(6, 8), [
      ['它接不了的帧', '没有'],
      ['站点以为送到了、它其实没读到', 'say：2 次，最近 30 分钟前'],
    ])
    assert.match(text(), /屏幕内容里被站点删掉的字段presence · others_text：4 次，最近 2 分钟前/)
    assert.match(text(), /让 Mac 办事发出 3 · 办完 2 · 那边在忙 1/)

    // Stopped past the alarm's ten minutes, every driver passed over.
    shown = alive((next) => {
      next.life.quietSecs = 1380
      next.life.gaps = { count: 23, secs: 16000 }
      next.life.drivers[0] = { ...next.life.drivers[0], skippedAt: NOW, skippedWhy: 'mind_elsewhere', skips: 23 }
    })
    await render()
    assert.equal(verdict().getAttribute('data-verdict'), 'danger')
    assert.match(verdict().textContent, /她停了 23 分钟/)
    assert.match(text(), /近一天停过 23 次/)
    assert.match(text(), /被跳过 23 次：她的心在一台设备上/)

    // Her persona off: not living, and not an alarm.
    shown = alive((next) => {
      next.life.personaOn = false
      next.life.shouldLive = false
      next.life.quietSecs = null
    })
    await render()
    assert.equal(verdict().getAttribute('data-verdict'), 'muted')
    assert.match(verdict().textContent, /人设关着/)

    // The sweep failing every time: her nodes not read, said and marked.
    shown = alive((next) => {
      next.memory.forgetting = { left_to_sweep: 1, swept: 6, sweep_errors: 9, left_out: 3, sweep_errors_in_a_row: 3 }
    })
    await render()
    assert.match(text(), /清理已连续出错 3 次：出错的那段对话里读不到她的节点。/)
    const stuck = [...dom.window.document.querySelectorAll('.agent-alive-rows dd')].find(dd => /连续出错/.test(dd.textContent ?? ''))
    assert.equal(stuck?.getAttribute('data-late'), 'true')

    // Her model: no answers line from a site without one; then answered;
    // then not answering, past the alarm, marked.
    assert.equal(dom.window.document.querySelector('.agent-alive-answers'), null)
    const answers = (unansweredInARow: number, alarmedFrom: string | null) =>
      alive((next) => {
        next.life.answers = {
          answeredAt: '2026-11-16T03:58:00Z',
          unansweredInARow,
          unansweredSince: unansweredInARow > 0 ? '2026-11-16T03:45:00Z' : null,
          alarmedFrom,
          alarmAfter: 10,
        }
      })
    shown = answers(0, null)
    await render()
    const line = () => dom.window.document.querySelector('.agent-alive-answers')
    assert.equal(line()?.textContent, '她的模型上次回答在 2 分钟前')
    assert.equal(line()?.getAttribute('data-late'), 'false')
    shown = answers(12, '2026-11-16T03:45:00Z')
    await render()
    assert.equal(line()?.textContent, '她的模型连续 12 次没回答，最早一次在 15 分钟前')
    assert.equal(line()?.getAttribute('data-late'), 'true')
  }
  finally {
    await act(async () => root.unmount())
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor)
      else delete (globalThis as Record<string, unknown>)[key]
    }
  }
})
