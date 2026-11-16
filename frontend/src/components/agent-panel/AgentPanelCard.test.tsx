import type { Root } from 'react-dom/client'
import type { CardAnswerReply, CardLine, CardNow } from '../../services/agent/types'
import assert from 'node:assert/strict'
import { createRequire } from 'node:module'
import { after, afterEach, beforeEach, it } from 'node:test'
import { act, createElement } from 'react'
import zhCN from '../../i18n/zh-CN.json'
import { AgentPanelCard } from './AgentPanelCard'
import { withToldLines } from './agentPieces'
import { restoreCardLine, restoreSessionMessage } from './sessionHistoryMessage'

const require = createRequire(import.meta.url)
const { JSDOM } = require(require.resolve('jsdom', { paths: [require.resolve('isomorphic-dompurify')] }))
const dom = new JSDOM('<div id="root"></div>')
dom.window.matchMedia = () => ({ matches: false, addEventListener() {}, removeEventListener() {} })
dom.window.HTMLElement.prototype.scrollIntoView = function () {}
const prior = new Map<string, PropertyDescriptor | undefined>()
for (const [key, value] of Object.entries({
  window: dom.window,
  document: dom.window.document,
  navigator: dom.window.navigator,
  HTMLElement: dom.window.HTMLElement,
  IS_REACT_ACT_ENVIRONMENT: true,
})) {
  prior.set(key, Object.getOwnPropertyDescriptor(globalThis, key))
  Object.defineProperty(globalThis, key, { configurable: true, value })
}
const { createRoot } = await import('react-dom/client')
after(() => {
  dom.window.close()
  for (const [key, descriptor] of prior) {
    if (descriptor) Object.defineProperty(globalThis, key, descriptor)
    else Reflect.deleteProperty(globalThis, key)
  }
})

const copy = zhCN.agentPanel.card
function format(template: string, params: Record<string, string | number>) {
  return template.replace(/\{(\w+)\}/g, (_, key: string) => String(params[key] ?? ''))
}

function now(over: Partial<CardNow['card']> = {}, rest: Partial<CardNow> = {}): CardNow {
  return {
    version: 'v1',
    waiting: true,
    card: {
      at: 1760000000000,
      id: 'c_1',
      kind: 'decide',
      scene: 'agents',
      state: 'waiting_owner',
      title: '现在推送吗',
      facts: [
        { text: '会话停下来问：现在推送吗', ref: 'session:s-1' },
        { text: '这件事是你亲自派的', ref: 'session:s-1' },
      ],
      view: { text: '先等检查跑完再推。', because: [2] },
      actions: [
        { id: 'open', label: '去会话里说' },
        { id: 'push', label: '推送' },
        { id: 'wait', label: '等等' },
      ],
      ...over,
    },
    ...rest,
  }
}

function line(card?: CardNow, over: Partial<CardLine> = {}): CardLine {
  return {
    id: 'pro_7',
    role: 'card',
    content: '',
    createdAt: '2026-11-12T10:00:00+09:00',
    cardId: 'c_1',
    line: 'card_x',
    ...(card ? { card } : {}),
    ...over,
  }
}

let root: Root
beforeEach(() => {
  root = createRoot(document.getElementById('root')!)
})
afterEach(async () => {
  await act(async () => root.unmount())
})

async function render(
  cardLine: CardLine,
  answer: (cardId: string, version: string, action: string) => Promise<CardAnswerReply> = async () => ({ kept: true }),
) {
  await act(async () => root.render(createElement(AgentPanelCard, { line: cardLine, copy, format, answer })))
}

function buttons() {
  return [...document.querySelectorAll<HTMLButtonElement>('.agent-panel-card-actions button')].map(button => button.textContent)
}
function text() {
  return document.getElementById('root')!.textContent ?? ''
}

it('shows title, state, numbered facts, the view resting on its facts, and no open button', async () => {
  await render(line(now()))
  assert.equal(document.querySelector('.agent-panel-card-title')?.textContent, '现在推送吗')
  assert.equal(document.querySelector('.agent-panel-card-state')?.textContent, copy.states.waiting_owner)
  const facts = [...document.querySelectorAll('.agent-panel-card-fact')]
  assert.deepEqual(facts.map(fact => fact.querySelector('.agent-panel-card-fact-text')?.textContent), ['会话停下来问：现在推送吗', '这件事是你亲自派的'])
  assert.deepEqual(facts.map(fact => fact.querySelector('.agent-panel-card-fact-n')?.textContent), ['1', '2'])
  assert.deepEqual(facts.map(fact => fact.getAttribute('data-cited')), [null, 'true'])
  assert.match(text(), /先等检查跑完再推。/)
  const cite = document.querySelector<HTMLButtonElement>('.agent-panel-card-cite')!
  assert.equal(cite.textContent, '2')
  assert.equal(cite.getAttribute('aria-controls'), facts[1].id)
  assert.equal(cite.getAttribute('aria-label'), format(copy.because, { n: 2 }))
  assert.deepEqual(buttons(), ['推送', '等等'])
  assert.doesNotMatch(text(), /去会话里说/)
  assert.equal(document.querySelector('[data-card-holder]')?.getAttribute('data-card-holder'), 'c_1')
})

