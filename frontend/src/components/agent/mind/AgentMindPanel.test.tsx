import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import test from 'node:test'
import { compileFunction } from 'node:vm'
import { act, createElement } from 'react'
import { createRoot } from 'react-dom/client'
import merope from '../../../i18n/merope.zh-CN.json'

const require = createRequire(import.meta.url)
const { JSDOM } = require(require.resolve('jsdom', { paths: [require.resolve('isomorphic-dompurify')] }))
const { build } = createRequire(import.meta.resolve('tsx/package.json'))('esbuild')

const day = (offset: number) => new Date(Date.UTC(2026, 8, 20 + offset)).toISOString()

/** A made-up snapshot in the shape `GET /api/agent/mind` returns. */
const fixture = {
  generatedAt: day(10),
  her: {
    selfStory: [{ text: '这周听了很多慢歌，比以前安静。', at: day(8), current: true }],
    wants: [
      { want: '把一张专辑从头听完', why: '那首歌一直在脑子里', reach: 'on_your_own', since: day(6), notes: [{ at: day(9), note: '听到第三首了' }] },
      { want: '再被什么狠狠打动一次', why: '最近听的都差一口气', reach: 'on_your_own', longing: true, since: day(7), notes: [] },
    ],
    wantsEnded: [{ text: '想要的「看完一本连载」实现了：昨晚看完了', at: day(7), current: true }],
    views: [
      { text: '以前觉得电子乐太吵', at: day(1), current: false, endedAt: day(5), endedWhy: 'superseded' },
      { text: '电子乐也能很安静', at: day(5), current: true },
    ],
    questions: [],
    corrected: [],
    days: [{ text: '今天和两个人聊了天。', at: day(9), current: true }],
    doingThisWeek: [{ at: day(9), text: 'listening 「夜航」 (moved you)' }],
    voice: [
      { week: '2026-09-14', lines: 80, peopleLines: 60, drift: null, peopleDrift: null, fromPeople: 0.91 },
      { week: '2026-09-21', lines: 120, peopleLines: 70, drift: 0.58, peopleDrift: 0.54, fromPeople: 0.8 },
    ],
    vitals: [{
      day: '2026-09-30', calls: 226, failedCalls: 3, inputTokens: 509856, busiest: [['doing_choice', 81]], kept: { doing: 67 },
      unreadable: 2, things: 67, ownMinutes: 399, lazedMinutes: 0, landed: { liked: 20, moved: 20 }, replies: 8, replyP50: 7.6, replyP90: 10.3,
      notesLeanOn: [['不是这个', 0.36]], repliesLeanOn: [], repliesAsking: 0.72, openersLeanOn: [['怎么这个点', 0.6]], proactive: 3, proactiveAnswered: 1,
      learned: 0,
      alerts: [{ kind: 'unreadable', count: 2 }, { kind: 'notesLeanOn', phrase: '不是这个', percent: 36 }, { kind: 'repliesAsking', percent: 72 }, { kind: 'proactiveUnanswered', sent: 6, answered: 1 }, { kind: 'nothingLearned', days: 2 }],
    }],
    taste: { likedBy: [{ kind: 'song', name: 'ヨルシカ' }], notForHer: [] },
    pace: {
      usualMinutes: 260,
      daysPastUsual: 2,
      tone: 'flat',
      days: [{ day: '2026-09-29', minutes: 390, lazed: 120 }, { day: '2026-09-30', minutes: 75, lazed: 480 }],
    },
  },
  people: [{
    id: 7,
    name: '阿明',
    firstTalked: day(-20),
    daysTalked: 18,
    us: [
      { text: '刚认识，话不多。', at: day(0), current: false, endedAt: day(6), endedWhy: 'superseded' },
      { text: '总在半夜来吐槽工作的朋友，嘴上嫌他烦，其实挺担心他。', at: day(6), current: true },
    ],
    lands: [{ text: '一逗他就接着闹；讲长了他只回「嗯」。', at: day(7), current: true }],
    chatDays: [{ day: '2026-09-25', text: '玩了海龟汤：天台男人拍星轨那道。', current: true }],
    sore: [
      { what: '他说我的歌单全是垃圾', weight: 'deep', since: day(3), mended: day(4), where: 'private', status: 'let_go', endedAt: day(5) },
      { what: '他说我暴躁', weight: 'petty', since: day(8), where: 'group', status: 'open' },
    ],
    threads: [{ about: '考试', then: '问他考得怎么样', at: day(8), current: true }],
    bits: [{ handle: '连环你好呀', how: '连着发好几遍打招呼', at: day(2), current: true }],
  }],
  groups: [{
    venue: 'onebot:123',
    lands: [{ text: '接梗很快，但刷屏时插话没人理。', at: day(8), current: true }],
    guesses: [{ stranger: '路人甲', candidate: '阿明', sure: 'likely', why: '他提到了年糕和星穹互娱。', at: day(9) }],
    days: [{ day: '2026-09-28', text: '大家在吵海带汤算不算韩国风', current: true }],
    bits: [],
    sore: [{ what: '当众笑我', weight: 'hurt', since: day(7), where: 'group', who: '阿明', status: 'open' }],
  }],
}

