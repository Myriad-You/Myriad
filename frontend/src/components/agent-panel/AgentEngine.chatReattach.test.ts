import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

const engine = readFileSync(new URL('./AgentEngine.tsx', import.meta.url), 'utf8')
const api = readFileSync(new URL('../../services/agent/agentApi.ts', import.meta.url), 'utf8')

test('a chat loaded while its reply is still being written takes the run up', () => {
  assert.match(api, /sessions\/\$\{encodeURIComponent\(sessionId\)\}\/live-run/)
  assert.match(engine, /else void reattachLiveChat\(session\.id, loaded, subject\)/)
  const reattach = engine.slice(engine.indexOf('const reattachLiveChat'), engine.indexOf('const loadSession'))
  assert.match(reattach, /getLiveSessionRun\(chatSessionId, subject\)/)
  // Not twice: not over a turn this page is writing, nor over a reply already loaded.
  assert.match(reattach, /loadingMessageIdByModeRef\.current\.chat\) return/)
  assert.match(reattach, /message\.taskExecution\?\.runId === runId\)\) return/)
  assert.match(reattach, /followChatRunRef\.current\?\.\(runId, chatSessionId\)/)
})

test('a run this page did not start is shown, not spoken again', () => {
  const follow = engine.slice(engine.indexOf('const followChatRun'), engine.indexOf('followChatRunRef.current = followChatRun'))
  assert.match(follow, /createProgressHandler\(messageId, 'chat', generation, 'external', runId\)/)
  assert.match(follow, /handleAgentResponse\(messageId, response, 'chat', generation, 'external'\)/)
  assert.match(engine, /adopt: \(notice\) => followChatRun\(notice\.runId, notice\.sessionId, notice\.input\)/)
})

test('opened anew, the panel takes up the conversation it was last talked in', () => {
  assert.match(api, /`\$\{this\.baseUrl\}\/panel-chat`/)
  const takeUp = engine.slice(engine.indexOf('const takeUp'), engine.indexOf('const openSession'))
  assert.match(takeUp, /getPanelChat\(subject\)/)
  // Not over a conversation this page already has, or one being written.
  assert.match(takeUp, /sessionIdsByModeRef\.current\.chat \|\|/)
  assert.match(takeUp, /loadingByModeRef\.current\.chat \|\|/)
  assert.match(takeUp, /messagesRef\.current\.chat\.length > 0/)
  assert.match(takeUp, /loadSession\(\{ \.\.\.session, mode: 'chat' \}, undefined, 'chat'\)/)
  // Again for whoever signs in.
  assert.match(takeUp, /authSubject\.subscribe\(\(\) => void takeUp\(\)\)/)
})

test('her lines about how pieces ended reach the conversation they came from', () => {
  assert.match(engine, /setPieceLineSink\(\(chat, lines\) => \{/)
  assert.match(engine, /if \(sessionIdsByModeRef\.current\.chat !== chat\) return/)
  assert.match(engine, /withToldLines\(prev, restored\), 'chat'/)
  assert.match(engine, /Array\.isArray\(responseData\.told\)/)
})
