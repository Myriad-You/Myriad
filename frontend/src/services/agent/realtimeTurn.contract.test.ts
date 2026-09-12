import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { decideStreamDropAction } from './sseTransport'
import {
  acceptRunSequence,
  ChatTurnClock,
  isCurrentChatGeneration,
  isStreamSupersededError,
  STREAM_SUPERSEDED_MESSAGE,
} from './turnIdentity'

function source(relative: string): string {
  return readFileSync(new URL(relative, import.meta.url), 'utf8')
}

test('old Chat generation cannot keep applying after a newer Chat send', () => {
  const clock = new ChatTurnClock()
  const first = clock.next()
  const second = clock.next()
  assert.equal(isCurrentChatGeneration(first, second), false)
  const engine = source('../../components/agent-panel/AgentEngine.tsx')
  assert.match(engine, /isCurrentChatGeneration\(generation/)
  assert.match(engine, /setTurnGeneration\(chatGeneration\)/)
  // Motion and face must advance together.
  const facade = source('../../features/merope/engineFace.ts')
  assert.match(
    facade,
    /setTurnGeneration[^}]*setLiveMotionGeneration\(generation\)[^}]*agentFace\.setGeneration\(generation\)/,
  )
  const lifecycle = source('../../features/merope/performanceLifecycle.ts')
  assert.match(lifecycle, /acceptLiveMotionGeneration\(event\.generation\)/)
})

test('duplicate run sequences are dropped so speech and motion are not replayed', () => {
  const seen = new Map<string, number>()
  assert.equal(acceptRunSequence(seen, 'run_1', 4), true)
  assert.equal(acceptRunSequence(seen, 'run_1', 4), false)
  const transport = source('./sseTransport.ts')
  assert.match(transport, /seenSequences/)
  assert.match(transport, /acceptRunSequence/)
  const performance = source('../../features/merope/performanceLifecycle.ts')
  assert.match(performance, /activePlanKey/)
})

test('Chat abort does not take the Work SSE lane', () => {
  const api = source('./agentApi.ts')
  assert.match(api, /activeAbortControllersByMode/)
  assert.match(api, /lane === 'chat'/)
  assert.match(api, /lane === 'chat'/)
  const engine = source('../../components/agent-panel/AgentEngine.tsx')
  assert.match(engine, /abortCurrentRequest\(current\)/)
  assert.match(api, /session\/cancel-chat/)
})

test('Work completion cannot take an active Chat mouth', () => {
  const gate = source('../../features/merope/faceSpeechArbitration.ts')
  assert.match(gate, /incomingMode === 'work' && input\.chatUtteranceActive/)
  assert.match(gate, /record-without-speech/)
})

test('SSE disconnect resumes the same Work run instead of cancelling it', () => {
  assert.equal(
    decideStreamDropAction({
      hasFinalResponse: false,
      capturedRunId: 'run_work',
      capturedTaskId: 'task_work',
      abortIntent: null,
      hasStreamError: true,
    }),
    'resume_run',
  )
  assert.equal(
    decideStreamDropAction({
      hasFinalResponse: false,
      capturedRunId: 'run_work',
      capturedTaskId: 'task_work',
      abortIntent: 'user',
      hasStreamError: true,
    }),
    'reject_user_abort',
  )
  const process = source('../../../../backend/src/api/agent/process.rs')
  assert.match(process, /let run = create_run\(/)
  assert.match(process, /pub async fn cancel_task/)
  assert.match(process, /cancel_task_for_user/)
  assert.match(process, /claim_chat_turn/)
})

test('replace and cancel stay idempotent and do not look like faults', () => {
  assert.equal(
    isStreamSupersededError(new Error(STREAM_SUPERSEDED_MESSAGE)),
    true,
  )
  const speech = source('../../features/merope/agentFaceChannel.ts')
  assert.match(speech, /close\('cancel'\)/)
  const lease = source('../../features/merope/motion/speechLease.ts')
  assert.match(lease, /release/)
})

test('hidden face and motion timeout leave a local floor without blocking text', () => {
  const motion = source(
    '../../../../backend/src/services/agent/merope/motion.rs',
  )
  assert.match(motion, /fn face_is_hidden/)
  assert.match(motion, /MOTION_TOTAL_TIMEOUT/)
  assert.match(motion, /pub async fn refine_motion/)
  const overlay = source(
    '../../../../backend/src/services/agent/motion_overlay.rs',
  )
  const dropGuard = overlay
    .split('impl Drop for MotionRefinementGuard {')[1]
    ?.split('\n}')[0]
  assert.ok(dropGuard, 'turn-scoped refinement keeps a cancellation guard')
  assert.match(dropGuard, /self\.task\.abort\(\)/)
  const stream = source(
    '../../../../backend/src/services/agent/confirmation_and_tasks/chat_stream.rs',
  )
  // Do not drive motion from speech before the transport outlet.
  const textOutlet = stream.indexOf(
    'response_agent::emit_stream_delta(tx, delta).await',
  )
  const motionObserve = stream.indexOf('.observe(&text, tx)')
  assert.ok(textOutlet >= 0 && motionObserve > textOutlet)
})