it('a line of the card that no longer holds it is one line pointing to the one that does', async () => {
  await render(line(undefined, { id: 'pro_3', role: 'assistant', content: '有件事要你定' }))
  assert.equal(document.querySelector('.agent-panel-card'), null)
  const pointer = document.querySelector<HTMLButtonElement>('.agent-panel-card-pointer')!
  assert.equal(pointer.textContent, copy.moved)
  const holder = document.createElement('div')
  holder.dataset.cardHolder = 'c_1'
  holder.tabIndex = -1
  let scrolled = 0
  holder.scrollIntoView = () => { scrolled++ }
  document.body.append(holder)
  await act(async () => pointer.click())
  assert.equal(scrolled, 1)
  assert.equal(document.activeElement, holder)
  holder.remove()
})

it('answering sends the version seen, holds the buttons, then shows the label chosen', async () => {
  const calls: unknown[][] = []
  const reply = Promise.withResolvers<CardAnswerReply>()
  await render(line(now()), (...args) => { calls.push(args); return reply.promise })
  const push = [...document.querySelectorAll<HTMLButtonElement>('.agent-panel-card-actions button')][0]
  await act(async () => push.click())
  assert.deepEqual(calls, [['c_1', 'v1', 'push']])
  assert.ok([...document.querySelectorAll<HTMLButtonElement>('.agent-panel-card-actions button')].every(button => button.disabled))
  await act(async () => reply.resolve({ kept: true, card: now({}, { waiting: false, answered: { at: 1, action: 'push', version: 'v1' } }) }))
  assert.deepEqual(buttons(), [])
  assert.equal(document.querySelector('.agent-panel-card-chosen')?.textContent, format(copy.chose, { label: '推送' }))
})

it('a stale answer shows the latest card, still waiting; a failure says so and lets them try again', async () => {
  const later = now({ title: '现在推送吗（第二版）', actions: [{ id: 'push', label: '推送' }, { id: 'drop', label: '不推了' }] }, { version: 'v2' })
  let fail = false
  const calls: unknown[][] = []
  await render(line(now()), async (...args) => {
    calls.push(args)
    if (fail) throw Object.assign(new Error('x'), { status: 500, code: 'internal_error' })
    return { kept: true, code: 'card_stale', card: later }
  })
  await act(async () => document.querySelector<HTMLButtonElement>('.agent-panel-card-actions button')!.click())
  assert.equal(document.querySelector('.agent-panel-card-title')?.textContent, '现在推送吗（第二版）')
  assert.deepEqual(buttons(), ['推送', '不推了'])
  assert.equal(document.querySelector('.agent-panel-card-note')?.textContent, copy.stale)
  fail = true
  await act(async () => document.querySelector<HTMLButtonElement>('.agent-panel-card-actions button')!.click())
  assert.deepEqual(calls[1], ['c_1', 'v2', 'push'])
  assert.equal(document.querySelector('.agent-panel-card-note')?.textContent, copy.failed)
  assert.ok([...document.querySelectorAll<HTMLButtonElement>('.agent-panel-card-actions button')].every(button => !button.disabled))
})

it('a card of a kind never seen shows the same way; one neither waiting nor answered has no buttons', async () => {
  await render(line(now({ kind: 'weather_report', state: 'some_new_state', view: undefined }, { waiting: false })))
  assert.equal(document.querySelector('.agent-panel-card')?.getAttribute('data-kind'), 'weather_report')
  assert.equal(document.querySelector('.agent-panel-card-title')?.textContent, '现在推送吗')
  assert.equal(document.querySelectorAll('.agent-panel-card-fact').length, 2)
  assert.equal(document.querySelector('.agent-panel-card-state'), null)
  assert.deepEqual(buttons(), [])
  assert.equal(document.querySelector('.agent-panel-card-chosen'), null)
})

it('card lines go among the page by time, as her lines, never as forgettable session lines', () => {
  const messages = [
    { id: 1, role: 'user' as const, content: '在吗', createdAt: '2026-11-12T00:59:00Z' },
    { id: 2, role: 'assistant' as const, content: '在', createdAt: '2026-11-12T01:01:00Z' },
  ].map(row => restoreSessionMessage(row, 's'))
  const cards = [
    line(undefined, { id: 'pro_1', createdAt: '2026-11-12T00:00:00Z' }),
    line(undefined, { id: 'pro_2', role: 'assistant', content: '有件事要你定', createdAt: '2026-11-12T01:00:00Z' }),
    line(now(), { id: 'pro_3', createdAt: '2026-11-12T01:02:00Z' }),
  ].map(card => restoreCardLine(card, 's'))
  const merged = withToldLines(messages, cards)
  assert.deepEqual(merged.map(message => message.id), ['loaded_1', 'card_pro_2', 'loaded_2', 'card_pro_3'])
  assert.equal(merged[1].role, 'assistant')
  assert.equal(merged[1].content, '有件事要你定')
  assert.equal(merged[3].content, '')
  assert.equal(merged[3].cardLine?.card?.version, 'v1')
  assert.doesNotMatch(merged[1].id, /^loaded_\d+$/)
})
