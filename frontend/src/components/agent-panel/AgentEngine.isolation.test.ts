import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

test('Work interrupt cannot abort Chat SSE, and session ids stay per mode', () => {
  const engine = readFileSync(
    new URL('./AgentEngine.tsx', import.meta.url),
    'utf8',
  )
  assert.match(engine, /abortCurrentRequest\(current\)/)
  assert.match(engine, /findMessageWhere/)
  assert.match(engine, /setSessionId\(event\.sessionId, mode\)/)
  assert.match(
    engine,
    /loadingByModeRef\.current\.work \|\| loadingByModeRef\.current\.chat/,
  )
  assert.match(engine, /restorePendingActionFromMessages/)
  assert.match(engine, /pendingQuestionFromMetadata/)

  const api = readFileSync(
    new URL('../../services/agent/agentApi.ts', import.meta.url),
    'utf8',
  )
  assert.match(api, /activeAbortControllersByMode/)
  assert.match(api, /context\?\.mode === 'chat' \? 'chat' : 'work'/)
  assert.match(api, /lane === 'chat'/)
  assert.match(engine, /chatTurnClockRef/)
  assert.match(engine, /isStreamSupersededError/)
  assert.match(engine, /isUserInterruptError/)
  assert.match(engine, /isCurrentChatGeneration/)
  // Both generations now advance in one facade call; assert it there.
  assert.match(engine, /setTurnGeneration\(/)
  assert.match(engine, /openTurnSpeech\(/)
  assert.doesNotMatch(engine, /SpeechSegmenter/)
  assert.doesNotMatch(engine, /turnSpeechPipeline/)
  const face = readFileSync(
    new URL('../../features/merope/engineFace.ts', import.meta.url),
    'utf8',
  )
  assert.match(face, /setLiveMotionGeneration/)
  assert.match(face, /agentFace\.setGeneration/)
  assert.match(face, /function openTurnSpeech/)
  assert.match(engine, /cancelChatTurn/)
  assert.match(engine, /turnSpeechAlreadyFed\(/)
  assert.match(face, /alreadyFed/)
  assert.doesNotMatch(engine, /if \(current === 'work'\) resetAgentStatus/)
  assert.doesNotMatch(engine, /if \(!otherRunning\) resetAgentStatus/)
  assert.match(engine, /resetAgentStatus\(\)/)
  assert.match(engine, /if \(otherRunning\) setAgentStatusThinking/)
  assert.match(engine, /setAgentLaneLoading/)
  assert.match(engine, /discardedResponseIdsRef/)
  assert.match(
    engine,
    /loadingMessageIdByModeRef\.current\[mode\] !== assistantMessageId/,
  )
  const composer = readFileSync(
    new URL('./AgentPanelComposer.tsx', import.meta.url),
    'utf8',
  )
  assert.match(composer, /agentStatusForLane\(status, laneLoading\)/)
  assert.match(composer, /agentStatusForLane\(island, laneLoading\)/)
  const panelFace = readFileSync(
    new URL('./AgentPanelFace.tsx', import.meta.url),
    'utf8',
  )
  assert.match(panelFace, /agentStatusActivity\(status\)/)
  assert.doesNotMatch(panelFace, /activityWhileChatIdle/)
  assert.doesNotMatch(panelFace, /useAgentLaneLoading/)
  const panel = readFileSync(
    new URL('./AgentPanel.tsx', import.meta.url),
    'utf8',
  )
  assert.match(panel, /agentStatusForLane\(island\.status, laneLoading\)/)
  assert.doesNotMatch(engine, /if \(mode === 'chat'\) return/)
  assert.match(engine, /mode !== 'chat'/)
  assert.match(engine, /chatPagePayload\(/)
  assert.match(engine, /mode !== 'chat' && hasActionHandler\('query_windows'\)/)
  assert.match(engine, /case 'thinking_token'/)
  assert.match(engine, /publishThinking/)
  assert.match(engine, /case 'outfit_overlay'/)
  assert.match(engine, /setChatOutfitOverlay/)
  assert.match(engine, /clearChatOutfitOverlay/)
})
