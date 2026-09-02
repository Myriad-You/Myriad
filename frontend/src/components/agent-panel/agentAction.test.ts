import type { ConfirmationInfo } from '../../services/agent'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  agentActionExpired,
  agentActionImpacts,
  agentActionRemainingSeconds,
  agentActionTier,
  buildAgentPendingAction,
  normalizeActionRisk,
} from './agentAction'

const NOW = 1_700_000_000_000

function confirmation(
  overrides: Partial<ConfirmationInfo> = {},
): ConfirmationInfo {
  return {
    confirmationId: 'c1',
    riskLevel: 'high',
    expiresInSeconds: 300,
    pendingSteps: [
      {
        stepId: 's1',
        capabilityName: '删除文件',
        message: '删除 3 个文件',
        impact: ['无法恢复', '会影响引用它们的笔记'],
      },
    ],
    ...overrides,
  }
}

test('认不出来的风险按最重的算', () => {
  assert.equal(normalizeActionRisk('low'), 'low')
  assert.equal(normalizeActionRisk('CRITICAL'), 'critical')
  assert.equal(normalizeActionRisk('  medium  '), 'medium')
  // 猜轻了要出事，猜重了只是多问一句
  assert.equal(normalizeActionRisk('weird'), 'critical')
  assert.equal(normalizeActionRisk(undefined), 'critical')
  assert.equal(normalizeActionRisk(''), 'critical')
})

test('按问得多重分档，不按风险名字', () => {
  assert.equal(agentActionTier('none'), 'light')
  assert.equal(agentActionTier('low'), 'light')
  assert.equal(agentActionTier('medium'), 'preview')
  assert.equal(agentActionTier('high'), 'explicit')
  assert.equal(agentActionTier('critical'), 'explicit')
})

test('把确认请求转成卡片要的形状', () => {
  const action = buildAgentPendingAction({
    confirmation: confirmation(),
    prompt: '确定要删掉这些吗？',
    nowMs: NOW,
  })

  assert.equal(action.id, 'c1')
  assert.equal(action.risk, 'high')
  assert.equal(action.tier, 'explicit')
  assert.equal(action.prompt, '确定要删掉这些吗？')
  assert.equal(action.expiresAtMs, NOW + 300_000)
  assert.deepEqual(action.steps, [
    {
      id: 's1',
      name: '删除文件',
      message: '删除 3 个文件',
      impact: ['无法恢复', '会影响引用它们的笔记'],
    },
  ])
})

test('后端没给有效期就不显示倒计时，不自己编一个', () => {
  const action = buildAgentPendingAction({
    confirmation: confirmation({ expiresInSeconds: 0 }),
    prompt: '?',
    nowMs: NOW,
  })
  assert.equal(action.expiresAtMs, null)
  assert.equal(agentActionRemainingSeconds(action, NOW + 60_000), null)
  assert.equal(agentActionExpired(action, NOW + 10 ** 9), false)
})

test('倒计时存到期时刻而不是剩余秒数', () => {
  const action = buildAgentPendingAction({
    confirmation: confirmation(),
    prompt: '?',
    nowMs: NOW,
  })

  assert.equal(agentActionRemainingSeconds(action, NOW), 300)
  assert.equal(agentActionRemainingSeconds(action, NOW + 299_500), 1)
  assert.equal(agentActionExpired(action, NOW + 299_999), false)

  // 到点之后不给负数，也不再让人点确认
  assert.equal(agentActionRemainingSeconds(action, NOW + 400_000), 0)
  assert.equal(agentActionExpired(action, NOW + 300_000), true)
})

test('几个步骤报同一条影响时只列一遍', () => {
  const action = buildAgentPendingAction({
    confirmation: confirmation({
      pendingSteps: [
        {
          stepId: 's1',
          capabilityName: 'a',
          message: 'm',
          impact: ['无法恢复', ' 会通知订阅者 '],
        },
        {
          stepId: 's2',
          capabilityName: 'b',
          message: 'm',
          impact: ['无法恢复', '', '会清空缓存'],
        },
      ],
    }),
    prompt: '?',
    nowMs: NOW,
  })

  assert.deepEqual(agentActionImpacts(action), [
    '无法恢复',
    '会通知订阅者',
    '会清空缓存',
  ])
})

test('步骤缺 id 时给一个稳定的替代，别让列表 key 撞车', () => {
  const action = buildAgentPendingAction({
    confirmation: confirmation({
      pendingSteps: [
        { stepId: '', capabilityName: 'a', message: 'm', impact: [] },
        { stepId: '', capabilityName: 'b', message: 'm', impact: [] },
      ],
    }),
    prompt: '?',
    nowMs: NOW,
  })
  assert.deepEqual(
    action.steps.map((step) => step.id),
    ['step-0', 'step-1'],
  )
})
