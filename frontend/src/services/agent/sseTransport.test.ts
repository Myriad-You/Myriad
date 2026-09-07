import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import { ApiError } from '../api'
import {
  abortSseSubscriptions,
  agentHttpFailure,
  AgentStreamError,
  decideStreamDropAction,
  messageFromStepOutput,
  shouldYieldSsePaint,
} from './sseTransport'
import { STREAM_SUPERSEDED_MESSAGE } from './turnIdentity'

describe('executeSSERequest headers', () => {
  it('asks for an event stream so proxies do not buffer a JSON body', () => {
    const src = readFileSync(
      new URL('./sseTransport.ts', import.meta.url),
      'utf8',
    )
    assert.match(src, /Accept:\s*'text\/event-stream'/)
    assert.match(src, /getReader\(\)/)
  })

  it('does not drain a 200 SSE body to sniff CSRF', () => {
    const src = readFileSync(
      new URL('./sseTransport.ts', import.meta.url),
      'utf8',
    )
    assert.match(src, /response\.status === 403/)
    assert.doesNotMatch(
      src,
      /isCsrfBody\(response\.status,\s*await response\.clone\(\)\.text\(\)\)/,
    )
  })
})

describe('shouldYieldSsePaint', () => {
  it('breaks React batching for thinking and answer tokens', () => {
    assert.equal(shouldYieldSsePaint('thinking_token'), true)
    assert.equal(shouldYieldSsePaint('summary_token'), true)
    assert.equal(shouldYieldSsePaint('task_created'), false)
  })
})

describe('dev proxy pipes agent SSE', () => {
  it('does not treat process/stream as a buffered JSON body', () => {
    const src = readFileSync(
      new URL('../../../astro.config.mjs', import.meta.url),
      'utf8',
    )
    assert.match(src, /function isAgentSsePath/)
    assert.match(src, /\/api\/agent\/process\/stream/)
    assert.match(src, /isAgentSsePath\(originalUrl\)/)
  })
})

describe('decideStreamDropAction', () => {
  it('prefers a final completion response when present', () => {
    assert.equal(
      decideStreamDropAction({
        hasFinalResponse: true,
        capturedRunId: 'run_1',
        capturedTaskId: 'task_1',
        abortIntent: 'user',
        hasStreamError: true,
      }),
      'use_final',
    )
  })

  it('does not re-subscribe after intentional user abort even when runId is known', () => {
    assert.equal(
      decideStreamDropAction({
        hasFinalResponse: false,
        capturedRunId: 'run_abc',
        capturedTaskId: 'task_abc',
        abortIntent: 'user',
        hasStreamError: true,
      }),
      'reject_user_abort',
    )
  })

  it('does not re-subscribe when a newer request replaced the stream', () => {
    assert.equal(
      decideStreamDropAction({
        hasFinalResponse: false,
        capturedRunId: 'run_abc',
        capturedTaskId: null,
        abortIntent: 'replace',
        hasStreamError: true,
      }),
      'reject_replace',
    )
  })

  it('re-subscribes the same run after a transport drop (no abort intent)', () => {
    assert.equal(
      decideStreamDropAction({
        hasFinalResponse: false,
        capturedRunId: 'run_xyz',
        capturedTaskId: 'task_xyz',
        abortIntent: null,
        hasStreamError: true,
      }),
      'resume_run',
    )
  })

  it('re-subscribes after idle timeout abort intent', () => {
    assert.equal(
      decideStreamDropAction({
        hasFinalResponse: false,
        capturedRunId: 'run_timeout',
        capturedTaskId: null,
        abortIntent: 'timeout',
        hasStreamError: true,
      }),
      'resume_run',
    )
  })

  it('falls back to task polling when only taskId was observed', () => {
    assert.equal(
      decideStreamDropAction({
        hasFinalResponse: false,
        capturedRunId: null,
        capturedTaskId: 'task_only',
        abortIntent: undefined,
        hasStreamError: false,
      }),
      'poll_task',
    )
  })
})

