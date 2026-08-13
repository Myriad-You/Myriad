import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  abortSseSubscriptions,
  AgentStreamError,
  decideStreamDropAction,
} from './sseTransport'

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

describe('abortSseSubscriptions', () => {
  it('marks controllers as user-aborted so drop recovery can read the intent', () => {
    const controllers = new Set<AbortController>()
    const controller = new AbortController()
    controllers.add(controller)

    abortSseSubscriptions(controllers, 'user')

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
