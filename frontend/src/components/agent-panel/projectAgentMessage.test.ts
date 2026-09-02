import type { ChatMessage } from './engineTypes'
import assert from 'node:assert/strict'
import { afterEach, test } from 'node:test'
import { getAgentMessagesSnapshot, setAgentMessages } from './agentMessages'
import { executionStepsFromHistory } from './engineTypes'
import {
  projectAgentMessage,
  projectThought,
  syncProjectedMessages,
} from './projectAgentMessage'

function chat(overrides: Partial<ChatMessage> = {}): ChatMessage {
  return {
    id: 'a',
    sessionId: 's',
    role: 'assistant',
    content: '你好',
    createdAt: new Date(1_700_000_000_000),
    ...overrides,
  }
}

afterEach(() => {
  setAgentMessages([])
})

test('投影只留下界面要的字段', () => {
  const projected = projectAgentMessage(
    chat({
      content: '在说',
      taskExecution: {
        taskId: 't',
        status: 'processing',
        progress: 10,
        steps: [
          {
            id: 'step-1',
            name: '读',
            status: 'running',
            durationMs: 12,
            message: '还在',
          },
        ],
      },
    }),
  )
  assert.equal(projected.state, 'streaming')
  assert.equal(projected.steps?.[0]?.status, 'running')
  assert.equal(projected.steps?.[0]?.note, '还在')
  assert.equal(projected.at, 1_700_000_000_000)
})

test('还没排出步骤时，计划描述先当成待跑的过程', () => {
  const projected = projectAgentMessage(
    chat({
      content: '',
      taskExecution: {
        taskId: 't',
        status: 'processing',
        progress: 0,
        steps: [],
        planStepDescriptions: ['读页', '写回复'],
      },
    }),
  )
  assert.equal(projected.state, 'streaming')
  assert.deepEqual(
    projected.steps?.map((step) => [step.name, step.status]),
    [
      ['读页', 'pending'],
      ['写回复', 'pending'],
    ],
  )
})

test('正文里的 think 标签投影后只留答案，思考进过程区', () => {
  const projected = projectAgentMessage(
    chat({
      content: '<think>先加再报</think>\n\n答案是 4',
      taskExecution: {
        taskId: '',
        status: 'processing',
        progress: 40,
        steps: [],
      },
    }),
  )
  assert.equal(projected.thought, '先加再报')
  assert.equal(projected.content, '答案是 4')
})

test('思考链若被抄进正文，投影时从正文剥掉', () => {
  const thought = '这是一段足够长的思考过程，用来判断怎么回答。'
  const projected = projectAgentMessage(
    chat({
      content: `${thought}\n\n你好呀`,
      taskExecution: {
        taskId: '',
        status: 'processing',
        progress: 40,
        steps: [],
        reasoning: thought,
      },
    }),
  )
  assert.equal(projected.thought, thought)
  assert.equal(projected.content, '你好呀')
})

test('模型思考链写在 reasoning 上，投影成 thought，不进正文', () => {
  const projected = projectAgentMessage(
    chat({
      content: '答案是 4',
      taskExecution: {
        taskId: '',
        status: 'processing',
        progress: 40,
        steps: [],
        reasoning: '2 + 2，所以是 4。',
      },
    }),
  )
  assert.equal(projected.thought, '2 + 2，所以是 4。')
  assert.equal(projected.content, '答案是 4')
  assert.equal(projected.state, 'streaming')
})

test('聊天路径：进度句和 Planner 推理要变成思考过程，不能跟正文撞车', () => {
  const placeholder = chat({
    content: '',
    taskExecution: {
      taskId: '',
      status: 'processing',
      progress: 5,
      steps: [],
      statusMessage: '正在理解你的请求...',
    },
  })
  assert.equal(projectThought(placeholder), '正在理解你的请求...')
  assert.equal(projectAgentMessage(placeholder).state, 'streaming')
  assert.equal(projectAgentMessage(placeholder).thought, '正在理解你的请求...')

  const decided = chat({
    content: '',
    taskExecution: {
      taskId: '',
      status: 'processing',
      progress: 5,
      steps: [],
      statusMessage: '正在理解你的请求...',
      reasoning: '这是寒暄，直接聊。',
    },
  })
  assert.equal(projectThought(decided), '这是寒暄，直接聊。')

  const answered = chat({
    content: '你好呀',
    taskExecution: {
      taskId: '',
      status: 'completed',
      progress: 100,
      steps: [],
      statusMessage: '你好呀',
      reasoning: '这是寒暄，直接聊。',
    },
  })
  assert.equal(projectThought(answered), '这是寒暄，直接聊。')
  assert.equal(projectAgentMessage(answered).state, undefined)
  assert.equal(projectAgentMessage(answered).thought, '这是寒暄，直接聊。')
})

