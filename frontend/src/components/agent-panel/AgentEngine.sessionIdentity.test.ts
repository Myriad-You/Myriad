import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { runInNewContext } from 'node:vm'
import ts from 'typescript'
import { authSubject } from '../../utils/authSubject'
import { SessionLoadScope } from './sessionLoadScope'

const source = ts.createSourceFile('AgentEngine.tsx', readFileSync(new URL('./AgentEngine.tsx', import.meta.url), 'utf8'), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
function callback(name: string): string {
  let found = ''
  function visit(node: ts.Node) {
    if (ts.isVariableDeclaration(node) && node.name.getText(source) === name) {
      const call = node.initializer as ts.CallExpression
      found = call.arguments[0].getText(source)
    }
    ts.forEachChild(node, visit)
  }
  visit(source)
  assert.ok(found, name)
  return ts.transpileModule(`globalThis.${name} = ${found}`, {
    compilerOptions: { target: ts.ScriptTarget.ES2022 },
  }).outputText
}
function harness(getMessages: (...args: unknown[]) => Promise<unknown[]>) {
  const writes: unknown[] = []
  const reattaches: unknown[] = []
  const scope = new SessionLoadScope()
  const noop = () => {}
  const context: Record<string, any> = {
    authSubject, console, sessionLoads: scope,
    sessionIdsByModeRef: { current: { work: null, chat: null } },
    sessionTitleSetByModeRef: { current: {} }, setSessionId: noop,
    stopAgoraConversation: noop, agentService: { getSessionMessages: getMessages },
    imageUrlsFromAgentPayload: () => [], executionStepsFromHistory: () => [],
    pendingQuestionFromMetadata: () => undefined,
    setMessages: (rows: unknown) => writes.push(rows),
    restorePendingActionFromMessages: () => null, clearAgentPendingAction: noop,
    restoreFollowUpQuestion: () => null,
    reattachLiveWork: (...args: unknown[]) => reattaches.push(args),
  }
  runInNewContext(callback('loadSession'), context)
  return { context, writes, reattaches, scope }
}
const rows = (content: string) => [{ id: 1, role: 'assistant', content, createdAt: '2026-09-12T00:00:00Z' }]

test('late history after identity loss cannot restore messages, pending actions or reconnect', async () => {
  const held = Promise.withResolvers<unknown[]>()
  const h = harness(() => held.promise)
  const pending = h.context.loadSession({ id: 'A', mode: 'work' })
  authSubject.change('B', true)
  held.resolve(rows('private A'))
  await pending
  assert.deepEqual(h.writes, [])
  assert.deepEqual(h.reattaches, [])
  h.scope.reset()
  authSubject.change('guest', true)
})

test('newer session load wins even when the previous response arrives last', async () => {
  const held = Promise.withResolvers<unknown[]>()
  const h = harness(async id => id === 'old' ? held.promise : rows('new'))
  const pending = h.context.loadSession({ id: 'old', mode: 'work' })
  await h.context.loadSession({ id: 'new', mode: 'work' })
  held.resolve(rows('old'))
  await pending
  assert.equal(h.writes.length, 1)
  assert.match(JSON.stringify(h.writes), /new/)
  assert.doesNotMatch(JSON.stringify(h.writes), /old/)
  h.scope.reset()
})

test('new session/disposal invalidates a pending restore; Chat and Work remain independent', async () => {
  const held = Promise.withResolvers<unknown[]>()
  const h = harness(async id => id === 'work' ? held.promise : rows('chat'))
  const pending = h.context.loadSession({ id: 'work', mode: 'work' })
  await h.context.loadSession({ id: 'chat', mode: 'chat' })
  assert.equal(h.reattaches.length, 0, 'Chat must not revive Work')
  h.scope.reset('work')
  held.resolve(rows('work'))
  await pending
  assert.equal(h.writes.length, 1)
})

test('a late task probe cannot start a stream after the owning load was cancelled', async () => {
  const held = Promise.withResolvers<unknown>()
  const h = harness(async () => [])
  const subject = h.scope.begin('work')
  const effects: string[] = []
  Object.assign(h.context, {
    collectReattachCandidates: () => [{ messageId: 'old', taskId: 'task', runId: 'run' }],
    isNonTerminalTaskStatus: () => true,
    agentService: { getTask: () => held.promise, subscribeRun: () => effects.push('stream') },
    updateMessage: () => effects.push('message'),
  })
  runInNewContext(callback('reattachLiveWork'), h.context)
  const pending = h.context.reattachLiveWork([], undefined, subject)
  h.scope.reset('work')
  held.resolve({ status: 'running' })
  await pending
  assert.deepEqual(effects, [])
})

test('identity loss during Work window-query preparation prevents the process POST', async () => {
  // Exercise the actual preparation block, not a second implementation of the guard.
  let preparation = ''
  function visit(node: ts.Node) {
    if (ts.isTryStatement(node) && node.tryBlock.getText(source).includes('const body = captureTurnBody')) {
      preparation = node.tryBlock.getText(source)
    }
    ts.forEachChild(node, visit)
  }
  visit(source)
  assert.ok(preparation)
  const held = Promise.withResolvers<unknown>()
  const entered = Promise.withResolvers<void>()
  const effects: string[] = []
  const subject = authSubject.signal
  const context: Record<string, any> = {
    subject, mode: 'work', modeSessionId: null, intentionId: undefined,
    location: { pathname: '/' }, getAgentContextConsent: () => false,
    window: {}, hasActionHandler: () => true,
    executeFrontendAction: (_action: unknown, signal: AbortSignal) => {
      assert.equal(signal, subject)
      entered.resolve()
      return held.promise
    },
    captureTurnBody: () => { effects.push('capture'); return {} },
    agentService: { processWithProgress: () => effects.push('post') },
  }
  runInNewContext(ts.transpileModule(`globalThis.prepare = async () => ${preparation}`, {
    compilerOptions: { target: ts.ScriptTarget.ES2022 },
  }).outputText, context)
  const pending = context.prepare()
  await entered.promise
  authSubject.change('B', true)
  held.resolve({ windows: [] })
  await pending
  assert.deepEqual(effects, [])
  authSubject.change('guest', true)
})
