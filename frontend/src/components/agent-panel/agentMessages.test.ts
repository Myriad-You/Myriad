import type { AgentMessage } from './agentMessages'
import assert from 'node:assert/strict'
import { afterEach, test } from 'node:test'
import {
  getAgentMessageCountSnapshot,
  getAgentMessagesSnapshot,
  setAgentMessages,
  subscribeAgentMessages,
} from './agentMessages'

function message(overrides: Partial<AgentMessage> = {}): AgentMessage {
  return {
    id: 'm1',
    role: 'assistant',
    content: '你好',
    ...overrides,
  }
}

afterEach(() => {
  setAgentMessages([])
})

test('换了内容才叫醒订阅者', () => {
  let notifications = 0
  const unsubscribe = subscribeAgentMessages(() => {
    notifications += 1
  })

  setAgentMessages([message()])
  assert.equal(notifications, 1)

  setAgentMessages([message()])
  assert.equal(notifications, 1)

  setAgentMessages([message({ content: '你好呀' })])
  assert.equal(notifications, 2)

  unsubscribe()
  setAgentMessages([message({ content: '再改一次' })])
  assert.equal(notifications, 2)
})

test('思考过程变了也要叫醒订阅者', () => {
  setAgentMessages([message()])
  const snapshot = getAgentMessagesSnapshot()
  setAgentMessages([message({ thought: '先查天气' })])
  assert.notEqual(getAgentMessagesSnapshot(), snapshot)
  assert.equal(getAgentMessagesSnapshot()[0]?.thought, '先查天气')
})

test('条数、角色、状态、图片数量任一变了都算变了', () => {
  const base = [message()]
  setAgentMessages(base)
  const snapshot = getAgentMessagesSnapshot()

  setAgentMessages([message({ role: 'user' })])
  assert.notEqual(getAgentMessagesSnapshot(), snapshot)

  setAgentMessages([message()])
  setAgentMessages([message({ state: 'streaming' })])
  assert.equal(getAgentMessagesSnapshot()[0].state, 'streaming')

  setAgentMessages([message()])
  setAgentMessages([message({ imageUrls: ['/a.png'] })])
  assert.equal(getAgentMessagesSnapshot()[0].imageUrls?.length, 1)

  setAgentMessages([message(), message({ id: 'm2' })])
  assert.equal(getAgentMessagesSnapshot().length, 2)
})

test('没变的那条沿用原来的对象，流式追加不拖着整列重建', () => {
  setAgentMessages([
    message({ id: 'a', content: '问' }),
    message({ id: 'b', role: 'assistant', content: '答' }),
  ])
  const snap = getAgentMessagesSnapshot()
  setAgentMessages([
    message({ id: 'a', content: '问' }),
    message({ id: 'b', role: 'assistant', content: '答呀' }),
  ])
  const next = getAgentMessagesSnapshot()
  assert.equal(next[0], snap[0])
  assert.notEqual(next[1], snap[1])
  assert.equal(next[1].content, '答呀')
})

test('外壳只关心条数：内容变了 count 不变', () => {
  setAgentMessages([message()])
  assert.equal(getAgentMessageCountSnapshot(), 1)
  setAgentMessages([message({ content: '你好呀' })])
  assert.equal(getAgentMessageCountSnapshot(), 1)
  setAgentMessages([message(), message({ id: 'm2' })])
  assert.equal(getAgentMessageCountSnapshot(), 2)
})

test('清空之后拿到的是同一个空数组，引用稳定', () => {
  setAgentMessages([message()])
  setAgentMessages([])
  const first = getAgentMessagesSnapshot()
  setAgentMessages([])
  assert.equal(getAgentMessagesSnapshot(), first)
  assert.equal(first.length, 0)
})