describe('messageFromStepOutput', () => {
  it('unwraps Tapp envelopes for analyze, chat, and summarize', () => {
    assert.equal(
      messageFromStepOutput({
        format: 'json',
        value: { analysis: '分析正文', type: 'custom' },
        contextProvenance: [],
      }),
      '分析正文',
    )
    assert.equal(
      messageFromStepOutput({
        format: 'text',
        value: '回复正文',
        contextProvenance: [],
      }),
      '回复正文',
    )
    assert.equal(
      messageFromStepOutput({
        format: 'json',
        value: { summary: '摘要正文', style: 'brief' },
        contextProvenance: [],
      }),
      '摘要正文',
    )
  })

  it('still reads flat step output', () => {
    assert.equal(
      messageFromStepOutput({ analysis: '旧格式' }),
      '旧格式',
    )
  })

  it('does not treat image envelope url as the reply text', () => {
    assert.equal(
      messageFromStepOutput({
        format: 'image',
        value: { url: 'https://example.invalid/a.png', width: 1024, height: 768 },
        contextProvenance: [],
      }),
      undefined,
    )
  })
})

describe('sequence replay', () => {
  it('keeps seen sequences across resume so replayed events are dropped', () => {
    const src = readFileSync(
      new URL('./sseTransport.ts', import.meta.url),
      'utf8',
    )
    assert.match(src, /seenSequences/)
    assert.match(src, /acceptRunSequence/)
    assert.match(src, /startsWith\('id:'\)/)
    assert.match(src, /STREAM_SUPERSEDED_MESSAGE/)
  })
})

describe('abortSseSubscriptions', () => {
  it('marks controllers as user-aborted so drop recovery can read the intent', () => {
    const controllers = new Set<AbortController>()
    const controller = new AbortController()
    controllers.add(controller)

    abortSseSubscriptions(controllers, 'user')
    assert.equal(STREAM_SUPERSEDED_MESSAGE.includes('superseded'), true)

    assert.equal(controllers.size, 0)
    assert.equal(controller.signal.aborted, true)
    // Decision path uses the same intent label as abortSseSubscriptions('user')
    assert.equal(
      decideStreamDropAction({
        hasFinalResponse: false,
        capturedRunId: 'run_still_running',
        capturedTaskId: 'task_still_running',
        abortIntent: 'user',
        hasStreamError: true,
      }),
      'reject_user_abort',
    )
  })
})

describe('AgentStreamError', () => {
  it('keeps the backend code so quota rejections are distinguishable', () => {
    // The stream is HTTP 200 before anything can fail, so this code is the only
    // way a caller can tell a budget rejection from a processing failure.
    for (const code of [
      'AI_COOLDOWN_ACTIVE',
      'AI_DAILY_CALL_LIMIT',
      'AI_ANONYMOUS_DAILY_CALL_LIMIT',
      'AI_DAILY_TOKEN_LIMIT',
      'AI_ANONYMOUS_DAILY_TOKEN_LIMIT',
      'AI_QUOTA_EXCEEDED',
    ]) {
      const error = new AgentStreamError('nope', code)
      assert.equal(error.code, code)
      assert.equal(error.isQuotaRejection, true)
    }
  })

  it('does not treat processing failures as quota rejections', () => {
    for (const code of [
      'PROCESSING_ERROR',
      'RESUME_ERROR',
      'EXECUTION_ERROR',
      'QUEUE_FULL',
      'AI_QUOTA_LEDGER_ERROR',
      '',
    ]) {
      assert.equal(new AgentStreamError('boom', code).isQuotaRejection, false)
    }
  })

  it('stays a real Error so existing catch sites keep working', () => {
    const error = new AgentStreamError('boom', 'PROCESSING_ERROR')
    assert.ok(error instanceof Error)
    assert.equal(error.message, 'boom')
    assert.equal(error.name, 'AgentStreamError')
  })
})

describe('agentHttpFailure', () => {
  it('keeps the machine code and public message instead of dumping JSON', () => {
    const error = agentHttpFailure(
      429,
      JSON.stringify({
        error: 'Agent queue is full',
        code: 'QUEUE_FULL',
      }),
    )
    assert.ok(error instanceof ApiError)
    assert.equal(error.status, 429)
    assert.equal(error.code, 'QUEUE_FULL')
    assert.equal(error.message, 'Agent queue is full')
  })

  it('falls back to the raw body when the response is not JSON', () => {
    const error = agentHttpFailure(502, 'upstream exploded')
    assert.equal(error.status, 502)
    assert.equal(error.message, 'upstream exploded')
  })
})
