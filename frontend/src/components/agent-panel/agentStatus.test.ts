import type { ProgressEvent } from '../../services/agent'
import type { AgentStatusState } from './agentStatus'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  agentStatusForLane,
  agentStatusIsActive,
  awaitingConfirmation,
  beginAgentRun,
  IDLE_AGENT_STATUS,
  reduceAgentStatus,
  withListening,
} from './agentStatus'

/** 按顺序喂一串事件，拿最终状态。 */
function run(...events: ProgressEvent[]): AgentStatusState {
  return events.reduce(reduceAgentStatus, IDLE_AGENT_STATUS)
}

const runStarted: ProgressEvent = { type: 'run_started', runId: 'run_1' }

const stepStarted: ProgressEvent = {
  type: 'step_started',
  stepId: 'step_1',
  stepIndex: 0,
  totalSteps: 2,
  capabilityName: 'search',
  description: '正在查天气',
}

test('思考链流式时岛上仍是思考，正文 token 才转到执行', () => {
  const thinking = run(runStarted, {
    type: 'thinking_token',
    token: '先算一下',
    done: false,
  })
  assert.equal(thinking.status, 'thinking')

  const answering = reduceAgentStatus(thinking, {
    type: 'summary_token',
    token: '你好',
    done: false,
  })
  assert.equal(answering.status, 'working')
})

test('接管后先是思考，排出步骤才转到执行', () => {
  assert.equal(run(runStarted).status, 'thinking')

  const planned = run(runStarted, {
    type: 'task_created',
    taskId: 't1',
    message: '这就去办',
    totalSteps: 2,
  })
  assert.equal(planned.status, 'thinking')
  assert.equal(planned.detail, '这就去办')

  const working = run(runStarted, stepStarted)
  assert.equal(working.status, 'working')
  assert.equal(working.detail, '正在查天气')
})

test('进度只由 progress 事件写入，步骤事件不去猜', () => {
  const started = run(runStarted, stepStarted)
  assert.equal(started.progress, undefined)

  const reported = run(runStarted, stepStarted, {
    type: 'progress',
    progress: 40,
    completedSteps: 1,
    totalSteps: 2,
    message: '',
  })
  assert.equal(reported.progress, 40)

  // 下一步开始时进度保持，不回退也不跳
  const nextStep = reduceAgentStatus(reported, {
    ...stepStarted,
    stepId: 'step_2',
    stepIndex: 1,
    description: '正在整理结果',
  })
  assert.equal(nextStep.progress, 40)
  assert.equal(nextStep.detail, '正在整理结果')
})

test('重试把原因顶到台面上，仍算在执行', () => {
  const retrying = run(runStarted, stepStarted, {
    type: 'step_retrying',
    stepId: 'step_1',
    stepIndex: 0,
    retryCount: 1,
    maxRetries: 3,
    reason: '接口超时，换个参数再试',
  })
  assert.equal(retrying.status, 'working')
  assert.equal(retrying.detail, '接口超时，换个参数再试')
})

test('要用户回话时带上问题本身', () => {
  const asking = run(runStarted, stepStarted, {
    type: 'waiting_for_input',
    taskId: 't1',
    questionId: 'q1',
    questionType: 'choice',
    question: '要发给谁？',
    required: true,
  })
  assert.equal(asking.status, 'needsInput')
  assert.equal(asking.detail, '要发给谁？')

  // 敏感确认不走 SSE，但落到同一档
  assert.deepEqual(awaitingConfirmation('确认删除这 3 个文件？'), {
    status: 'needsInput',
    detail: '确认删除这 3 个文件？',
  })
})

test('收尾分成完成与出错两档', () => {
  const done = run(runStarted, stepStarted, {
    type: 'task_completed',
    taskId: 't1',
    success: true,
    response: { success: true, message: '', responseType: 'answer' },
  } as ProgressEvent)
  assert.equal(done.status, 'done')
  assert.equal(done.progress, 100)

  const failed = run(runStarted, stepStarted, {
    type: 'task_completed',
    taskId: 't1',
    success: false,
    response: { success: false, message: '', responseType: 'error' },
  } as ProgressEvent)
  assert.equal(failed.status, 'error')

  const errored = run(runStarted, {
    type: 'error',
    message: '网络断了',
  })
  assert.equal(errored.status, 'error')
  assert.equal(errored.detail, '网络断了')
})

test('形象与调试事件不改状态 —— 心情不是进度', () => {
  const working = run(runStarted, stepStarted)

  const noise: ProgressEvent[] = [
    { type: 'session_created', sessionId: 's1' },
    { type: 'session_title_updated', sessionId: 's1', title: '查天气' },
    {
      type: 'merope_state_changed',
      mood: 'happy',
      activity: 'idle',
    } as ProgressEvent,
    { type: 'performance_plan', performance: {} } as ProgressEvent,
    {
      type: 'step_debug',
      phase: 'start',
      stepId: 'step_1',
      capabilityId: 'search',
      isDynamic: false,
    } as ProgressEvent,
  ]

  for (const event of noise) {
    assert.deepEqual(reduceAgentStatus(working, event), working)
  }
})

test('本地发出去之后立刻是思考，等 SSE 接管', () => {
  assert.equal(beginAgentRun().status, 'thinking')
})

test('录音只在没事干的时候顶到最前', () => {
  assert.equal(withListening(IDLE_AGENT_STATUS, true).status, 'listening')
  assert.equal(withListening({ status: 'done' }, true).status, 'listening')
  assert.equal(withListening({ status: 'error' }, true).status, 'listening')

  // 任务跑着时，用户更需要看到它走到哪一步
  const working = run(runStarted, stepStarted)
  assert.deepEqual(withListening(working, true), working)
  assert.deepEqual(withListening({ status: 'needsInput', detail: '?' }, true), {
    status: 'needsInput',
    detail: '?',
  })

  assert.deepEqual(withListening(working, false), working)
})

test('当前档没在跑时，思考和执行收成空闲，别的岛状态不动', () => {
  assert.equal(agentStatusForLane('thinking', false), 'idle')
  assert.equal(agentStatusForLane('working', false), 'idle')
  assert.equal(agentStatusForLane('thinking', true), 'thinking')
  assert.equal(agentStatusForLane('working', true), 'working')
  assert.equal(agentStatusForLane('needsInput', false), 'needsInput')
  assert.equal(agentStatusForLane('done', false), 'done')
  assert.equal(agentStatusForLane('error', false), 'error')
  assert.equal(agentStatusForLane('listening', false), 'listening')
  assert.equal(agentStatusForLane('idle', true), 'idle')
})

test('除空闲外都要岛留在场上 —— 移动端顶替导航岛就看这个', () => {
  assert.equal(agentStatusIsActive('idle'), false)
  for (const status of [
    'listening',
    'thinking',
    'working',
    'needsInput',
    'done',
    'error',
  ] as const) {
    assert.equal(agentStatusIsActive(status), true)
  }
})
