import assert from 'node:assert/strict'
import test from 'node:test'
import {
  adoptRealtimeChatRun,
  bindRealtimeChat,
  parseVoiceRunNotice,
  realtimeChatSessionId,
} from './realtimeChat'

test('only the mounted Chat engine adopts realtime runs', () => {
  const adopted: string[] = []
  const unbind = bindRealtimeChat({
    sessionId: () => 'chat-session',
    adopt: (notice) => {
      adopted.push(notice.runId)
      return { messageId: `msg_${notice.runId}`, generation: notice.sequence }
    },
  })
  assert.equal(realtimeChatSessionId(), 'chat-session')
  assert.deepEqual(
    adoptRealtimeChatRun({
      runId: 'run-1',
      sessionId: 'chat-session',
      input: 'hello',
      providerTurnId: 7,
      sequence: 1,
    }),
    { messageId: 'msg_run-1', generation: 1 },
  )
  assert.deepEqual(adopted, ['run-1'])
  unbind()
  assert.throws(() => realtimeChatSessionId(), /not mounted/)
})

test('cloud notices are bounded and structurally validated', () => {
  assert.deepEqual(
    parseVoiceRunNotice({
      runId: 'run',
      sessionId: 'session',
      input: 'hello',
      providerTurnId: 7,
      sequence: 2,
    }),
    {
      runId: 'run',
      sessionId: 'session',
      input: 'hello',
      providerTurnId: 7,
      sequence: 2,
    },
  )
  for (const invalid of [
    null,
    { runId: '', sessionId: 's', input: 'x', providerTurnId: 7, sequence: 1 },
    { runId: 'r', sessionId: '', input: 'x', providerTurnId: 7, sequence: 1 },
    { runId: 'r', sessionId: 's', input: ' ', providerTurnId: 7, sequence: 1 },
    { runId: 'r', sessionId: 's', input: 'x', sequence: 1 },
    { runId: 'r', sessionId: 's', input: 'x', providerTurnId: -1, sequence: 1 },
    { runId: 'r', sessionId: 's', input: 'x', providerTurnId: 7, sequence: 0 },
    {
      runId: 'r',
      sessionId: 's',
      input: 'x',
      providerTurnId: 7,
      sequence: 1.5,
    },
  ]) {
    assert.equal(parseVoiceRunNotice(invalid), null)
  }
})
