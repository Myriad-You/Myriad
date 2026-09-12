import type { ProgressEvent } from '../../services/agent'
import assert from 'node:assert/strict'
import { afterEach, mock, test } from 'node:test'
import { buildAgentPendingAction } from './agentAction'
import { DONE_LINGER_MS, ERROR_LINGER_MS } from './agentStatus'
import {
  clearAgentPendingAction,
  getAgentLaneLoading,
  getAgentPendingActionSnapshot,
  getAgentStatusSnapshot,
  pushAgentStatusEvent,
  resetAgentStatus,
  setAgentLaneLoading,
  setAgentPendingAction,
  setAgentStatusAwaitingConfirmation,
  setAgentStatusRecording,
  setAgentStatusThinking,
  subscribeAgentStatus,
} from './agentStatusStore'

const runStarted: ProgressEvent = { type: 'run_started', runId: 'run_1' }

const stepStarted: ProgressEvent = {
  type: 'step_started',
  stepId: 'step_1',
  stepIndex: 0,
  totalSteps: 2,
  capabilityName: 'search',
  description: '正在查天气',
}

afterEach(() => {
  mock.timers.reset()
  setAgentStatusRecording(false)
  setAgentLaneLoading('work', false)
  setAgentLaneLoading('chat', false)
  resetAgentStatus()
})

test('订阅者只在状态真的变了的时候被叫醒', () => {
  let notifications = 0
  const unsubscribe = subscribeAgentStatus(() => {
    notifications += 1
  })

  pushAgentStatusEvent(runStarted)
  assert.equal(getAgentStatusSnapshot().status, 'thinking')
  assert.equal(notifications, 1)

  pushAgentStatusEvent({ type: 'session_created', sessionId: 's1' })
  assert.equal(notifications, 1)

  pushAgentStatusEvent(stepStarted)
  assert.equal(notifications, 2)

  unsubscribe()
  pushAgentStatusEvent({ type: 'task_completed' } as ProgressEvent)
  assert.equal(notifications, 2)
})

test('状态没变时快照引用保持不变', () => {
  pushAgentStatusEvent(runStarted)
  const before = getAgentStatusSnapshot()
  pushAgentStatusEvent({
    type: 'session_title_updated',
    sessionId: 's1',
    title: '查天气',
  })
  assert.equal(getAgentStatusSnapshot(), before)
})

test('发出去立刻占住思考，已经在跑就不要打回去', () => {
  setAgentStatusThinking()
  assert.equal(getAgentStatusSnapshot().status, 'thinking')

  pushAgentStatusEvent(stepStarted)
  assert.equal(getAgentStatusSnapshot().status, 'working')

  setAgentStatusThinking()
  assert.equal(getAgentStatusSnapshot().status, 'working')
})

test('录音叠在事件状态之上，但不抢正在跑的任务', () => {
  setAgentStatusRecording(true)
  assert.equal(getAgentStatusSnapshot().status, 'listening')

  pushAgentStatusEvent(runStarted)
  pushAgentStatusEvent(stepStarted)
  assert.equal(getAgentStatusSnapshot().status, 'working')

  setAgentStatusRecording(false)
  assert.equal(getAgentStatusSnapshot().status, 'working')
})

test('完成与出错各自停留一会儿，然后自己退回空闲', () => {
  mock.timers.enable({ apis: ['setTimeout'] })

  pushAgentStatusEvent(runStarted)
  pushAgentStatusEvent({
    type: 'task_completed',
    taskId: 't1',
    success: true,
    response: { success: true, message: '', responseType: 'answer' },
  } as ProgressEvent)
  assert.equal(getAgentStatusSnapshot().status, 'done')

  mock.timers.tick(DONE_LINGER_MS - 1)
  assert.equal(getAgentStatusSnapshot().status, 'done')
  mock.timers.tick(1)
  assert.equal(getAgentStatusSnapshot().status, 'idle')

  pushAgentStatusEvent({ type: 'error', message: '网络断了', code: 'NETWORK' })
  assert.equal(getAgentStatusSnapshot().status, 'error')
  mock.timers.tick(DONE_LINGER_MS)
  assert.equal(getAgentStatusSnapshot().status, 'error')
  mock.timers.tick(ERROR_LINGER_MS - DONE_LINGER_MS)
  assert.equal(getAgentStatusSnapshot().status, 'idle')
})