test('her mind shows herself, each person and each group, with how each changed', async () => {
  const dom = new JSDOM('<div id="root"></div>')
  const globals = { window: dom.window, document: dom.window.document, IS_REACT_ACT_ENVIRONMENT: true }
  const previous = new Map(Object.keys(globals).map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)]))
  for (const [key, value] of Object.entries(globals)) Object.defineProperty(globalThis, key, { configurable: true, value })
  // Run once against a real snapshot too: MEROPE_MIND_FIXTURE=<file>.
  const snapshot = process.env.MEROPE_MIND_FIXTURE
    ? JSON.parse(readFileSync(process.env.MEROPE_MIND_FIXTURE, 'utf8'))
    : fixture
  let loads = 0
  const bundle = await build({
    entryPoints: [new URL('./AgentMindPanel.tsx', import.meta.url).pathname], bundle: true, write: false,
    platform: 'node', format: 'cjs', packages: 'external', define: { 'import.meta.env': '{}' },
    plugins: [{ name: 'boundaries', setup(builder) {
      builder.onResolve({ filter: /(\/settings|contexts\/I18nContext|services\/agent)$/ }, ({ path }) => ({ path, external: true }))
    } }],
  })
  const mockRequire = (path: string) => {
    if (path.endsWith('/settings')) {
      return {
        SettingGroup: ({ title, description, children }: { title?: string; description?: string; children?: React.ReactNode }) =>
          createElement('section', null, createElement('h3', null, title), description ? createElement('p', null, description) : null, children),
        SettingsButton: ({ children, onClick }: { children: React.ReactNode; onClick: () => void }) => createElement('button', { onClick, 'data-refresh': '' }, children),
        SegmentedControl: ({ options, onChange }: { options: { value: string; label: string }[]; onChange: (value: string) => void }) =>
          createElement('div', null, options.map(option => createElement('button', { key: option.value, 'data-tab': option.value, onClick: () => onChange(option.value) }, option.label))),
      }
    }
    if (path.endsWith('/I18nContext')) return { useI18n: () => ({ t: { merope }, locale: 'zh-CN' }) }
    if (path.endsWith('/services/agent')) return { agentService: { getMind: async () => { loads++; return snapshot } } }
    return require(path)
  }
  const module = { exports: {} as typeof import('./AgentMindPanel') }
  compileFunction(bundle.outputFiles[0].text, ['require', 'module', 'exports'])(mockRequire, module, module.exports)
  const root = createRoot(dom.window.document.getElementById('root'))
  const text = () => dom.window.document.body.textContent as string
  const tab = (name: string) => act(async () => dom.window.document.querySelector(`[data-tab="${name}"]`).click())
  try {
    await act(async () => root.render(createElement(module.exports.default)))
    assert.equal(loads, 1)
    assert.ok(text().includes(merope.mind.her.selfStory))
    if (snapshot === fixture) {
      assert.ok(text().includes('把一张专辑从头听完'))
      assert.ok(text().includes(merope.mind.reach.on_your_own))
      assert.ok(text().includes('听到第三首了'))
      // A view she changed her mind about stays, marked as rewritten.
      assert.ok(text().includes('以前觉得电子乐太吵'))
      assert.ok(text().includes(merope.mind.ended.superseded))
      // Her voice, week by week.
      assert.ok(text().includes(merope.mind.her.voice))
      assert.ok(text().includes('0.58') && text().includes('0.80'))
      // Her pace, and a longing marked as one.
      assert.ok(text().includes(merope.mind.her.pace) && text().includes(merope.mind.pace.tone.flat))
      assert.ok(text().includes('6.5') && text().includes('8.0'))
      assert.ok(text().includes(merope.mind.longing))
      assert.ok(text().includes(merope.mind.her.taste) && text().includes('ヨルシカ 的歌'))
      // Her vital signs, with what is worth raising.
      assert.ok(text().includes(merope.mind.her.vitals) && text().includes('doing_choice 81'))
      assert.ok(text().includes('有 2 条她自己的经历读不出来') && text().includes('心得里 36% 出现「不是这个」'))
      assert.ok(text().includes('72% 的回复以问句收尾'))
      assert.ok(text().includes('这一周主动发了 6 条，只有 1 条有回应') && text().includes('3 条，1 条有回应'))
      assert.ok(text().includes('学到新东西：0') && text().includes('连着 2 天没学到任何新东西'))
    }
    await tab('people')
    if (snapshot === fixture) {
      assert.ok(text().includes('阿明'))
      assert.ok(text().includes(merope.mind.person.us))
      assert.ok(text().includes('总在半夜来吐槽工作的朋友'))
      assert.ok(text().includes('刚认识，话不多。'))
      assert.ok(text().includes(merope.mind.person.lands) && text().includes('讲长了他只回'))
      assert.ok(text().includes(merope.mind.person.chatDays) && text().includes('天台男人拍星轨'))
      assert.ok(text().includes(merope.mind.weight.deep))
      assert.ok(text().includes(merope.mind.status.let_go))
      assert.ok(text().includes(merope.mind.mended))
      assert.ok(text().includes('聊过 18 天'))
    }
    await tab('groups')
    if (snapshot === fixture) {
      assert.ok(text().includes('onebot:123'))
      assert.ok(text().includes('大家在吵海带汤算不算韩国风'))
      assert.ok(text().includes('路人甲 可能是 阿明'))
      assert.ok(text().includes(merope.mind.group.lands) && text().includes('刷屏时插话没人理'))
      assert.ok(text().includes(merope.mind.guess.sure.likely))
      assert.ok(text().includes('阿明：'))
      assert.ok(text().includes(merope.mind.status.open))
    } else {
      for (const group of snapshot.groups) assert.ok(text().includes(group.venue))
    }
    await act(async () => dom.window.document.querySelector('[data-refresh]').click())
    assert.equal(loads, 2)
  } finally {
    await act(async () => root.unmount())
    dom.window.close()
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor)
      else Reflect.deleteProperty(globalThis, key)
    }
  }
})