test('终态进度句和与正文相同的快照都不是思考过程', () => {
  assert.equal(
    projectThought(
      chat({
        content: '东京 25°C',
        taskExecution: {
          taskId: 't',
          status: 'completed',
          progress: 100,
          steps: [],
          statusMessage: '完成',
        },
      }),
    ),
    undefined,
  )
  assert.equal(
    projectThought(
      chat({
        content: '好的，我去查天气',
        taskExecution: {
          taskId: 't',
          status: 'processing',
          progress: 20,
          steps: [],
          statusMessage: '好的，我去查天气',
        },
      }),
    ),
    undefined,
  )
})

test('Planner 步骤在还没 step_started 时先当成待跑的过程', () => {
  const projected = projectAgentMessage(
    chat({
      content: '',
      taskExecution: {
        taskId: '',
        status: 'processing',
        progress: 5,
        steps: [],
        reasoning: '要查完再写。',
        planStepDescriptions: ['查天气', '写回复'],
      },
    }),
  )
  assert.equal(projected.thought, '要查完再写。')
  assert.deepEqual(
    projected.steps?.map((step) => [step.name, step.status]),
    [
      ['查天气', 'pending'],
      ['写回复', 'pending'],
    ],
  )
})

test('会话里的 stepHistory 能还原成执行步骤', () => {
  const steps = executionStepsFromHistory([
    {
      stepId: 's1',
      capabilityName: '查天气',
      status: 'completed',
      durationMs: 120,
      outputSummary: '25°C',
    },
    {
      step_id: 's2',
      capability_name: '写回复',
      status: 'failed',
      error: '超时',
    },
  ])
  assert.equal(steps[0]?.status, 'completed')
  assert.equal(steps[0]?.name, '查天气')
  assert.equal(steps[1]?.status, 'error')
  assert.equal(steps[1]?.message, '超时')
})

test('只有换行的正文当没答，思考还留着', () => {
  const projected = projectAgentMessage(
    chat({
      content: '\n\n',
      taskExecution: {
        taskId: '',
        status: 'processing',
        progress: 20,
        steps: [],
        reasoning: '先把问题想清楚。',
      },
    }),
  )
  assert.equal(projected.content, '')
  assert.equal(projected.thought, '先把问题想清楚。')
  assert.equal(projected.state, 'streaming')
})

test('等你回答时不是还在说，问句后面不该跟光标', () => {
  const projected = projectAgentMessage(
    chat({
      content: '',
      pendingQuestion: {
        questionId: 'q1',
        questionType: 'choice',
        question: '发给谁？',
        options: [{ value: 'a', label: '甲' }],
      },
      taskExecution: {
        taskId: 't',
        status: 'waiting',
        progress: 40,
        steps: [],
      },
    }),
  )
  assert.equal(projected.state, undefined)
  assert.equal(projected.question?.text, '发给谁？')
})

test('流式时只投影最后一条，前面的对象沿用', () => {
  syncProjectedMessages([
    chat({ id: 'u', role: 'user', content: '问' }),
    chat({ id: 'a', content: '答' }),
  ])
  const snap = getAgentMessagesSnapshot()
  syncProjectedMessages([
    chat({ id: 'u', role: 'user', content: '问' }),
    chat({ id: 'a', content: '答呀' }),
  ])
  const next = getAgentMessagesSnapshot()
  assert.equal(next[0], snap[0])
  assert.equal(next[1]?.content, '答呀')
  assert.notEqual(next[1], snap[1])
})