test('中断立刻回空闲，不等停留时间，也不会被之前的定时器再改一次', () => {
  mock.timers.enable({ apis: ['setTimeout'] })

  pushAgentStatusEvent(runStarted)
  pushAgentStatusEvent({ type: 'error', message: '出错', code: 'X' })
  resetAgentStatus()
  assert.equal(getAgentStatusSnapshot().status, 'idle')

  pushAgentStatusEvent(runStarted)
  mock.timers.tick(ERROR_LINGER_MS * 2)
  assert.equal(getAgentStatusSnapshot().status, 'thinking')
})

test('敏感确认落到等回话那一档，带上问题本身', () => {
  pushAgentStatusEvent(runStarted)
  setAgentStatusAwaitingConfirmation('确认删除这 3 个文件？')
  assert.deepEqual(getAgentStatusSnapshot(), {
    status: 'needsInput',
    detail: '确认删除这 3 个文件？',
  })
})

function pendingAction() {
  return buildAgentPendingAction({
    confirmation: {
      confirmationId: 'c1',
      riskLevel: 'high',
      expiresInSeconds: 300,
      pendingSteps: [],
    },
    prompt: '确定要删掉这些吗？',
    nowMs: Date.now(),
  })
}

test('摆出操作卡片的同时把状态推到等回话 —— 两者是一件事', () => {
  setAgentPendingAction(pendingAction())
  assert.equal(getAgentStatusSnapshot().status, 'needsInput')
  assert.equal(getAgentStatusSnapshot().detail, '确定要删掉这些吗？')
  assert.equal(getAgentPendingActionSnapshot()?.id, 'c1')
})

test('状态一离开等回话，卡片自己就没了 —— 不会留在界面上问过去的事', () => {
  setAgentPendingAction(pendingAction())
  pushAgentStatusEvent({
    type: 'step_started',
    stepId: 's1',
    stepIndex: 0,
    totalSteps: 1,
    capabilityName: 'delete',
    description: '正在删除',
  })
  assert.equal(getAgentStatusSnapshot().status, 'working')
  assert.equal(getAgentPendingActionSnapshot(), null)
})

test('按 id 收卡片，收错的那张不动', () => {
  setAgentPendingAction(pendingAction())
  clearAgentPendingAction('another')
  assert.equal(getAgentPendingActionSnapshot()?.id, 'c1')
  clearAgentPendingAction('c1')
  assert.equal(getAgentPendingActionSnapshot(), null)
})

test('车道占用变了就算岛状态没变也要叫醒订阅者', () => {
  setAgentStatusThinking()
  let notifications = 0
  const unsubscribe = subscribeAgentStatus(() => {
    notifications += 1
  })
  setAgentLaneLoading('chat', true)
  assert.equal(getAgentLaneLoading('chat'), true)
  assert.equal(getAgentLaneLoading('work'), false)
  assert.equal(notifications, 1)
  setAgentLaneLoading('chat', true)
  assert.equal(notifications, 1)
  setAgentLaneLoading('chat', false)
  assert.equal(getAgentLaneLoading('chat'), false)
  assert.equal(notifications, 2)
  unsubscribe()
})

test('中断时卡片一起收走', () => {
  setAgentPendingAction(pendingAction())
  resetAgentStatus()
  assert.equal(getAgentPendingActionSnapshot(), null)
  assert.equal(getAgentStatusSnapshot().status, 'idle')
})

test('卡片出现和消失都会叫醒订阅者', () => {
  let notifications = 0
  const unsubscribe = subscribeAgentStatus(() => {
    notifications += 1
  })
  setAgentPendingAction(pendingAction())
  const afterSet = notifications
  assert.ok(afterSet > 0)
  clearAgentPendingAction('c1')
  assert.ok(notifications > afterSet)
  unsubscribe()
})
