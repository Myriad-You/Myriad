import type { ChatMessage } from './engineTypes'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  pendingQuestionFromMetadata,
  restoreFollowUpQuestion,
  restorePendingActionFromMessages,
} from './sessionPendingRestore'

const CREATED = 1_700_000_000_000

function assistant(overrides: Partial<ChatMessage> = {}): ChatMessage {
  return {
    id: 'm1',
    sessionId: 's1',
    role: 'assistant',
    content: '继续？',
    createdAt: new Date(CREATED),
    ...overrides,
  }
}

test('confirmation metadata restores id, risk, steps, and expiry clock', () => {
  const question = pendingQuestionFromMetadata(
    {
      pendingQuestion: {
        questionId: 'confirmation:c1',
        confirmationId: 'c1',
        questionType: 'confirmation',
        question: '发送这封信？',
        riskLevel: 'high',
        expiresInSeconds: 300,
        pendingSteps: [
          {
            stepId: 's1',
            capabilityName: 'mail.send',
            message: 'Send the note',
            impact: ['Writes mail'],
          },
        ],
      },
    },
    CREATED,
  )
  assert.equal(question?.confirmationId, 'c1')
  assert.equal(question?.riskLevel, 'high')
  assert.equal(question?.receivedAtMs, CREATED)
  assert.deepEqual(question?.pendingSteps, [
    {
      stepId: 's1',
      capabilityName: 'mail.send',
      message: 'Send the note',
      impact: ['Writes mail'],
    },
  ])
})

test('opening a session rebuilds the confirmation card from the last unanswered Work message', () => {
  const work = assistant({
    pendingQuestion: {
      questionId: 'confirmation:c1',
      confirmationId: 'c1',
      questionType: 'confirmation',
      question: '发送这封信？',
      riskLevel: 'high',
      expiresInSeconds: 300,
      receivedAtMs: CREATED,
      pendingSteps: [
        {
          stepId: 's1',
          capabilityName: 'mail.send',
          message: 'Send the note',
          impact: ['Writes mail'],
        },
      ],
    },
    taskExecution: {
      taskId: 'confirmation:c1',
      runId: 'run_1',
      status: 'waiting',
      progress: 50,
      steps: [],
    },
  })
  const action = restorePendingActionFromMessages([work], CREATED + 60_000)
  assert.equal(action?.id, 'c1')
  assert.equal(action?.risk, 'high')
  assert.equal(action?.prompt, '发送这封信？')
  assert.equal(action?.expiresAtMs, CREATED + 300_000)
  assert.equal(action?.steps[0]?.name, 'mail.send')
})

test('an already-answered confirmation is not restored', () => {
  const work = assistant({
    selectedAnswer: 'confirm',
    pendingQuestion: {
      questionId: 'confirmation:c1',
      confirmationId: 'c1',
      questionType: 'confirmation',
      question: '发送这封信？',
    },
    taskExecution: {
      taskId: 'confirmation:c1',
      status: 'waiting',
      progress: 50,
      steps: [],
    },
  })
  assert.equal(restorePendingActionFromMessages([work], CREATED), null)
})

test('follow-up questions restore as text, not a confirmation card', () => {
  const work = assistant({
    pendingQuestion: {
      questionId: 'q2',
      questionType: 'free_text',
      question: '哪一天？',
    },
    taskExecution: {
      taskId: 't1',
      runId: 'run_1',
      status: 'waiting',
      progress: 40,
      steps: [],
    },
  })
  assert.equal(restorePendingActionFromMessages([work], CREATED), null)
  assert.equal(restoreFollowUpQuestion([work]), '哪一天？')
})
