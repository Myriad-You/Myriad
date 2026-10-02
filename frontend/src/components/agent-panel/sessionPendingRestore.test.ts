import type { ChatMessage } from './engineTypes'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  pendingQuestionFromMetadata,
  restoreFollowUpQuestion,
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

test('Work approval metadata restores as an answerable question', () => {
  const question = pendingQuestionFromMetadata({
    task: {
      status: 'waiting_for_input',
      pendingQuestion: {
        questionId: 'q1',
        questionType: 'confirmation',
        question: '发送这封信？',
        options: [
          { value: 'confirm', label: '确认' },
          { value: 'cancel', label: '取消' },
        ],
        required: true,
      },
    },
  })
  assert.equal(question?.questionId, 'q1')
  assert.equal(question?.questionType, 'confirmation')
  assert.equal(question?.options?.length, 2)
})

test('follow-up questions restore the waiting prompt, answered ones do not', () => {
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
  assert.equal(restoreFollowUpQuestion([work]), '哪一天？')
  assert.equal(
    restoreFollowUpQuestion([{ ...work, selectedAnswer: 'tomorrow' }]),
    null,
  )
})
