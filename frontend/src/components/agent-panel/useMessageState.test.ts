import type { ChatMessage } from './engineTypes'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  emptyMessagesByMode,
  findMessageWhereInBag,
  mapMessagesById,
  writeModeMessages,
} from './useMessageState'

function msg(id: string, content: string): ChatMessage {
  return {
    id,
    sessionId: 's',
    role: 'assistant',
    content,
    createdAt: new Date(0),
  }
}

test('mode switch keeps the other mode’s messages', () => {
  let bag = emptyMessagesByMode()
  bag = writeModeMessages(bag, 'work', [msg('w1', '办事中')])
  bag = writeModeMessages(bag, 'chat', [msg('c1', '在聊')])
  assert.equal(bag.work[0].content, '办事中')
  assert.equal(bag.chat[0].content, '在聊')
  bag = writeModeMessages(bag, 'chat', [msg('c2', '换一句')])
  assert.equal(bag.work[0].id, 'w1')
  assert.equal(bag.chat[0].id, 'c2')
})

test('id updates still find a background Work message while Chat is visible', () => {
  let bag = emptyMessagesByMode()
  bag = writeModeMessages(bag, 'work', [msg('w1', '办事中')])
  bag = writeModeMessages(bag, 'chat', [msg('c1', '在聊')])
  bag = mapMessagesById(bag, 'w1', (message) => ({
    ...message,
    content: '办完了',
  }))
  assert.equal(bag.work[0].content, '办完了')
  assert.equal(bag.chat[0].content, '在聊')
})

test('predicate search still finds a Work confirmation while Chat is visible', () => {
  const work = msg('w1', '办事中')
  work.pendingQuestion = {
    questionId: 'confirmation:c1',
    confirmationId: 'c1',
    questionType: 'confirmation',
    question: '继续？',
    required: true,
  }
  let bag = emptyMessagesByMode()
  bag = writeModeMessages(bag, 'work', [work])
  bag = writeModeMessages(bag, 'chat', [msg('c1', '在聊')])
  const found = findMessageWhereInBag(
    bag,
    (message) =>
      message.pendingQuestion?.confirmationId === 'c1' && !message.selectedAnswer,
  )
  assert.equal(found?.id, 'w1')
})
