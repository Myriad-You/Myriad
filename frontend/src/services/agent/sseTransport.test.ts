import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  abortSseSubscriptions,
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
